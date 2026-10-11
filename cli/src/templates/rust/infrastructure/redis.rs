//! Redis is synchronous; call `connect` and cache operations on a blocking executor.
use std::time::Duration;

use forklaunch_core::common::{FrameworkError, FrameworkResult};
use forklaunch_infrastructure_redis::{RedisTtlCacheConfig, client::RedisTtlCache};

pub fn config() -> FrameworkResult<RedisTtlCacheConfig> {
    let url = std::env::var("REDIS_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| FrameworkError::new("redis_config_invalid", "REDIS_URL is required"))?;
    let config = RedisTtlCacheConfig::new(url, Duration::from_secs(60 * 60));
    config.validate()?;
    Ok(config)
}

/// Connect explicitly when the application needs Redis; service startup stays independent.
pub fn connect() -> FrameworkResult<RedisTtlCache> {
    RedisTtlCache::connect(config()?)
}

/// Explicit encrypted JSON factory; ENCRYPTION_KEY is only required when this is called.
pub fn encrypted_cache()
-> FrameworkResult<forklaunch_infrastructure_redis::json::JsonCache<RedisTtlCache>> {
    let codec = forklaunch_core::persistence::json::JsonRecordCodec::from_env()?;
    Ok(forklaunch_infrastructure_redis::json::JsonCache::new(
        connect()?,
        codec,
    ))
}

/// Explicit encrypted FIFO queue factory sharing the configured Redis URL/key ring.
pub fn encrypted_queue(
    name: &str,
) -> FrameworkResult<forklaunch_infrastructure_redis::json::JsonQueue> {
    let codec = forklaunch_core::persistence::json::JsonRecordCodec::from_env()?;
    let queue = forklaunch_infrastructure_redis::client::RedisQueue::connect(&config()?.url, name)?;
    Ok(forklaunch_infrastructure_redis::json::JsonQueue::new(
        queue, codec,
    ))
}
