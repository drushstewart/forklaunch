use fieldnotes_contracts::Attachment;
pub(crate) fn attachment(record: crate::AttachmentRecord) -> Attachment {
    Attachment {
        id: record.id.to_string(),
        workspace_id: record.workspace_id.to_string(),
        note_id: record.note_id.to_string(),
        filename: record.filename,
        content_type: record.content_type,
        size: record.size,
        created_at: record.created_at,
    }
}
