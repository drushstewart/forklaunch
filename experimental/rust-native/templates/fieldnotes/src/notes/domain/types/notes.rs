//! Notes records and insight outcomes shared by the application and its storage adapter.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InsightInput {
    pub operation_id: Uuid,
    pub revision: i64,
    pub excerpt: String,
    pub word_count: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InsightApplyResult {
    Applied,
    Duplicate,
    Stale,
}
pub struct InsightRequest {
    pub operation_id: Uuid,
    pub revision: i64,
}
