pub use fieldnotes_contracts::events::insight::{NoteJob, PURPOSE, QUEUE, SealedNoteJob};
use uuid::Uuid;
use zeroize::Zeroizing;

pub struct Insight {
    pub operation_id: Uuid,
    pub workspace_id: Uuid,
    pub note_id: Uuid,
    pub revision: i64,
    pub excerpt: Zeroizing<String>,
    pub word_count: u64,
}

