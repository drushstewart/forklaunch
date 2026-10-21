//! Worker-owned serialized queue payload classification.
#![allow(dead_code)]
use framework_core::persistence::compliance::{ComplianceEntity, ComplianceMetadata};
use uuid::Uuid;
/// Members INSIDE the serialized SealedNoteJob stored in apalis.jobs.job.
/// Only payload is encrypted; context stays public and is authenticated against
/// the inner encrypted body. Apalis status/lease/error columns are outside this
/// descriptor, so this is not a classification of the complete queue table.
#[derive(ComplianceMetadata)]
#[compliance(entity = "SealedNoteJob", crate_path = "framework_core")]
pub struct NoteQueueEnvelope {
    #[compliance(level = "none", name = "operationId")]
    operation_id: Uuid,
    #[compliance(level = "none", name = "workspaceId")]
    workspace_id: Uuid,
    #[compliance(level = "none", name = "noteId")]
    note_id: Uuid,
    #[compliance(level = "none")]
    revision: i64,
    #[compliance(level = "pii")]
    payload: String,
}

pub fn declarations() -> Vec<ComplianceEntity> {
    vec![ComplianceEntity::of::<NoteQueueEnvelope>(
        "apalis.jobs.job",
        "serialized-job-envelope",
    )]
}
