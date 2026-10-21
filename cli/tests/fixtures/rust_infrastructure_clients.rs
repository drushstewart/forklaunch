use std::time::{Duration, SystemTime, UNIX_EPOCH};

use forklaunch_core::persistence::TenantContext;
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
    let configured = std::env::args().any(|arg| arg == "--configured");
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
        if configured {
            let tenant = TenantContext::new("tenant-fixture").map_err(|error| error.to_string())?;
            let wrong = TenantContext::new("tenant-other").map_err(|error| error.to_string())?;
            let mut encrypted =
                redis_infrastructure::encrypted_cache().map_err(|error| error.to_string())?;
            let value = vec!["private@example.test".to_owned(), "秘密🙂".to_owned()];
            encrypted
                .put_record(&cache_key, &value, None, Some(&tenant))
                .map_err(|error| error.to_string())?;
            assert_eq!(
                encrypted
                    .read_record::<Vec<String>>(&cache_key, Some(&tenant))
                    .map_err(|error| error.to_string())?,
                Some(value)
            );
            assert!(
                encrypted
                    .read_record::<Vec<String>>(&cache_key, Some(&wrong))
                    .is_err()
            );
            assert!(
                encrypted
                    .read_record::<Vec<String>>(&cache_key, None)
                    .is_err()
            );
            let bytes = cache
                .get(&cache_key)
                .map_err(|error| error.to_string())?
                .unwrap();
            assert!(bytes.starts_with(b"v4:"));
            assert!(
                !String::from_utf8(bytes)
                    .unwrap()
                    .contains("private@example.test")
            );
            let missing_key = format!("{cache_key}-missing");
            assert_eq!(
                encrypted
                    .peek_batch_records(&[cache_key.clone(), missing_key, cache_key.clone()])
                    .map_err(|error| error.to_string())?,
                vec![true, false, true]
            );
            assert_eq!(
                encrypted
                    .list_keys_matching(&format!("{cache_key}*"))
                    .map_err(|error| error.to_string())?,
                vec![cache_key.clone()]
            );
            assert_eq!(
                encrypted.backend().config().default_ttl(),
                Duration::from_secs(3600)
            );
            encrypted
                .delete_record(&cache_key)
                .map_err(|error| error.to_string())?;
            let codec = forklaunch_core::persistence::json::JsonRecordCodec::from_env()
                .map_err(|error| error.to_string())?
                .with_typescript_safe_values();
            let mut compatible = forklaunch_infrastructure_redis::json::JsonCache::new(
                redis_infrastructure::connect().map_err(|error| error.to_string())?,
                codec,
            );
            compatible
                .put_record(&cache_key, "false", None, Some(&tenant))
                .map_err(|error| error.to_string())?;
            assert_eq!(
                compatible
                    .read_record::<bool>(&cache_key, Some(&tenant))
                    .map_err(|error| error.to_string())?,
                Some(false)
            );
            assert!(
                compatible
                    .read_record::<String>(&cache_key, Some(&tenant))
                    .is_err()
            );
            assert!(
                compatible
                    .read_record::<bool>(&cache_key, Some(&wrong))
                    .is_err()
            );
            assert_eq!(
                compatible
                    .delete_batch_records(&[cache_key.clone(), cache_key.clone()])
                    .map_err(|error| error.to_string())?,
                1
            );
            let mut queue = redis_infrastructure::encrypted_queue(&format!("{cache_key}-queue"))
                .map_err(|error| error.to_string())?;
            queue
                .enqueue_batch_records(&[false, true], Some(&tenant))
                .map_err(|error| error.to_string())?;
            assert!(queue.peek_record::<bool>(Some(&wrong)).is_err());
            assert_eq!(
                queue
                    .peek_records::<bool>(10, Some(&tenant))
                    .map_err(|error| error.to_string())?,
                vec![false, true]
            );
            assert_eq!(
                queue
                    .dequeue_batch_records::<bool>(10, Some(&tenant))
                    .map_err(|error| error.to_string())?,
                vec![false, true]
            );
            assert!(
                queue
                    .dequeue_record::<bool>(Some(&tenant))
                    .map_err(|error| error.to_string())?
                    .is_none()
            );
            queue
                .enqueue_batch_records(&[false, true], Some(&tenant))
                .map_err(|error| error.to_string())?;
            assert_eq!(
                queue
                    .peek_records_typescript::<bool>(0, Some(&tenant))
                    .map_err(|error| error.to_string())?,
                vec![true]
            );
            assert!(
                queue
                    .peek_records_typescript::<bool>(0, Some(&wrong))
                    .is_err()
            );
            assert!(
                queue
                    .dequeue_batch_records_typescript::<bool>(1, Some(&tenant))
                    .map_err(|error| error.to_string())?
                    .is_empty()
            );
            assert_eq!(
                queue
                    .dequeue_batch_records_typescript::<bool>(2, Some(&tenant))
                    .map_err(|error| error.to_string())?,
                vec![true]
            );
        }
        Ok(())
    })
    .await?
    .map_err(std::io::Error::other)?;
    let store = if std::env::args().any(|arg| arg == "--configured") {
        s3_infrastructure::configured_object_store().await?
    } else {
        s3_infrastructure::object_store()?
    };
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
    if configured {
        let encrypted = s3_infrastructure::encrypted_object_store().await?;
        let tenant = TenantContext::new("tenant-fixture")?;
        let wrong = TenantContext::new("tenant-other")?;
        let value = vec!["private@example.test".to_owned(), "秘密🙂".to_owned()];
        encrypted
            .put_object(&key, &value, metadata.clone(), Some(&tenant))
            .await?;
        assert_eq!(
            encrypted
                .get_object::<Vec<String>>(&key, Some(&tenant))
                .await?,
            Some((value, metadata.clone()))
        );
        assert!(
            encrypted
                .get_object::<Vec<String>>(&key, Some(&wrong))
                .await
                .is_err()
        );
        assert!(
            encrypted
                .get_object::<Vec<String>>(&key, None)
                .await
                .is_err()
        );
        let bytes = store.get(&key).await?.unwrap().body;
        assert!(bytes.starts_with(b"v4:"));
        assert!(
            !String::from_utf8(bytes)
                .unwrap()
                .contains("private@example.test")
        );
        assert!(store.delete(&key).await?);
        let first = format!("{key}-batch-a");
        let second = format!("{key}-batch-b");
        let missing = format!("{key}-absent");
        encrypted
            .put_batch_objects(
                vec![
                    (
                        first.clone(),
                        vec!["first".to_owned()],
                        ObjectMetadata::new(),
                    ),
                    (second.clone(), vec!["second".to_owned()], metadata.clone()),
                ],
                Some(&tenant),
            )
            .await?;
        assert_eq!(
            encrypted
                .get_batch_objects::<Vec<String>>(
                    &[second.clone(), missing.clone(), first.clone()],
                    Some(&tenant)
                )
                .await?,
            vec![
                Some((vec!["second".to_owned()], metadata)),
                None,
                Some((
                    vec!["first".to_owned()],
                    ObjectMetadata::new().with_content_type("application/json")
                )),
            ]
        );
        assert!(
            encrypted
                .get_batch_objects::<Vec<String>>(&[first.clone(), second.clone()], Some(&wrong))
                .await
                .is_err()
        );
        assert_eq!(
            encrypted
                .delete_batch_objects(&[first, missing, second])
                .await?,
            2
        );
        println!(
            "PASS generated encrypted Redis cache/queue and S3 single/batch JSON factories: actual backends, at-rest envelopes, metadata, expiry, misses and tenant rejection"
        );
    }
    println!(
        "PASS generated native Redis and S3 factories round-trip binary data through actual backends"
    );
    Ok(())
}
