//! Async Amazon S3 adapter using an injected official AWS SDK client.
use std::{future::Future, time::Duration};

use super::{AsyncObjectStore, ObjectMetadata, S3ObjectStoreConfig, StoredObject, normalize_key};
pub use aws_sdk_s3;
use aws_sdk_s3::{
    Client,
    primitives::ByteStream,
    types::{BucketLocationConstraint, CreateBucketConfiguration},
};
use forklaunch_core::common::{FrameworkError, FrameworkResult};

/// Resource bounds and a namespace applied to every operation.
#[derive(Debug, Clone)]
pub struct AwsS3Options {
    pub prefix: String,
    pub max_object_bytes: usize,
    pub max_list_bytes: usize,
    pub max_list_objects: usize,
    pub operation_timeout: Duration,
}
impl Default for AwsS3Options {
    fn default() -> Self {
        Self {
            prefix: String::new(),
            max_object_bytes: 16 * 1024 * 1024,
            max_list_bytes: 64 * 1024 * 1024,
            max_list_objects: 1000,
            operation_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectPage {
    pub objects: Vec<StoredObject>,
    pub next_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AwsS3ObjectStore {
    client: Client,
    config: S3ObjectStoreConfig,
    options: AwsS3Options,
}
impl AwsS3ObjectStore {
    /// The caller supplies credentials, endpoint, TLS and retry policy through `client`.
    /// Construction makes no network requests and never creates a bucket.
    /// # Errors
    /// Rejects invalid configuration, namespace prefixes, and zero resource bounds.
    pub fn new(
        client: Client,
        config: S3ObjectStoreConfig,
        mut options: AwsS3Options,
    ) -> FrameworkResult<Self> {
        config.validate()?;
        if options.max_object_bytes == 0
            || options.max_list_bytes == 0
            || options.max_list_objects == 0
            || options.operation_timeout.is_zero()
        {
            return Err(error(
                "s3_config_invalid",
                "object, listing and timeout limits must be positive",
            ));
        }
        if !options.prefix.is_empty() {
            options.prefix = format!("{}/", normalize_key(&options.prefix)?.trim_end_matches('/'));
            if options.prefix.len() >= 1024 {
                return Err(error("s3_config_invalid", "namespace prefix is too long"));
            }
        }
        Ok(Self {
            client,
            config,
            options,
        })
    }

    #[must_use]
    pub fn config(&self) -> &S3ObjectStoreConfig {
        &self.config
    }

    fn bounded<T>(
        &self,
        future: impl Future<Output = FrameworkResult<T>>,
    ) -> impl Future<Output = FrameworkResult<T>> {
        let deadline = tokio::time::timeout(self.options.operation_timeout, Box::pin(future));
        async move {
            deadline
                .await
                .map_err(|_| error("s3_timeout", "object store operation exceeded its deadline"))?
        }
    }

    fn wire_key(&self, key: &str) -> FrameworkResult<(String, String)> {
        let key = normalize_key(key)?;
        let wire = format!("{}{key}", self.options.prefix);
        if wire.len() > 1024 {
            return Err(error(
                "s3_object_invalid_key",
                "object key exceeds 1024 UTF-8 bytes",
            ));
        }
        Ok((key, wire))
    }

    /// Checks bucket access and optionally creates an absent bucket. Permission,
    /// credential and transport failures never trigger creation.
    /// # Errors
    /// Returns an error on inaccessible buckets, failed creation, or timeout.
    pub async fn init_bucket(&self, create_if_missing: bool) -> FrameworkResult<()> {
        self.bounded(async {
            match self
                .client
                .head_bucket()
                .bucket(&self.config.bucket)
                .send()
                .await
            {
                Ok(_) => return Ok(()),
                Err(failure)
                    if failure
                        .as_service_error()
                        .is_some_and(aws_sdk_s3::operation::head_bucket::HeadBucketError::is_not_found)
                        || failure
                            .raw_response()
                            .is_some_and(|response| response.status().as_u16() == 404) =>
                {
                    if !create_if_missing {
                        return Err(error(
                            "s3_bucket_missing",
                            "configured bucket does not exist",
                        ));
                    }
                }
                Err(_) => return Err(request_error()),
            }
            let mut request = self.client.create_bucket().bucket(&self.config.bucket);
            let region = self
                .config
                .region
                .as_deref()
                .or_else(|| self.client.config().region().map(aws_sdk_s3::config::Region::as_ref));
            if let Some(region) = region.filter(|region| *region != "us-east-1") {
                request = request.create_bucket_configuration(
                    CreateBucketConfiguration::builder()
                        .location_constraint(BucketLocationConstraint::from(region))
                        .build(),
                );
            }
            match request.send().await {
                Ok(_) => Ok(()),
                Err(failure)
                    if failure
                        .as_service_error()
                        .is_some_and(aws_sdk_s3::operation::create_bucket::CreateBucketError::is_bucket_already_owned_by_you) =>
                {
                    Ok(())
                }
                Err(_) => Err(request_error()),
            }
        })
        .await
    }

    async fn get_inner(&self, key: &str) -> FrameworkResult<Option<StoredObject>> {
        let (key, wire) = self.wire_key(key)?;
        let mut output = match self
            .client
            .get_object()
            .bucket(&self.config.bucket)
            .key(&wire)
            .send()
            .await
        {
            Ok(output) => output,
            Err(failure)
                if failure.as_service_error().is_some_and(
                    aws_sdk_s3::operation::get_object::GetObjectError::is_no_such_key,
                ) =>
            {
                return Ok(None);
            }
            Err(_) => return Err(request_error()),
        };
        if output.content_length().is_some_and(|length| {
            length < 0
                || u64::try_from(length)
                    .is_ok_and(|length| length > self.options.max_object_bytes as u64)
        }) {
            return Err(size_error());
        }
        let content_type = output.content_type().map(str::to_owned);
        let mut body = Vec::new();
        while let Some(chunk) = output.body.try_next().await.map_err(|_| request_error())? {
            if chunk.len() > self.options.max_object_bytes.saturating_sub(body.len()) {
                return Err(size_error());
            }
            body.extend_from_slice(&chunk);
        }
        let tags = self
            .client
            .get_object_tagging()
            .bucket(&self.config.bucket)
            .key(&wire)
            .set_version_id(output.version_id)
            .send()
            .await
            .map_err(|_| request_error())?;
        let metadata = ObjectMetadata {
            content_type,
            tags: tags
                .tag_set()
                .iter()
                .map(|tag| (tag.key().to_owned(), tag.value().to_owned()))
                .collect(),
        };
        Ok(Some(StoredObject {
            key,
            body,
            metadata,
        }))
    }

    /// Returns one bounded page. Continuation tokens are opaque and scoped to the
    /// same bucket/namespace/prefix; concurrent changes are not snapshot isolated.
    /// # Errors
    /// Rejects invalid limits, prefixes, backend failures, and resource overflow.
    pub async fn list_page(
        &self,
        prefix: &str,
        token: Option<&str>,
        limit: usize,
    ) -> FrameworkResult<ObjectPage> {
        self.bounded(self.list_page_inner(prefix, token, limit))
            .await
    }

    async fn list_page_inner(
        &self,
        prefix: &str,
        token: Option<&str>,
        limit: usize,
    ) -> FrameworkResult<ObjectPage> {
        if prefix.contains("..")
            || !(1..=1000).contains(&limit)
            || limit > self.options.max_list_objects
        {
            return Err(error("s3_list_invalid", "invalid prefix or page size"));
        }
        let wire_prefix = format!("{}{prefix}", self.options.prefix);
        let output = self
            .client
            .list_objects_v2()
            .bucket(&self.config.bucket)
            .prefix(&wire_prefix)
            .set_continuation_token(token.map(str::to_owned))
            .max_keys(
                i32::try_from(limit).map_err(|_| error("s3_list_invalid", "invalid page size"))?,
            )
            .send()
            .await
            .map_err(|_| request_error())?;
        if output.contents().len() > limit {
            return Err(error(
                "s3_list_limit",
                "object listing exceeds configured bounds",
            ));
        }
        let next_token = output.next_continuation_token().map(str::to_owned);
        if output.is_truncated() == Some(true) && next_token.is_none() {
            return Err(error(
                "s3_list_invalid",
                "truncated listing has no continuation token",
            ));
        }
        let mut objects = Vec::new();
        let mut bytes = 0usize;
        for object in output.contents() {
            let wire = object
                .key()
                .filter(|key| key.starts_with(&wire_prefix))
                .ok_or_else(|| error("s3_list_invalid", "object listing escaped its prefix"))?;
            let key = wire
                .strip_prefix(&self.options.prefix)
                .ok_or_else(|| error("s3_list_invalid", "object listing escaped its namespace"))?;
            if let Some(object) = self.get_inner(key).await? {
                bytes = bytes
                    .checked_add(object.body.len())
                    .filter(|bytes| *bytes <= self.options.max_list_bytes)
                    .ok_or_else(|| {
                        error(
                            "s3_list_limit",
                            "object listing exceeds configured byte bound",
                        )
                    })?;
                objects.push(object);
            }
        }
        Ok(ObjectPage {
            objects,
            next_token,
        })
    }
}

impl AsyncObjectStore for AwsS3ObjectStore {
    async fn put(
        &self,
        key: &str,
        body: Vec<u8>,
        metadata: ObjectMetadata,
    ) -> FrameworkResult<StoredObject> {
        self.bounded(async {
            let (key, wire) = self.wire_key(key)?;
            let mut metadata = metadata;
            metadata
                .content_type
                .get_or_insert_with(|| "application/octet-stream".into());
            if body.len() > self.options.max_object_bytes {
                return Err(size_error());
            }
            if metadata.tags.len() > 10 {
                return Err(error(
                    "s3_metadata_invalid",
                    "S3 accepts at most ten object tags",
                ));
            }
            let tagging = metadata
                .tags
                .iter()
                .map(|(key, value)| format!("{}={}", encode_tag(key), encode_tag(value)))
                .collect::<Vec<_>>()
                .join("&");
            self.client
                .put_object()
                .bucket(&self.config.bucket)
                .key(wire)
                .body(ByteStream::from(body.clone()))
                .set_content_type(metadata.content_type.clone())
                .set_tagging((!tagging.is_empty()).then_some(tagging))
                .send()
                .await
                .map_err(|_| request_error())?;
            Ok(StoredObject {
                key,
                body,
                metadata,
            })
        })
        .await
    }

    async fn get(&self, key: &str) -> FrameworkResult<Option<StoredObject>> {
        self.bounded(self.get_inner(key)).await
    }

    /// Existence is checked before deletion; the returned boolean is advisory
    /// under concurrent writes/deletes. S3 deletion itself is idempotent.
    async fn delete(&self, key: &str) -> FrameworkResult<bool> {
        self.bounded(async {
            let (_, wire) = self.wire_key(key)?;
            match self
                .client
                .head_object()
                .bucket(&self.config.bucket)
                .key(&wire)
                .send()
                .await
            {
                Ok(_) => {}
                Err(failure)
                    if failure.as_service_error().is_some_and(
                        aws_sdk_s3::operation::head_object::HeadObjectError::is_not_found,
                    ) =>
                {
                    return Ok(false);
                }
                Err(_) => return Err(request_error()),
            }
            self.client
                .delete_object()
                .bucket(&self.config.bucket)
                .key(wire)
                .send()
                .await
                .map_err(|_| request_error())?;
            Ok(true)
        })
        .await
    }

    async fn list(&self, prefix: &str) -> FrameworkResult<Vec<StoredObject>> {
        self.bounded(async {
            let mut objects = Vec::new();
            let mut token = None;
            let mut bytes = 0usize;
            loop {
                let remaining = self.options.max_list_objects.saturating_sub(objects.len());
                if remaining == 0 {
                    return Err(error(
                        "s3_list_limit",
                        "object listing exceeds configured count bound",
                    ));
                }
                let page = self
                    .list_page_inner(prefix, token.as_deref(), remaining.min(1000))
                    .await?;
                for object in page.objects {
                    bytes = bytes
                        .checked_add(object.body.len())
                        .filter(|bytes| *bytes <= self.options.max_list_bytes)
                        .ok_or_else(|| {
                            error(
                                "s3_list_limit",
                                "object listing exceeds configured byte bound",
                            )
                        })?;
                    objects.push(object);
                }
                let Some(next) = page.next_token else {
                    return Ok(objects);
                };
                if token.as_ref() == Some(&next) {
                    return Err(error(
                        "s3_list_invalid",
                        "object listing repeated its continuation token",
                    ));
                }
                token = Some(next);
            }
        })
        .await
    }
}

fn error(code: &str, message: &str) -> FrameworkError {
    FrameworkError::new(code, message)
}
fn request_error() -> FrameworkError {
    error("s3_request_failed", "object store request failed")
}
fn size_error() -> FrameworkError {
    error(
        "s3_object_too_large",
        "object exceeds configured byte bound",
    )
}
fn encode_tag(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_s3::config::{Credentials, Region, retry::RetryConfig};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn client(endpoint: &str) -> Client {
        Client::from_conf(
            aws_sdk_s3::Config::builder()
                .region(Region::new("us-east-1"))
                .credentials_provider(Credentials::new(
                    std::env::var("FORKLAUNCH_TEST_S3_ACCESS_KEY")
                        .unwrap_or_else(|_| "parity-test-access".into()),
                    std::env::var("FORKLAUNCH_TEST_S3_SECRET_KEY")
                        .unwrap_or_else(|_| "parity-test-secret-only".into()),
                    None,
                    None,
                    "parity-test",
                ))
                .endpoint_url(endpoint)
                .force_path_style(true)
                .retry_config(RetryConfig::standard().with_max_attempts(1))
                .build(),
        )
    }
    fn test_store(client: Client, prefix: &str, mut options: AwsS3Options) -> AwsS3ObjectStore {
        options.prefix = prefix.into();
        AwsS3ObjectStore::new(
            client,
            S3ObjectStoreConfig::new("forklaunch-parity").with_region("us-east-1"),
            options,
        )
        .unwrap()
    }

    #[test]
    fn aws_config_and_keys_reject_invalid_bounds_without_network_access() {
        let client = client("http://127.0.0.1:1");
        for options in [
            AwsS3Options {
                max_object_bytes: 0,
                ..AwsS3Options::default()
            },
            AwsS3Options {
                max_list_bytes: 0,
                ..AwsS3Options::default()
            },
            AwsS3Options {
                operation_timeout: Duration::ZERO,
                ..AwsS3Options::default()
            },
            AwsS3Options {
                prefix: "../other".into(),
                ..AwsS3Options::default()
            },
            AwsS3Options {
                prefix: "/".into(),
                ..AwsS3Options::default()
            },
        ] {
            assert!(
                AwsS3ObjectStore::new(client.clone(), S3ObjectStoreConfig::new("assets"), options)
                    .is_err()
            );
        }
        let store = test_store(client, "tenant", AwsS3Options::default());
        for key in ["", " ", "/", "///", "../other"] {
            assert!(store.wire_key(key).is_err());
        }
        assert!(store.wire_key(&"a".repeat(1024)).is_err());
        assert_eq!(
            store.wire_key("/a/🦀").unwrap(),
            ("a/🦀".into(), "tenant/a/🦀".into())
        );
        assert_eq!(encode_tag("hello /é+"), "hello%20%2F%C3%A9%2B");
    }

    #[tokio::test]
    async fn aws_operations_stop_at_the_configured_deadline() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (_connection, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let store = test_store(
            client(&endpoint),
            "deadline",
            AwsS3Options {
                operation_timeout: Duration::from_millis(100),
                ..AwsS3Options::default()
            },
        );
        let error = store.get("blocked").await.unwrap_err();
        assert_eq!(error.code(), "s3_timeout");
        server.abort();
    }

    #[tokio::test]
    async fn aws_bucket_creation_does_not_follow_access_denied() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let seen = requests.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut connection, _) = listener.accept().await.unwrap();
                let mut request = [0; 8192];
                let _ = connection.read(&mut request).await.unwrap();
                seen.fetch_add(1, Ordering::SeqCst);
                connection
                    .write_all(
                        b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await
                    .unwrap();
            }
        });
        let store = test_store(client(&endpoint), "denied", AwsS3Options::default());
        assert_eq!(
            store.init_bucket(true).await.unwrap_err().code(),
            "s3_request_failed"
        );
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        server.abort();
    }

    #[tokio::test]
    #[ignore = "requires isolated S3-compatible endpoint in FORKLAUNCH_TEST_S3_ENDPOINT"]
    async fn s3_backend_round_trips_binary_tags_batches_namespaces_and_pages() {
        let endpoint = std::env::var("FORKLAUNCH_TEST_S3_ENDPOINT")
            .expect("FORKLAUNCH_TEST_S3_ENDPOINT must be set");
        let client = client(&endpoint);
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let missing = AwsS3ObjectStore::new(
            client.clone(),
            S3ObjectStoreConfig::new(format!("missing-{id}")),
            AwsS3Options::default(),
        )
        .unwrap();
        assert_eq!(
            missing.init_bucket(false).await.unwrap_err().code(),
            "s3_bucket_missing"
        );
        assert_eq!(
            missing.get("absent").await.unwrap_err().code(),
            "s3_request_failed"
        );
        let prefix = format!("test-{id}/tenant-a");
        let store = test_store(client.clone(), &prefix, AwsS3Options::default());
        store.init_bucket(true).await.unwrap();
        store.init_bucket(false).await.unwrap();
        let foreign = test_store(
            client.clone(),
            &format!("test-{id}/tenant-b"),
            AwsS3Options::default(),
        );
        foreign
            .put("records/foreign", vec![99], ObjectMetadata::new())
            .await
            .unwrap();
        assert_empty_and_codec_roundtrips(&store).await;
        let metadata = ObjectMetadata::new()
            .with_content_type("application/octet-stream")
            .with_tag("service", "accounts")
            .with_tag("label", "hello rust/cloud");
        let objects = vec![
            (
                "records/a".into(),
                vec![0, 255, 13, 10, 42],
                metadata.clone(),
            ),
            ("records/b".into(), vec![1, 2], metadata.clone()),
            ("records/c".into(), vec![3, 4], metadata.clone()),
        ];
        let stored = store.put_batch(objects).await.unwrap();
        assert_eq!(
            store.get("/records/a").await.unwrap().as_ref(),
            Some(&stored[0])
        );
        let read = store
            .get_batch(&["records/c".into(), "absent".into(), "records/a".into()])
            .await
            .unwrap();
        assert_eq!(
            read,
            [Some(stored[2].clone()), None, Some(stored[0].clone())]
        );
        let first = store.list_page("records/", None, 2).await.unwrap();
        assert_eq!(first.objects, stored[..2]);
        let second = store
            .list_page("records/", first.next_token.as_deref(), 2)
            .await
            .unwrap();
        assert_eq!(second.objects, stored[2..]);
        assert!(second.next_token.is_none());
        assert_eq!(store.list("records/").await.unwrap(), stored);
        assert!(store.list("records/../").await.is_err());
        assert!(store.list_page("", None, 0).await.is_err());
        assert_resource_bounds(client, &prefix, &store).await;
        assert_eq!(
            store
                .delete_batch(&["records/a".into(), "absent".into(), "records/b".into()])
                .await
                .unwrap(),
            2
        );
        assert!(!store.delete("records/a").await.unwrap());
        assert_eq!(store.list("records/").await.unwrap(), stored[2..]);
        assert!(store.delete("records/c").await.unwrap());
        assert!(store.list("").await.unwrap().is_empty());
        assert!(foreign.get("records/foreign").await.unwrap().is_some());
        foreign.delete("records/foreign").await.unwrap();
    }
    async fn assert_resource_bounds(client: Client, prefix: &str, store: &AwsS3ObjectStore) {
        let tiny = test_store(
            client.clone(),
            prefix,
            AwsS3Options {
                max_object_bytes: 4,
                ..AwsS3Options::default()
            },
        );
        assert_eq!(
            tiny.get("records/a").await.unwrap_err().code(),
            "s3_object_too_large"
        );
        assert_eq!(
            tiny.put("oversized", vec![0; 5], ObjectMetadata::new())
                .await
                .unwrap_err()
                .code(),
            "s3_object_too_large"
        );
        assert!(store.get("oversized").await.unwrap().is_none());
        let count_bound = test_store(
            client.clone(),
            prefix,
            AwsS3Options {
                max_list_objects: 2,
                ..AwsS3Options::default()
            },
        );
        assert_eq!(
            count_bound.list("records/").await.unwrap_err().code(),
            "s3_list_limit"
        );
        let byte_bound = test_store(
            client,
            prefix,
            AwsS3Options {
                max_list_bytes: 3,
                ..AwsS3Options::default()
            },
        );
        assert_eq!(
            byte_bound.list("records/").await.unwrap_err().code(),
            "s3_list_limit"
        );
    }
    async fn assert_empty_and_codec_roundtrips(store: &AwsS3ObjectStore) {
        let empty = store
            .put("empty", vec![], ObjectMetadata::new())
            .await
            .unwrap();
        assert_eq!(store.get("empty").await.unwrap(), Some(empty));
        store.delete("empty").await.unwrap();
        // The codec hook is transport independent; production callers supply an AEAD codec.
        let codec = crate::XorObjectCodec::new(b"test-only-codec").unwrap();
        crate::put_encrypted_async(
            store,
            &codec,
            "encoded",
            b"plaintext",
            ObjectMetadata::new(),
        )
        .await
        .unwrap();
        assert_ne!(
            store.get("encoded").await.unwrap().unwrap().body,
            b"plaintext"
        );
        assert_eq!(
            crate::get_decrypted_async(store, &codec, "encoded")
                .await
                .unwrap(),
            Some(b"plaintext".to_vec())
        );
        store.delete("encoded").await.unwrap();
    }
}
