//! Typed JSON records; encryption errors propagate before the backing write.
use crate::TtlCache;
use forklaunch_core::{
    common::FrameworkResult,
    persistence::{TenantContext, json::JsonRecordCodec},
};
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;

pub struct JsonCache<C> {
    cache: C,
    codec: JsonRecordCodec,
}

impl<C: TtlCache> JsonCache<C> {
    #[must_use]
    pub fn new(cache: C, codec: JsonRecordCodec) -> Self {
        Self { cache, codec }
    }

    #[must_use]
    pub fn into_inner(self) -> C {
        self.cache
    }

    /// # Errors
    /// Propagates serialization, encryption, invalid TTL and backend failures.
    pub fn put_record<T: Serialize + ?Sized>(
        &mut self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
        tenant: Option<&TenantContext>,
    ) -> FrameworkResult<()> {
        self.cache.set(key, self.codec.encode(value, tenant)?, ttl)
    }

    /// Missing keys are `None`; authentication and JSON failures remain errors.
    /// # Errors
    /// Propagates backend, JSON and decryption failures.
    pub fn read_record<T: DeserializeOwned>(
        &mut self,
        key: &str,
        tenant: Option<&TenantContext>,
    ) -> FrameworkResult<Option<T>> {
        self.cache
            .get(key)?
            .map(|bytes| self.codec.decode(&bytes, tenant))
            .transpose()
    }

    /// Serializes every entry before writing any of them.
    /// # Errors
    /// Propagates serialization, encryption and backend failures.
    pub fn put_batch_records<T: Serialize>(
        &mut self,
        entries: Vec<(String, T)>,
        ttl: Option<Duration>,
        tenant: Option<&TenantContext>,
    ) -> FrameworkResult<()> {
        let entries = entries
            .into_iter()
            .map(|(key, value)| Ok((key, self.codec.encode(&value, tenant)?)))
            .collect::<FrameworkResult<Vec<_>>>()?;
        self.cache.put_batch(entries, ttl)
    }

    /// Preserves input order and missing keys, including false, zero and null values.
    /// # Errors
    /// Propagates backend, JSON and authentication failures.
    pub fn read_batch_records<T: DeserializeOwned>(
        &mut self,
        keys: &[String],
        tenant: Option<&TenantContext>,
    ) -> FrameworkResult<Vec<Option<T>>> {
        self.cache
            .get_batch(keys)?
            .into_iter()
            .map(|bytes| {
                bytes
                    .map(|bytes| self.codec.decode(&bytes, tenant))
                    .transpose()
            })
            .collect()
    }

    /// # Errors
    /// Propagates backend failures.
    pub fn delete_record(&mut self, key: &str) -> FrameworkResult<bool> {
        self.cache.delete(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InMemoryRedisTtlCache, RedisTtlCacheConfig};
    use forklaunch_core::persistence::encryption::FieldEncryptor;
    use serde::Serializer;

    struct Fallible(bool);
    impl Serialize for Fallible {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            if self.0 {
                serializer.serialize_bool(false)
            } else {
                Err(serde::ser::Error::custom("secret must never escape"))
            }
        }
    }

    #[test]
    fn serialization_failure_does_not_commit_a_partial_batch_or_expose_secrets() {
        let cache = InMemoryRedisTtlCache::new(RedisTtlCacheConfig::new(
            "redis://fixture",
            Duration::from_secs(60),
        ))
        .unwrap();
        let mut cache = JsonCache::new(
            cache,
            JsonRecordCodec::new(FieldEncryptor::new("fixture-key").unwrap()),
        );
        let tenant = TenantContext::new("tenant-a").unwrap();
        let error = cache
            .put_batch_records(
                vec![
                    ("first".into(), Fallible(true)),
                    ("second".into(), Fallible(false)),
                ],
                None,
                Some(&tenant),
            )
            .unwrap_err();
        assert_eq!(error.code(), "record_json_invalid");
        assert!(!error.to_string().contains("secret"));
        assert!(
            cache
                .read_record::<bool>("first", Some(&tenant))
                .unwrap()
                .is_none()
        );
    }
}
