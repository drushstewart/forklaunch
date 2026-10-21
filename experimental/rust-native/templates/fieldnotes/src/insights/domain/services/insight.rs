use crate::{Insight, NoteJob, PURPOSE, SealedNoteJob, WorkerError};
use fieldnotes_security::EnvelopeKey;
use zeroize::Zeroizing;
pub fn analyze(key: &EnvelopeKey, sealed: &SealedNoteJob) -> Result<Insight, WorkerError> {
    let job = ::worker::decode_job::<fieldnotes_contracts::events::insight::TracedNoteJob>(
        key.as_keyring(),
        sealed.context(),
        PURPOSE,
        &sealed.payload,
    )
    .map_err(|_| WorkerError::InvalidEnvelope)?;
    summarize(&job)
}
pub(crate) fn summarize(job: &NoteJob) -> Result<Insight, WorkerError> {
    let body = &job.body;
    if body.chars().count() > 65_536 {
        return Err(WorkerError::InvalidEnvelope);
    }
    Ok(Insight {
        operation_id: job.operation_id,
        workspace_id: job.workspace_id,
        note_id: job.note_id,
        revision: job.revision,
        excerpt: Zeroizing::new(body.chars().take(160).collect()),
        word_count: body.split_whitespace().count() as u64,
    })
}

#[cfg(test)]
#[path = "../../__test__/insight.rs"]
mod tests;
