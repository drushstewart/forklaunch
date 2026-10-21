//! Domain wire DTOs, corresponding to TypeScript domain/schemas.
mod attachments;
mod errors;
mod insights;
mod notes;
mod realtime;

pub use attachments::{Attachment, AttachmentUpload};
pub use errors::ErrorBody;
pub use insights::{InsightRequested, InsightResult, InsightStatus, InternalInsightInput};
pub use notes::{CreateNote, Note, PatchNote};
pub use realtime::{NoteEvent, PresenceEvent, PresenceInput, WatchHeaders};
