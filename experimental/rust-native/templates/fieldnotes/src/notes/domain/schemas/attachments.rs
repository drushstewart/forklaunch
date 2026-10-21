#[framework_core::contract::schema(rename_all = "camelCase")]
#[derive(Debug, Clone)]
pub struct Attachment {
    pub id: String,
    pub workspace_id: String,
    pub note_id: String,
    pub filename: String,
    pub content_type: String,
    #[field(min = 0)]
    pub size: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[framework_core::contract::schema(unknown_fields = "deny")]
#[derive(Debug, Clone)]
pub struct AttachmentUpload {
    pub file: framework_core::contract::UploadFile,
}
