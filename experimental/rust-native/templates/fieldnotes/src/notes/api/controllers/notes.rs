use crate::api::context::{InternalContext, NoteContext};
use crate::{
    api::errors::{ApiError, id},
    registrations::AppState,
};
use axum::extract::State;
use fieldnotes_contracts::{
    CreateNote, InsightRequested, InsightResult, InsightStatus, InternalInsightInput, Note,
    PatchNote,
};
use framework::native::EndpointError;

pub(crate) async fn list_notes(
    State(state): State<AppState>,
    context: NoteContext,
    (): (),
) -> Result<Vec<Note>, EndpointError> {
    state
        .notes
        .list(&context.tenant)
        .await
        .map_err(ApiError::from)
        .map_err(Into::into)
}
pub(crate) async fn get_note(
    State(state): State<AppState>,
    context: NoteContext,
    (): (),
) -> Result<Note, EndpointError> {
    state
        .notes
        .get(&context.tenant, context.note_id()?)
        .await
        .map_err(ApiError::from)
        .map_err(Into::into)
}
pub(crate) async fn create_note(
    State(state): State<AppState>,
    context: NoteContext,
    input: CreateNote,
) -> Result<Note, EndpointError> {
    state
        .notes
        .create(&context.tenant, &input.title, &input.body)
        .await
        .map_err(ApiError::from)
        .map_err(Into::into)
}
pub(crate) async fn update_note(
    State(state): State<AppState>,
    context: NoteContext,
    input: PatchNote,
) -> Result<Note, EndpointError> {
    state
        .notes
        .update(
            &context.tenant,
            context.note_id()?,
            input.expected_revision,
            input.title.as_option().map(String::as_str),
            input.body.as_option().map(String::as_str),
        )
        .await
        .map_err(ApiError::from)
        .map_err(Into::into)
}
pub(crate) async fn archive_note(
    State(state): State<AppState>,
    context: NoteContext,
    (): (),
) -> Result<(), EndpointError> {
    state
        .notes
        .delete(&context.tenant, context.note_id()?)
        .await
        .map_err(ApiError::from)
        .map_err(Into::into)
}
pub(crate) async fn request_insights(
    State(state): State<AppState>,
    context: NoteContext,
    (): (),
) -> Result<InsightRequested, EndpointError> {
    let result = state
        .notes
        .request_insights(&context.tenant, context.note_id()?)
        .await
        .map_err(ApiError::from)?;
    Ok(InsightRequested {
        operation_id: result.operation_id.to_string(),
        revision: result.revision,
    })
}
pub(crate) async fn apply_insight(
    State(state): State<AppState>,
    context: InternalContext,
    input: InternalInsightInput,
) -> Result<InsightResult, EndpointError> {
    let operation_id = id(&input.operation_id)?;
    let result = state
        .notes
        .apply_encrypted_insight(context.workspace_id, context.note_id, &input)
        .await
        .map_err(ApiError::from)?;
    let status = match result {
        crate::InsightApplyResult::Applied => InsightStatus::Applied,
        crate::InsightApplyResult::Duplicate => InsightStatus::Duplicate,
        crate::InsightApplyResult::Stale => InsightStatus::Stale,
    };
    Ok(InsightResult {
        operation_id: operation_id.to_string(),
        revision: input.revision,
        status,
    })
}
