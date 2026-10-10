//! Bounded native transfers without whole-object buffering.
use std::path::Path;

use super::{
    AwsS3ObjectStore, ObjectMetadata, aws_sdk_s3::primitives::ByteStream, error, request_error,
    size_error, upload_metadata,
};
use forklaunch_core::common::FrameworkResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadedObject {
    pub key: String,
    pub content_length: u64,
    pub metadata: ObjectMetadata,
    pub etag: Option<String>,
    pub version_id: Option<String>,
}

/// An owned pull-based download. Dropping it cancels the remaining body read.
/// The deadline includes caller idle time between chunks.
#[derive(Debug)]
pub struct StreamingDownload {
    pub key: String,
    pub metadata: ObjectMetadata,
    pub content_length: Option<u64>,
    body: Option<ByteStream>,
    deadline: tokio::time::Instant,
    max_bytes: usize,
    bytes_read: usize,
}
impl StreamingDownload {
    /// # Errors
    /// Returns an error on timeout, oversized/truncated data, or a failed body read.
    /// Errors terminate the stream; later calls return `None`.
    pub async fn next_chunk(&mut self) -> FrameworkResult<Option<Vec<u8>>> {
        if self.body.is_some() && tokio::time::Instant::now() >= self.deadline {
            self.body = None;
            return Err(error("s3_timeout", "object transfer exceeded its deadline"));
        }
        let Some(body) = self.body.as_mut() else {
            return Ok(None);
        };
        let chunk = match tokio::time::timeout_at(self.deadline, body.try_next()).await {
            Ok(Ok(chunk)) => chunk,
            Ok(Err(_)) => {
                self.body = None;
                return Err(request_error());
            }
            Err(_) => {
                self.body = None;
                return Err(error("s3_timeout", "object transfer exceeded its deadline"));
            }
        };
        if let Some(chunk) = chunk {
            if chunk.len() > self.max_bytes.saturating_sub(self.bytes_read) {
                self.body = None;
                return Err(size_error());
            }
            self.bytes_read += chunk.len();
            Ok(Some(chunk.to_vec()))
        } else {
            self.body = None;
            if self
                .content_length
                .is_some_and(|length| length != self.bytes_read as u64)
            {
                return Err(error(
                    "s3_object_truncated",
                    "download length does not match its declared length",
                ));
            }
            Ok(None)
        }
    }
}

impl AwsS3ObjectStore {
    /// Starts a native streaming download after reading object metadata and tags.
    /// # Errors
    /// Rejects invalid keys, oversized objects, inaccessible data, and timeouts.
    pub async fn download(&self, key: &str) -> FrameworkResult<Option<StreamingDownload>> {
        self.bounded(self.download_inner(key)).await
    }

    pub(super) async fn download_inner(
        &self,
        key: &str,
    ) -> FrameworkResult<Option<StreamingDownload>> {
        let deadline = tokio::time::Instant::now()
            .checked_add(self.options.operation_timeout)
            .ok_or_else(|| error("s3_config_invalid", "operation deadline is out of range"))?;
        let (key, wire) = self.wire_key(key)?;
        let output = match self
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
        let content_length = output
            .content_length()
            .map(u64::try_from)
            .transpose()
            .map_err(|_| size_error())?;
        if content_length.is_some_and(|length| length > self.options.max_object_bytes as u64) {
            return Err(size_error());
        }
        let content_type = output.content_type().map(str::to_owned);
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
        Ok(Some(StreamingDownload {
            key,
            metadata,
            content_length,
            body: Some(output.body),
            deadline,
            max_bytes: self.options.max_object_bytes,
            bytes_read: 0,
        }))
    }

    /// Uploads an SDK byte stream with an exact known length. File-backed SDK
    /// streams can reopen their source for retries; keep their source unchanged
    /// until the operation completes. Unknown-length streams are rejected.
    /// # Errors
    /// Rejects invalid keys/metadata, unknown or oversized lengths, and failed uploads.
    pub async fn put_stream(
        &self,
        key: &str,
        body: ByteStream,
        metadata: ObjectMetadata,
    ) -> FrameworkResult<UploadedObject> {
        self.bounded(self.put_stream_inner(key, body, metadata))
            .await
    }

    /// Opens a file through the SDK's retryable file stream and uploads it without
    /// collecting the file in memory. The deadline covers file setup and upload.
    /// # Errors
    /// Returns an error when opening the file or uploading its contents fails.
    pub async fn put_file(
        &self,
        key: &str,
        path: impl AsRef<Path>,
        metadata: ObjectMetadata,
    ) -> FrameworkResult<UploadedObject> {
        self.bounded(async {
            // Validate the destination before opening application files.
            self.wire_key(key)?;
            let body = ByteStream::from_path(path)
                .await
                .map_err(|_| error("s3_source_failed", "unable to open upload source"))?;
            self.put_stream_inner(key, body, metadata).await
        })
        .await
    }

    async fn put_stream_inner(
        &self,
        key: &str,
        body: ByteStream,
        metadata: ObjectMetadata,
    ) -> FrameworkResult<UploadedObject> {
        let (key, wire) = self.wire_key(key)?;
        let (length, upper) = body.size_hint();
        if upper != Some(length) {
            return Err(error(
                "s3_source_length_unknown",
                "stream upload requires an exact known length",
            ));
        }
        if length > self.options.max_object_bytes as u64 {
            return Err(size_error());
        }
        let (metadata, tagging) = upload_metadata(metadata)?;
        let output = self
            .client
            .put_object()
            .bucket(&self.config.bucket)
            .key(wire)
            .body(body)
            .content_length(i64::try_from(length).map_err(|_| size_error())?)
            .set_content_type(metadata.content_type.clone())
            .set_tagging(tagging)
            .send()
            .await
            .map_err(|_| request_error())?;
        Ok(UploadedObject {
            key,
            content_length: length,
            metadata,
            etag: output.e_tag,
            version_id: output.version_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        AwsS3Options,
        aws_sdk_s3::{
            Client,
            config::{Credentials, Region},
            primitives::SdkBody,
        },
    };
    use super::*;
    use crate::S3ObjectStoreConfig;
    use std::{
        pin::Pin,
        task::{Context, Poll},
        time::Duration,
    };

    fn download(body: Vec<u8>, limit: usize, length: Option<u64>) -> StreamingDownload {
        StreamingDownload {
            key: "test".into(),
            metadata: ObjectMetadata::new(),
            content_length: length,
            body: Some(ByteStream::from(body)),
            deadline: tokio::time::Instant::now() + Duration::from_secs(1),
            max_bytes: limit,
            bytes_read: 0,
        }
    }
    #[tokio::test]
    async fn chunked_download_enforces_byte_bounds_and_declared_lengths() {
        let mut bounded = download(vec![1, 2, 3], 2, None);
        assert_eq!(
            bounded.next_chunk().await.unwrap_err().code(),
            "s3_object_too_large"
        );
        assert!(bounded.next_chunk().await.unwrap().is_none());
        let mut truncated = download(vec![1, 2], 10, Some(3));
        assert_eq!(truncated.next_chunk().await.unwrap(), Some(vec![1, 2]));
        assert_eq!(
            truncated.next_chunk().await.unwrap_err().code(),
            "s3_object_truncated"
        );
        assert!(truncated.next_chunk().await.unwrap().is_none());
        let mut valid = download(vec![1, 2, 3], 3, Some(3));
        assert_eq!(valid.next_chunk().await.unwrap(), Some(vec![1, 2, 3]));
        assert!(valid.next_chunk().await.unwrap().is_none());
    }
    #[tokio::test]
    async fn chunked_download_deadline_includes_ready_buffered_bytes_and_idle_time() {
        let mut stream = download(vec![1], 10, Some(1));
        stream.deadline = tokio::time::Instant::now();
        assert_eq!(stream.next_chunk().await.unwrap_err().code(), "s3_timeout");
        assert!(stream.next_chunk().await.unwrap().is_none());
    }
    struct UnknownBody;
    impl http_body::Body for UnknownBody {
        type Data = bytes::Bytes;
        type Error = std::io::Error;
        fn poll_frame(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
            Poll::Ready(None)
        }
    }
    #[tokio::test]
    async fn stream_upload_rejects_unknown_lengths_and_bad_files_before_storage_access() {
        let client = Client::from_conf(
            aws_sdk_s3::Config::builder()
                .region(Region::new("us-east-1"))
                .credentials_provider(Credentials::new("test", "test", None, None, "test"))
                .endpoint_url("http://127.0.0.1:1")
                .force_path_style(true)
                .build(),
        );
        let store = AwsS3ObjectStore::new(
            client,
            S3ObjectStoreConfig::new("test"),
            AwsS3Options::default(),
        )
        .unwrap();
        let unknown = ByteStream::new(SdkBody::from_body_1_x(UnknownBody));
        assert_eq!(
            store
                .put_stream("key", unknown, ObjectMetadata::new())
                .await
                .unwrap_err()
                .code(),
            "s3_source_length_unknown"
        );
        let root = tempfile::tempdir().unwrap();
        let absent = root.path().join("absent");
        assert_eq!(
            store
                .put_file("key", &absent, ObjectMetadata::new())
                .await
                .unwrap_err()
                .code(),
            "s3_source_failed"
        );
        assert_eq!(
            store
                .put_file("../key", &absent, ObjectMetadata::new())
                .await
                .unwrap_err()
                .code(),
            "s3_object_invalid_key"
        );
    }
}
