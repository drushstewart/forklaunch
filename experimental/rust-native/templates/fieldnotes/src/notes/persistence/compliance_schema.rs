//! Storage classification declarations shared by build tooling and runtime adapters.
//! NoteStorage is the hydrated ciphertext-only database row. Other declarations
//! describe composite storage slots; their adapters bind dynamic contexts explicitly.
//! Opaque identity/reference columns have an explicit none policy in this demo;
//! they still require tenant/access controls. Arbitrary content is classified PII.
#![allow(dead_code)]
use chrono::{DateTime, Utc};
use framework_core::persistence::compliance::{ComplianceEntity, ComplianceMetadata};
use framework_core::persistence::protected::ProtectedField;
use uuid::Uuid;
#[derive(ComplianceMetadata)]
#[compliance(entity = "Note", crate_path = "framework_core", persisted)]
pub struct NoteStorage {
    #[compliance(level = "none")]
    pub(super) id: Uuid,
    #[compliance(level = "none")]
    pub(super) workspace_id: Uuid,
    #[compliance(level = "pii", purpose = "note-title")]
    pub(super) title: ProtectedField,
    #[compliance(level = "pii", purpose = "note-body")]
    pub(super) body: ProtectedField,
    #[compliance(level = "none")]
    pub(super) revision: i64,
    #[compliance(level = "none")]
    pub(super) created_by: String,
    #[compliance(level = "none")]
    pub(super) created_at: DateTime<Utc>,
    #[compliance(level = "none")]
    pub(super) updated_at: DateTime<Utc>,
    #[compliance(level = "pii", nullable, purpose = "note-insight")]
    pub(super) insight_excerpt: Option<ProtectedField>,
    #[compliance(level = "none", nullable)]
    pub(super) insight_word_count: Option<i64>,
    #[compliance(level = "none")]
    pub(super) cache_version: i64,
}
#[derive(ComplianceMetadata)]
#[compliance(entity = "Attachment", crate_path = "framework_core")]
pub struct AttachmentStorage {
    #[compliance(level = "none")]
    id: Uuid,
    #[compliance(level = "none")]
    workspace_id: Uuid,
    #[compliance(level = "none")]
    note_id: Uuid,
    #[compliance(level = "pii")]
    metadata: String,
    #[compliance(level = "none")]
    size: i64,
    #[compliance(level = "none")]
    state: String,
    #[compliance(level = "none")]
    created_at: DateTime<Utc>,
    #[compliance(level = "none")]
    expires_at: DateTime<Utc>,
}
#[derive(ComplianceMetadata)]
#[compliance(entity = "AttachmentObject", crate_path = "framework_core")]
pub struct AttachmentObject {
    #[compliance(level = "pii")]
    body: Vec<u8>,
}
#[derive(ComplianceMetadata)]
#[compliance(entity = "NoteCache", crate_path = "framework_core")]
pub struct NoteCacheStorage {
    #[compliance(level = "pii")]
    snapshot: Vec<u8>,
}
/// SQL outbox retains a sealed payload plus public routing/idempotency context.
#[derive(ComplianceMetadata)]
#[compliance(entity = "NoteOutbox", crate_path = "framework_core")]
pub struct NoteOutboxStorage {
    #[compliance(level = "none")]
    id: Uuid,
    #[compliance(level = "none")]
    workspace_id: Uuid,
    #[compliance(level = "none")]
    note_id: Uuid,
    #[compliance(level = "none")]
    revision: i64,
    #[compliance(level = "none")]
    kind: String,
    #[compliance(level = "pii")]
    payload: String,
    #[compliance(level = "none")]
    created_at: DateTime<Utc>,
    #[compliance(level = "none", nullable)]
    processed_at: Option<DateTime<Utc>>,
    #[compliance(level = "none", nullable)]
    applied_at: Option<DateTime<Utc>>,
}
pub fn declarations() -> Vec<ComplianceEntity> {
    vec![
        ComplianceEntity::of::<NoteOutboxStorage>("fieldnotes_outbox", "postgres"),
        ComplianceEntity::of::<NoteStorage>("fieldnotes_notes", "postgres"),
        ComplianceEntity::of::<AttachmentStorage>("fieldnotes_attachments", "postgres"),
        ComplianceEntity::of::<AttachmentObject>(
            "attachments/{tenant}/{note}/{id}",
            "object-store",
        ),
        ComplianceEntity::of::<NoteCacheStorage>(
            "fieldnotes:note:{tenant}:{id}:{version}",
            "redis",
        ),
    ]
}
