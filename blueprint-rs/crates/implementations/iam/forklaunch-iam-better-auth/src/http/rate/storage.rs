//! Application-owned atomic counter storage for auth rate limits.
use super::AuthRateLimitRule;
use forklaunch_blueprint_core::BlueprintResult;
use std::{future::Future, pin::Pin, sync::Arc};

/// Result of an atomic backend check-and-increment operation.
#[derive(Clone, Copy, Debug)]
pub enum AuthRateLimitStorageDecision {
    Allow,
    /// Retry delay in seconds; `None` falls back to the effective rule's window.
    Deny {
        retry_after: Option<u64>,
    },
}
type StorageFuture =
    Pin<Box<dyn Future<Output = BlueprintResult<AuthRateLimitStorageDecision>> + Send>>;
#[derive(Clone)]
pub(super) struct CustomStorage(
    Arc<dyn Fn(String, AuthRateLimitRule) -> StorageFuture + Send + Sync>,
);
impl std::fmt::Debug for CustomStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CustomRateLimitStorage")
    }
}
impl CustomStorage {
    pub(super) fn new<F, Fut>(consume: F) -> Self
    where
        F: Fn(String, AuthRateLimitRule) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = BlueprintResult<AuthRateLimitStorageDecision>> + Send + 'static,
    {
        Self(Arc::new(move |key, rule| Box::pin(consume(key, rule))))
    }
    pub(super) async fn consume(
        &self,
        key: String,
        rule: AuthRateLimitRule,
    ) -> BlueprintResult<Option<u64>> {
        Ok(match (self.0)(key, rule).await? {
            AuthRateLimitStorageDecision::Allow => None,
            AuthRateLimitStorageDecision::Deny { retry_after } => {
                Some(retry_after.unwrap_or(u64::from(rule.window_seconds())))
            }
        })
    }
}
