//! Redis-backed binary TTL records and FIFO queues.
//!
//! This synchronous adapter implements the framework's synchronous cache trait.
//! In async handlers use a blocking executor; socket operations have bounded timeouts.
use super::{RedisTtlCacheConfig, TtlCache, normalize_key};
use forklaunch_core::common::{FrameworkError, FrameworkResult};
use redis::{Connection, FromRedisValue};
use std::{collections::BTreeSet, time::Duration};

const TIMEOUT: Duration = Duration::from_secs(5);

fn backend_error(_: redis::RedisError) -> FrameworkError {
    // Server error strings and connection URLs can contain credentials or data.
    FrameworkError::new("redis_backend_error", "Redis operation failed")
}

fn connect(url: &str) -> FrameworkResult<Connection> {
    let client = redis::Client::open(url).map_err(backend_error)?;
    let connection = client
        .get_connection_with_timeout(TIMEOUT)
        .map_err(backend_error)?;
    connection
        .set_read_timeout(Some(TIMEOUT))
        .map_err(backend_error)?;
    connection
        .set_write_timeout(Some(TIMEOUT))
        .map_err(backend_error)?;
    Ok(connection)
}

fn ttl_millis(ttl: Duration) -> FrameworkResult<u64> {
    let millis = u64::try_from(ttl.as_millis())
        .ok()
        .filter(|millis| *millis > 0 && i64::try_from(*millis).is_ok());
    millis.ok_or_else(|| {
        FrameworkError::new(
            "redis_cache_invalid_ttl",
            "TTL must be a positive Redis millisecond value",
        )
    })
}

/// A cache using Redis expiration; values survive clients and processes.
pub struct RedisTtlCache {
    config: RedisTtlCacheConfig,
    connection: Connection,
}

impl RedisTtlCache {
    /// # Errors
    /// Returns an error for invalid configuration or connection failure.
    pub fn connect(config: RedisTtlCacheConfig) -> FrameworkResult<Self> {
        config.validate()?;
        ttl_millis(config.default_ttl())?;
        let connection = connect(&config.url)?;
        Ok(Self { config, connection })
    }

    /// Lists live keys with a literal prefix, using SCAN instead of blocking KEYS.
    ///
    /// # Errors
    /// Returns an error when Redis rejects a scan.
    pub fn list_keys_by_prefix(&mut self, prefix: &str) -> FrameworkResult<Vec<String>> {
        let mut pattern = String::new();
        for character in prefix.chars() {
            if matches!(character, '*' | '?' | '[' | ']' | '\\') {
                pattern.push('\\');
            }
            pattern.push(character);
        }
        pattern.push('*');
        let mut cursor = 0_u64;
        let mut keys = BTreeSet::new();
        loop {
            let (next, page): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(256)
                .query(&mut self.connection)
                .map_err(backend_error)?;
            keys.extend(page);
            cursor = next;
            if cursor == 0 {
                break;
            }
        }
        Ok(keys.into_iter().collect())
    }
}

impl TtlCache for RedisTtlCache {
    fn set(
        &mut self,
        key: impl Into<String>,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> FrameworkResult<()> {
        let key = normalize_key(key)?;
        let millis = ttl_millis(ttl.unwrap_or_else(|| self.config.default_ttl()))?;
        redis::cmd("SET")
            .arg(key)
            .arg(value)
            .arg("PX")
            .arg(millis)
            .query(&mut self.connection)
            .map_err(backend_error)
    }
    fn get(&mut self, key: &str) -> FrameworkResult<Option<Vec<u8>>> {
        redis::cmd("GET")
            .arg(normalize_key(key)?)
            .query(&mut self.connection)
            .map_err(backend_error)
    }
    fn delete(&mut self, key: &str) -> FrameworkResult<bool> {
        redis::cmd("DEL")
            .arg(normalize_key(key)?)
            .query(&mut self.connection)
            .map_err(backend_error)
    }
    fn peek(&mut self, key: &str) -> FrameworkResult<bool> {
        redis::cmd("EXISTS")
            .arg(normalize_key(key)?)
            .query(&mut self.connection)
            .map_err(backend_error)
    }
    fn put_batch(
        &mut self,
        entries: Vec<(String, Vec<u8>)>,
        ttl: Option<Duration>,
    ) -> FrameworkResult<()> {
        let millis = ttl_millis(ttl.unwrap_or_else(|| self.config.default_ttl()))?;
        let entries: Vec<_> = entries
            .into_iter()
            .map(|(key, value)| Ok((normalize_key(key)?, value)))
            .collect::<FrameworkResult<_>>()?;
        if entries.is_empty() {
            return Ok(());
        }
        let mut pipeline = redis::pipe();
        pipeline.atomic();
        for (key, value) in entries {
            pipeline
                .cmd("SET")
                .arg(key)
                .arg(value)
                .arg("PX")
                .arg(millis)
                .ignore();
        }
        pipeline.query(&mut self.connection).map_err(backend_error)
    }
    fn get_batch(&mut self, keys: &[String]) -> FrameworkResult<Vec<Option<Vec<u8>>>> {
        let keys: Vec<_> = keys
            .iter()
            .map(|key| normalize_key(key.as_str()))
            .collect::<FrameworkResult<_>>()?;
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        redis::cmd("MGET")
            .arg(keys)
            .query(&mut self.connection)
            .map_err(backend_error)
    }
    fn delete_batch(&mut self, keys: &[String]) -> FrameworkResult<usize> {
        let keys: Vec<_> = keys
            .iter()
            .map(|key| normalize_key(key.as_str()))
            .collect::<FrameworkResult<_>>()?;
        if keys.is_empty() {
            return Ok(0);
        }
        redis::cmd("DEL")
            .arg(keys)
            .query(&mut self.connection)
            .map_err(backend_error)
    }
}

/// A shared FIFO queue backed by a Redis list. Dequeue removes an item atomically;
/// use a worker queue with receipts/leases when processing requires acknowledgments.
pub struct RedisQueue {
    key: String,
    connection: Connection,
}

impl RedisQueue {
    /// # Errors
    /// Returns an error for an empty key or connection failure.
    pub fn connect(url: &str, key: impl Into<String>) -> FrameworkResult<Self> {
        Ok(Self {
            key: normalize_key(key)?,
            connection: connect(url)?,
        })
    }

    fn query<T: FromRedisValue>(&mut self, command: &mut redis::Cmd) -> FrameworkResult<T> {
        command.query(&mut self.connection).map_err(backend_error)
    }

    /// # Errors
    /// Returns an error if Redis rejects the enqueue.
    pub fn enqueue(&mut self, value: &[u8]) -> FrameworkResult<()> {
        let _: usize = self.query(redis::cmd("RPUSH").arg(self.key.clone()).arg(value))?;
        Ok(())
    }
    /// # Errors
    /// Returns an error if Redis rejects the batch. Order matches input order.
    pub fn enqueue_batch(&mut self, values: &[Vec<u8>]) -> FrameworkResult<()> {
        if values.is_empty() {
            return Ok(());
        }
        let _: usize = self.query(redis::cmd("RPUSH").arg(self.key.clone()).arg(values))?;
        Ok(())
    }
    /// # Errors
    /// Returns an error if Redis rejects the dequeue.
    pub fn dequeue(&mut self) -> FrameworkResult<Option<Vec<u8>>> {
        self.query(redis::cmd("LPOP").arg(self.key.clone()))
    }
    /// # Errors
    /// Returns an error if Redis rejects the batch dequeue (requires Redis 6.2+).
    pub fn dequeue_batch(&mut self, max: usize) -> FrameworkResult<Vec<Vec<u8>>> {
        if max == 0 {
            return Ok(Vec::new());
        }
        let values: Option<Vec<Vec<u8>>> =
            self.query(redis::cmd("LPOP").arg(self.key.clone()).arg(max))?;
        Ok(values.unwrap_or_default())
    }
    /// # Errors
    /// Returns an error if Redis rejects the peek.
    pub fn peek(&mut self) -> FrameworkResult<Option<Vec<u8>>> {
        self.query(redis::cmd("LINDEX").arg(self.key.clone()).arg(0))
    }
    /// # Errors
    /// Returns an error if Redis rejects the length query.
    pub fn len(&mut self) -> FrameworkResult<usize> {
        self.query(redis::cmd("LLEN").arg(self.key.clone()))
    }
    /// # Errors
    /// Returns an error if Redis rejects the length query.
    pub fn is_empty(&mut self) -> FrameworkResult<bool> {
        Ok(self.len()? == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires FORKLAUNCH_TEST_REDIS_URL; exercised in container CI"]
    fn redis_backend_persists_expires_batches_and_consumes_fifo_across_clients() {
        let url = std::env::var("FORKLAUNCH_TEST_REDIS_URL").expect("Redis test URL required");
        let prefix = format!(
            "fl-parity:{}:{}:",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let config = RedisTtlCacheConfig::new(&url, Duration::from_secs(60));
        let mut writer = RedisTtlCache::connect(config.clone()).unwrap();
        let mut reader = RedisTtlCache::connect(config).unwrap();
        let key = format!("{prefix}binary");
        writer.set(&key, vec![0, 255, 1], None).unwrap();
        assert_eq!(reader.get(&key).unwrap(), Some(vec![0, 255, 1]));
        assert!(reader.peek(&key).unwrap());
        let expiry = format!("{prefix}expiry");
        writer
            .set(
                &expiry,
                b"expires".to_vec(),
                Some(Duration::from_millis(20)),
            )
            .unwrap();
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(reader.get(&expiry).unwrap(), None);
        assert!(writer.set(&key, vec![], Some(Duration::ZERO)).is_err());
        assert!(
            writer
                .set(&key, vec![], Some(Duration::from_nanos(1)))
                .is_err()
        );
        writer
            .put_batch(
                vec![
                    (format!("{prefix}a"), b"a".to_vec()),
                    (format!("{prefix}b"), b"b".to_vec()),
                ],
                None,
            )
            .unwrap();
        assert_eq!(
            reader
                .get_batch(&[
                    format!("{prefix}b"),
                    format!("{prefix}missing"),
                    format!("{prefix}a")
                ])
                .unwrap(),
            vec![Some(b"b".to_vec()), None, Some(b"a".to_vec())]
        );
        assert_eq!(reader.list_keys_by_prefix(&prefix).unwrap().len(), 3);
        let literal = format!("{prefix}[literal]*?");
        writer.set(&literal, vec![1], None).unwrap();
        assert_eq!(
            reader.list_keys_by_prefix(&literal).unwrap(),
            vec![literal.clone()]
        );
        // Invalid batches are rejected before any writes occur.
        let not_written = format!("{prefix}invalid-batch");
        assert!(
            writer
                .put_batch(
                    vec![(not_written.clone(), vec![1]), (" ".to_owned(), vec![2])],
                    None
                )
                .is_err()
        );
        assert_eq!(reader.get(&not_written).unwrap(), None);
        let keys = reader.list_keys_by_prefix(&prefix).unwrap();
        assert_eq!(reader.delete_batch(&keys).unwrap(), 4);
        assert!(!reader.delete(&key).unwrap());
        assert_eq!(
            reader.get_batch(&[]).unwrap(),
            Vec::<Option<Vec<u8>>>::new()
        );
        assert_eq!(reader.delete_batch(&[]).unwrap(), 0);
        writer.put_batch(vec![], None).unwrap();
        let queue_key = format!("{prefix}queue");
        let mut producer = RedisQueue::connect(&url, &queue_key).unwrap();
        let mut consumer = RedisQueue::connect(&url, &queue_key).unwrap();
        producer.enqueue(&[0, 255]).unwrap();
        producer
            .enqueue_batch(&[b"second".to_vec(), b"third".to_vec()])
            .unwrap();
        assert_eq!(consumer.peek().unwrap(), Some(vec![0, 255]));
        assert_eq!(consumer.len().unwrap(), 3);
        assert_eq!(consumer.dequeue_batch(0).unwrap(), Vec::<Vec<u8>>::new());
        assert_eq!(consumer.dequeue().unwrap(), Some(vec![0, 255]));
        assert_eq!(
            consumer.dequeue_batch(10).unwrap(),
            vec![b"second".to_vec(), b"third".to_vec()]
        );
        assert!(consumer.is_empty().unwrap());
        assert_eq!(consumer.dequeue().unwrap(), None);
        assert!(consumer.dequeue_batch(1).unwrap().is_empty());
    }
}
