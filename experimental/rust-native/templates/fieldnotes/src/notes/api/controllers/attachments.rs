use crate::{
    api::{context::NoteContext, errors::ApiError},
    registrations::AppState,
};
use axum::extract::State;
use fieldnotes_contracts::Attachment;
use framework::native::EndpointError;
pub(crate) async fn list_attachments(
    State(state): State<AppState>,
    context: NoteContext,
    (): (),
) -> Result<Vec<Attachment>, EndpointError> {
    state
        .notes
        .list_attachments(&context.tenant, context.note_id()?)
        .await
        .map_err(ApiError::from)
        .map_err(Into::into)
}
pub(crate) async fn upload_attachment(
    State(state): State<AppState>,
    context: NoteContext,
    input: framework::native::MultipartData<fieldnotes_contracts::AttachmentUpload>,
) -> Result<Attachment, EndpointError> {
    let file = input
        .files
        .get("file")
        .ok_or(EndpointError::InvalidRequest)?;
    state
        .notes
        .upload_attachment(
            &context.tenant,
            context.note_id()?,
            &file.filename,
            &file.content_type,
            &input.fields.file.0,
        )
        .await
        .map_err(ApiError::from)
        .map_err(Into::into)
}
pub(crate) async fn download_attachment(
    State(state): State<AppState>,
    context: NoteContext,
    (): (),
) -> Result<Vec<u8>, EndpointError> {
    let result = state
        .notes
        .download_attachment(
            &context.tenant,
            context.note_id()?,
            context.attachment_id.ok_or(EndpointError::InvalidRequest)?,
        )
        .await
        .map_err(ApiError::from)?;
    Ok(result.body.to_vec())
}
