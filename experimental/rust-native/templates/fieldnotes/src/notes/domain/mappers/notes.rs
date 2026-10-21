use fieldnotes_contracts::Note;
pub(crate) fn note(record: crate::NoteRecord) -> Note {
    Note {
        id: record.id.to_string(),
        workspace_id: record.workspace_id.to_string(),
        title: record.title,
        body: record.body,
        revision: record.revision,
        created_at: record.created_at,
        updated_at: record.updated_at,
        insight_excerpt: record.insight_excerpt,
        insight_word_count: record.insight_word_count,
    }
}
