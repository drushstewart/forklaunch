use crate::registrations::State;
use crate::{SealedNoteJob, WorkerError};
use apalis::prelude::*;
use std::time::Duration;

async fn process(job: SealedNoteJob, state: Data<State>) -> Result<(), WorkerError> {
    let state = &*state;
    worker::process_traced_audited::<crate::NoteJob, _, _>(
        state.key.as_keyring(),
        job.workspace_id,
        worker::JobAccess {
            context: job.context(),
            purpose: crate::PURPOSE,
            actor: "service:insights",
        },
        &job.payload,
        Duration::from_secs(20),
        state.audit.as_ref(),
        |_, payload| async move {
            let result = super::insight::summarize(&payload).map_err(processor_error)?;
            publish(result, state).await.map_err(processor_error)
        },
    )
    .await
    .map_err(|error| match error {
        worker::ProcessorError::InvalidEnvelope => WorkerError::InvalidEnvelope,
        worker::ProcessorError::Configuration => WorkerError::Configuration,
        _ => WorkerError::Publication,
    })
}

fn processor_error(error: WorkerError) -> worker::ProcessorError {
    match error {
        WorkerError::InvalidEnvelope => worker::ProcessorError::InvalidEnvelope,
        WorkerError::Configuration => worker::ProcessorError::Configuration,
        WorkerError::Publication | WorkerError::Storage => worker::ProcessorError::Failed,
    }
}

async fn publish(result: crate::Insight, state: &State) -> Result<(), WorkerError> {
    let payload = zeroize::Zeroizing::new(fieldnotes_contracts::events::insight::InsightEvent {
        operation_id: result.operation_id,
        workspace_id: result.workspace_id,
        note_id: result.note_id,
        revision: result.revision,
        excerpt: result.excerpt.to_string(),
        word_count: result.word_count,
    });
    let channel =
        worker::EncryptedChannel::<fieldnotes_contracts::events::insight::InsightEvent>::new(
            fieldnotes_contracts::events::insight::RESULT_PURPOSE,
        )
        .map_err(|_| WorkerError::Configuration)?;
    let encrypted_payload = channel
        .seal(state.key.as_keyring(), result.workspace_id, &payload)
        .map_err(|_| WorkerError::InvalidEnvelope)?
        .ciphertext()
        .to_owned();
    let body = fieldnotes_client::ApplyInsightRequestBody {
        operation_id: result.operation_id.to_string(),
        revision: result.revision,
        encrypted_payload,
    };
    // Sign the same typed serialization used by the generated SDK.
    let encoded = serde_json::to_string(&body).map_err(|_| WorkerError::Publication)?;
    let path = format!(
        "/internal/workspaces/{}/notes/{}/insight",
        result.workspace_id, result.note_id
    );
    let authorization = fieldnotes_security::generate_hmac_auth_header(
        state.secret.as_bytes(),
        "worker",
        "POST",
        &path,
        fieldnotes_security::SignedBody::Text(&encoded),
        &chrono::Utc::now().to_rfc3339(),
        &uuid::Uuid::new_v4().to_string(),
    )
    .map_err(|_| WorkerError::Publication)?;
    let request = fieldnotes_client::ApplyInsightRequest {
        params: fieldnotes_client::ApplyInsightParams {
            workspace_id: result.workspace_id.to_string(),
            note_id: result.note_id.to_string(),
        },
        headers: [("authorization".into(), authorization)].into(),
        body: Some(body),
        ..Default::default()
    };
    let response = tokio::time::timeout(Duration::from_secs(15), state.api.apply_insight(request))
        .await
        .map_err(|_| WorkerError::Publication)?
        .map_err(|_| WorkerError::Publication)?;
    match response {
        fieldnotes_client::ApplyInsightResponses::Status200(body)
            if body.operation_id == result.operation_id.to_string()
                && body.revision == result.revision =>
        {
            Ok(())
        }
        _ => Err(WorkerError::Publication),
    }
}
pub(crate) async fn process_job(job: SealedNoteJob, state: Data<State>) -> Result<(), BoxDynError> {
    process(job, state)
        .await
        .map_err(|error| worker::apalis::queue_error(processor_error(error)))
}

#[cfg(test)]
#[path = "../../__test__/publication.rs"]
mod tests;
