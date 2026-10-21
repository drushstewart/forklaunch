//! Signed worker callbacks are admitted against original bytes before JSON decoding.
use crate::{
    api::errors::{ApiError, id},
    registrations::AppState,
};
use axum::{extract::FromRequestParts, http::request::Parts};
use framework::native::{EndpointError, EndpointPolicy, RouteContract};

pub(crate) struct InternalContext {
    pub workspace_id: uuid::Uuid,
    pub note_id: uuid::Uuid,
}
#[derive(Clone)]
pub(crate) struct InternalPolicy;
impl framework_core::contract::bindings::OriginalBodySignedPolicy for InternalPolicy {}
impl EndpointPolicy<AppState> for InternalPolicy {
    const CAPABILITIES: framework::native::PolicyCapabilities =
        framework::native::PolicyCapabilities::SignedInternal;
    type Context = InternalContext;
    async fn authorize(
        &self,
        parts: &mut Parts,
        state: &AppState,
        _: &RouteContract,
    ) -> Result<InternalContext, EndpointError> {
        let axum::extract::Path(path) = axum::extract::Path::<
            std::collections::HashMap<String, String>,
        >::from_request_parts(parts, state)
        .await
        .map_err(|_| EndpointError::InvalidRequest)?;
        Ok(InternalContext {
            workspace_id: id(path
                .get("workspace_id")
                .ok_or(EndpointError::InvalidRequest)?)?,
            note_id: id(path.get("note_id").ok_or(EndpointError::InvalidRequest)?)?,
        })
    }
    async fn authorize_body(
        &self,
        context: InternalContext,
        parts: &mut Parts,
        bytes: &[u8],
        state: &AppState,
        _: &RouteContract,
    ) -> Result<InternalContext, EndpointError> {
        let credential = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        state
            .security
            .authenticate_signed_internal(credential, parts.uri.path(), bytes, |nonce, expiry| {
                state.notes.claim_internal_nonce(nonce, expiry)
            })
            .await
            .map_err(ApiError::from)?;
        Ok(context)
    }
}
