//! Explicit S3 client configuration. Construction never creates a bucket.
use forklaunch_core::common::{FrameworkError, FrameworkResult};
use forklaunch_infrastructure_s3::{
    S3ObjectStoreConfig,
    aws::{AwsS3ObjectStore, AwsS3Options, aws_sdk_s3},
};

fn required(name: &str) -> FrameworkResult<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| FrameworkError::new("s3_config_invalid", format!("{name} is required")))
}

/// Customize the injected SDK client here for production credentials, TLS or retry policy.
pub fn object_store() -> FrameworkResult<AwsS3ObjectStore> {
    use aws_sdk_s3::config::{Credentials, Region};
    let region = required("S3_REGION")?;
    let bucket = required("S3_BUCKET")?;
    let client = aws_sdk_s3::Client::from_conf(
        aws_sdk_s3::Config::builder()
            .region(Region::new(region.clone()))
            .credentials_provider(Credentials::new(
                required("S3_ACCESS_KEY_ID")?,
                required("S3_SECRET_ACCESS_KEY")?,
                None,
                None,
                "forklaunch-environment",
            ))
            .endpoint_url(required("S3_URL")?)
            .force_path_style(true)
            .build(),
    );
    AwsS3ObjectStore::new(
        client,
        S3ObjectStoreConfig::new(bucket).with_region(region),
        AwsS3Options::default(),
    )
}
