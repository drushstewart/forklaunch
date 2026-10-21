use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
/// Decrypted application output. Deliberately not Debug: notes can be sensitive.
/// PII is this demo's explicit policy, not content detection. Deployments handling
/// health or payment data must select their corresponding classification/policies.
#[derive(Serialize, Deserialize)]
pub struct NoteRecord {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub body: String,
    pub revision: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub insight_excerpt: Option<String>,
    pub insight_word_count: Option<i64>,
}
pub struct NoteEventRecord {
    pub id: i64,
    pub workspace_id: Uuid,
    pub note_id: Uuid,
    pub revision: i64,
    pub kind: String,
    pub created_at: DateTime<Utc>,
}
