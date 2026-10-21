//! Explicit S3 client configuration. Construction never creates a bucket.
use forklaunch_core::{common::{FrameworkError, FrameworkResult}, persistence::json::JsonRecordCodec};
use forklaunch_infrastructure_s3::{
    S3ObjectStoreConfig,
    aws::{AwsS3ObjectStore, aws_sdk_s3},
};
use std::env;

fn required(name: &str) -> FrameworkResult<String> {
    env::var(name)
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
        forklaunch_infrastructure_s3::aws::environment::S3Environment::from_env()?.options,
    )
}

/// Production/default-chain factory, also honoring local overrides, prefix and presign limits.
pub async fn configured_object_store() -> FrameworkResult<AwsS3ObjectStore> {
    forklaunch_infrastructure_s3::aws::environment::S3Environment::from_env()?.into_store().await
}

/// Explicit encrypted JSON factory using the AWS chain and ENCRYPTION_KEY.
pub async fn encrypted_object_store()
-> FrameworkResult<forklaunch_infrastructure_s3::json::JsonObjectStore<AwsS3ObjectStore>> {
    let codec = JsonRecordCodec::from_env()?;
    Ok(forklaunch_infrastructure_s3::json::JsonObjectStore::new(
        configured_object_store().await?,
        codec,
    ))
}
