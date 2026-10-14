//! Safe diagnostics for server errors; authentication responses remain unchanged.
use super::{AuthHttp, AuthStore};
use axum::{
    extract::{MatchedPath, Request, State},
    middleware::Next,
    response::Response,
};
use forklaunch_blueprint_core::BlueprintError;
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

/// Diagnostic metadata excludes request data and freeform error messages/stacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthApiError {
    /// Router template, never a concrete path containing callback tokens.
    pub route: String,
    /// HTTP response status; raw authentication server failures are observed.
    pub status: u16,
    /// Allowlisted structural database metadata, when available.
    pub database: BTreeMap<String, String>,
}
/// Dispatch mode for a configured diagnostic callback. Wire responses are unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AuthApiErrorObserverMode {
    /// Compatibility behavior: every raw authentication 5xx invokes the callback.
    #[default]
    AllServerErrors,
    /// Matches the configured Better Auth hook: explicitly handled API errors
    /// skip the callback; unexpected server failures invoke it.
    UnexpectedOnly,
}

#[derive(Clone)]
pub(super) struct HandledResponse;

pub(super) type Observer = Arc<dyn Fn(&AuthApiError) + Send + Sync>;

#[derive(Clone, Default)]
pub(super) struct ErrorMetadata(BTreeMap<String, String>);
impl ErrorMetadata {
    pub(super) fn from_error(error: &BlueprintError) -> Self {
        let mut fields = BTreeMap::new();
        for field in ["code", "constraint", "table", "column"] {
            if let Some(value) = error.details.get(field).and_then(serde_json::Value::as_str)
                && !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
                && (field != "code" || value.len() == 5)
            {
                fields.insert(field.to_owned(), value.to_owned());
            }
        }
        Self(fields)
    }
}

pub(super) async fn observe<S: AuthStore + 'static>(
    State(app): State<Arc<AuthHttp<S>>>,
    request: Request,
    next: Next,
) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "<unmatched>".to_owned(), |path| path.as_str().to_owned());
    let mut response = next.run(request).await;
    let metadata = response.extensions_mut().remove::<ErrorMetadata>();
    let handled = response
        .extensions_mut()
        .remove::<HandledResponse>()
        .is_some();
    if response.status().is_server_error() {
        let event = AuthApiError {
            route,
            status: response.status().as_u16(),
            database: metadata.unwrap_or_default().0,
        };
        tracing::error!(target: "forklaunch_iam_better_auth", route = %event.route, status = event.status, database = ?event.database, "Better Auth API error");
        if (app.error_observer_mode == AuthApiErrorObserverMode::AllServerErrors || !handled)
            && let Some(observer) = &app.error_observer
            && catch_unwind(AssertUnwindSafe(|| observer(&event))).is_err()
        {
            tracing::error!(target: "forklaunch_iam_better_auth", "Better Auth API error observer panicked");
        }
    }
    response
}

#[cfg(test)]
mod tests;
