use crate::{
    api::{context::NoteContext, errors::ApiError},
    registrations::AppState,
};
use axum::extract::State;
use fieldnotes_contracts::{CreateNote, Note};
use framework::native::EndpointError;

pub(crate) async fn import_note_form(
    State(state): State<AppState>,
    context: NoteContext,
    body: CreateNote,
) -> Result<Note, EndpointError> {
    state
        .notes
        .create(&context.tenant, &body.title, &body.body)
        .await
        .map_err(ApiError::from)
        .map_err(Into::into)
}
pub(crate) async fn export_note_text(
    State(state): State<AppState>,
    context: NoteContext,
    (): (),
) -> Result<String, EndpointError> {
    let record = state
        .notes
        .get(&context.tenant, context.note_id()?)
        .await
        .map_err(ApiError::from)?;
    Ok(format!("{}\n\n{}", record.title, record.body))
}
pub(crate) fn html_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}
pub(crate) async fn export_note_html(
    State(state): State<AppState>,
    context: NoteContext,
    (): (),
) -> Result<String, EndpointError> {
    let record = state
        .notes
        .get(&context.tenant, context.note_id()?)
        .await
        .map_err(ApiError::from)?;
    Ok(format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title></head><body><h1>{}</h1><pre>{}</pre></body></html>",
        html_escape(&record.title),
        html_escape(&record.title),
        html_escape(&record.body)
    ))
}
