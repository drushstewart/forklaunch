use chrono::{DateTime, Utc};
use uuid::Uuid;
pub struct AttachmentRecord {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub note_id: Uuid,
    pub filename: String,
    pub content_type: String,
    pub size: i64,
    pub created_at: DateTime<Utc>,
}
