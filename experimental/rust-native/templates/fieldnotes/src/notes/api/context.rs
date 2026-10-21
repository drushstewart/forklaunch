mod internal;
pub(crate) use internal::{InternalContext, InternalPolicy};

use super::errors::{ApiError, id};
use crate::registrations::AppState;
use axum::http::HeaderMap;
use fieldnotes_security::VerifiedTenant;
pub(super) fn cookie(headers: &HeaderMap) -> &str {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}
pub(super) async fn scope(
    state: &AppState,
    headers: &HeaderMap,
    workspace: &str,
) -> Result<VerifiedTenant, ApiError> {
    // Browser provenance is enforced by the outer ForkLaunch HTTP boundary.
    // Tenant membership is revalidated here, including each SSE refresh.
    Ok(state
        .security
        .authenticate(cookie(headers), id(workspace)?)
        .await?)
}

/// A verified tenant and validated route identity, resolved once before JSON parsing.
pub(crate) struct NoteContext {
    pub tenant: VerifiedTenant,
    pub note_id: Option<uuid::Uuid>,
    pub attachment_id: Option<uuid::Uuid>,
}
impl NoteContext {
    pub fn note_id(&self) -> Result<uuid::Uuid, framework::native::EndpointError> {
        self.note_id
            .ok_or(framework::native::EndpointError::InvalidRequest)
    }
}
#[derive(Clone)]
pub(crate) struct NotesPolicy;
impl framework::native::EndpointPolicy<AppState> for NotesPolicy {
    type Context = NoteContext;
    async fn authorize(
        &self,
        parts: &mut axum::http::request::Parts,
        state: &AppState,
        _: &framework::native::RouteContract,
    ) -> Result<NoteContext, framework::native::EndpointError> {
        use axum::extract::FromRequestParts;
        let axum::extract::Path(path) = axum::extract::Path::<
            std::collections::HashMap<String, String>,
        >::from_request_parts(parts, state)
        .await
        .map_err(|_| framework::native::EndpointError::InvalidRequest)?;
        let workspace = path
            .get("workspace_id")
            .ok_or(framework::native::EndpointError::InvalidRequest)?;
        let tenant = scope(state, &parts.headers, workspace).await?;
        let note_id = path.get("note_id").map(|value| id(value)).transpose()?;
        let attachment_id = path
            .get("attachment_id")
            .map(|value| id(value))
            .transpose()?;
        Ok(NoteContext {
            tenant,
            note_id,
            attachment_id,
        })
    }
}
