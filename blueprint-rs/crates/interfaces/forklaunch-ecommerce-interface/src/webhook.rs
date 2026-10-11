//! Provider webhook bookkeeping, independent of provider authentication and business processing.
use forklaunch_blueprint_core::BlueprintResult;
use serde::{Deserialize, Serialize};
use std::future::Future;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookEvent {
    pub provider: String,
    pub provider_event_id: String,
    pub event_type: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WebhookEventOutcome {
    New,
    AlreadyProcessed,
    Retry,
}
/// `begin_processing` durably records receipt; only successful business processing
/// may call `mark_processed`. Controllers separately serialize concurrent delivery.
pub trait WebhookEventStore: Send + Sync {
    fn begin_processing(
        &self,
        event: WebhookEvent,
    ) -> impl Future<Output = BlueprintResult<WebhookEventOutcome>> + Send;
    fn mark_processed(
        &self,
        provider: &str,
        event_id: &str,
    ) -> impl Future<Output = BlueprintResult<()>> + Send;
}
