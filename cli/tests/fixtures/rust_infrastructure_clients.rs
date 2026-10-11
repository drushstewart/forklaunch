use std::time::{Duration, SystemTime, UNIX_EPOCH};

use forklaunch_infrastructure_redis::TtlCache;
use forklaunch_infrastructure_s3::{AsyncObjectStore, ObjectMetadata};
use storage::{redis_infrastructure, s3_infrastructure};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--unconfigured") {
        assert_eq!(
            redis_infrastructure::config().unwrap_err().code(),
            "redis_config_invalid"
        );
        assert_eq!(
            s3_infrastructure::object_store().unwrap_err().code(),
            "s3_config_invalid"
        );
        println!("PASS native infrastructure factories reject missing settings without connecting");
        return Ok(());
    }
    let id = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    assert_eq!(
        redis_infrastructure::config()?.default_ttl(),
        Duration::from_secs(60 * 60)
    );
    let key = format!("native-infra-{id}");
    let cache_key = key.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut cache = redis_infrastructure::connect().map_err(|error| error.to_string())?;
        cache
            .set(
                &cache_key,
                vec![0, 255, 13, 10],
                Some(Duration::from_secs(60)),
            )
            .map_err(|error| error.to_string())?;
        assert_eq!(
            cache.get(&cache_key).map_err(|error| error.to_string())?,
            Some(vec![0, 255, 13, 10])
        );
        assert!(
            cache
                .delete(&cache_key)
                .map_err(|error| error.to_string())?
        );
        assert!(
            cache
                .get(&cache_key)
                .map_err(|error| error.to_string())?
                .is_none()
        );
        Ok(())
    })
    .await?
    .map_err(std::io::Error::other)?;
    let store = s3_infrastructure::object_store()?;
    // Bucket provisioning is explicit application/fixture work, never a factory side effect.
    store.init_bucket(true).await?;
    let metadata = ObjectMetadata::new()
        .with_content_type("application/octet-stream")
        .with_tag("fixture", "native-infrastructure");
    store
        .put(&key, vec![0, 255, 13, 10], metadata.clone())
        .await?;
    let object = store.get(&key).await?.expect("Stored object");
    assert_eq!(object.body, vec![0, 255, 13, 10]);
    assert_eq!(object.metadata, metadata);
    assert_eq!(store.list(&key).await?.len(), 1);
    assert!(store.delete(&key).await?);
    assert!(store.get(&key).await?.is_none());
    println!(
        "PASS generated native Redis and S3 factories round-trip binary data through actual backends"
    );
    Ok(())
}
