//! Opt-in bounded validation of finite response bodies.
use super::framework_response_to_axum;
use axum::{
    body::{Body, to_bytes},
    response::Response,
};
use forklaunch_contract::{BodyKind, HttpMethod, ResponseContract, RouteContract, ValidationMode};
use forklaunch_http::{
    Body as FrameworkBody, ErrorKind, ErrorResponse, FrameworkRequest, FrameworkResponse,
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

#[path = "response_warning.rs"]
mod warning;

type Validator =
    dyn Fn(&FrameworkRequest, &FrameworkResponse) -> Option<FrameworkResponse> + Send + Sync;

#[derive(Clone)]
struct Registration {
    schemas: Vec<ResponseContract>,
    max_bytes: usize,
    timeout: Duration,
    validate: Arc<Validator>,
    warning: bool,
}

/// Validators for explicitly declared, finite JSON/text responses.
/// Only registered routes in `Error` response-validation mode are buffered.
/// Native status validation runs first. Callbacks receive UTF-8 body text (or
/// `Body::Empty`), and may return a replacement response with public diagnostics.
/// No validator semantics or error format are inferred from schema metadata.
/// Byte/time bounds apply to body collection; trusted synchronous callbacks
/// must return promptly and are not preempted by the collection timeout.
/// Warning/none modes leave responses untouched and do not invoke callbacks
/// unless Warning observation is explicitly enabled with `enable_warning`.
/// HEAD and bodyless HTTP statuses (204/205/304) also bypass these callbacks:
/// the transport may already have removed the body before this layer runs.
/// Both header and body checks are skipped in those cases; this is not an
/// assertion of TypeScript HEAD validation equivalence. Callback headers use
/// the single-value `FrameworkResponse` map; repeated headers are flattened
/// for inspection but preserved on the wire unless replaced by the callback.
#[derive(Clone, Default)]
pub struct ResponseValidators {
    entries: BTreeMap<String, Registration>,
}

impl ResponseValidators {
    /// Register bounded validation for an unversioned finite-response contract.
    /// # Errors
    /// Rejects empty/streaming contracts, versioned routes, duplicate operations,
    /// or zero byte/time limits. Body kinds other than JSON/text are unsupported.
    pub fn insert(
        &mut self,
        route: &RouteContract,
        max_bytes: usize,
        timeout: Duration,
        validate: impl Fn(&FrameworkRequest, &FrameworkResponse) -> Option<FrameworkResponse>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), String> {
        if !route.versions.is_empty()
            || route.responses.is_empty()
            || route.responses.iter().any(|response| {
                response
                    .body
                    .as_ref()
                    .is_some_and(|body| !matches!(body.kind, BodyKind::Json | BodyKind::Text))
            })
        {
            return Err(
                "Response validators require declared unversioned JSON/text responses".into(),
            );
        }
        if max_bytes == 0 || timeout.is_zero() {
            return Err("Response validation byte and time limits must be positive".into());
        }
        let key = key(route);
        if self.entries.contains_key(&key) {
            return Err("Duplicate response validator operation".into());
        }
        self.entries.insert(
            key,
            Registration {
                schemas: route.responses.clone(),
                max_bytes,
                timeout,
                validate: Arc::new(validate),
                warning: false,
            },
        );
        Ok(())
    }

    /// Opt into bounded observation in Warning mode for an existing registration.
    /// None still bypasses validation. Warnings preserve original frames/headers;
    /// collection bounds, stream errors and invalid UTF-8 skip the callback and
    /// emit a sanitized event. Callback replacement diagnostics are never sent
    /// or logged in Warning mode. HEAD/bodyless exclusions still apply.
    /// # Errors
    /// Rejects absent registrations or changed response schemas.
    pub fn enable_warning(&mut self, route: &RouteContract) -> Result<(), String> {
        let entry = self
            .entries
            .get_mut(&key(route))
            .ok_or("Warning observation requires a registered response validator")?;
        if !route.versions.is_empty() || route.responses != entry.schemas {
            return Err("Response validator schemas no longer match their route".into());
        }
        entry.warning = true;
        Ok(())
    }

    pub(super) fn validate_routes(&self, routes: &[RouteContract]) -> Result<(), String> {
        for (key, registration) in &self.entries {
            let matches: Vec<_> = routes
                .iter()
                .filter(|route| self::key(route) == *key)
                .collect();
            if matches.len() != 1 {
                return Err("Response validator must identify exactly one route".into());
            }
            if !matches[0].versions.is_empty() || matches[0].responses != registration.schemas {
                return Err("Response validator schemas no longer match their route".into());
            }
        }
        Ok(())
    }

    pub(super) fn extend(&mut self, other: Self) {
        for (key, entry) in other.entries {
            assert!(
                self.entries.insert(key, entry).is_none(),
                "duplicate response validator operation"
            );
        }
    }

    pub(super) async fn validate(
        &self,
        route: &RouteContract,
        request: &FrameworkRequest,
        response: Response,
    ) -> Response {
        if request.method == HttpMethod::Head
            || matches!(response.status().as_u16(), 204 | 205 | 304)
        {
            return response;
        }
        let Some(entry) = self.entries.get(&key(route)) else {
            return response;
        };
        if route.options.response_validation == ValidationMode::Warning && entry.warning {
            return warning::validate(entry, route, request, response).await;
        }
        if route.options.response_validation != ValidationMode::Error {
            return response;
        }
        let (mut parts, body) = response.into_parts();
        let Ok(Ok(bytes)) =
            tokio::time::timeout(entry.timeout, to_bytes(body, entry.max_bytes)).await
        else {
            return safe_failure(request);
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            return safe_failure(request);
        };
        let candidate = FrameworkResponse {
            status_code: parts.status.as_u16(),
            headers: parts
                .headers
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|value| (name.to_string(), value.to_owned()))
                })
                .collect(),
            body: if bytes.is_empty() {
                FrameworkBody::Empty
            } else {
                FrameworkBody::Text(text.to_owned())
            },
        };
        if let Some(replacement) = (entry.validate)(request, &candidate) {
            let (replacement, body) = framework_response_to_axum(replacement).into_parts();
            parts.status = replacement.status;
            // Application metadata survives; original representation headers
            // cannot describe a replacement diagnostic body.
            for name in [
                "content-type",
                "content-length",
                "content-encoding",
                "content-range",
                "transfer-encoding",
                "etag",
                "digest",
                "content-digest",
                "repr-digest",
                "content-md5",
                "trailer",
                "last-modified",
            ] {
                parts.headers.remove(name);
            }
            parts.headers.extend(replacement.headers);
            parts.extensions.extend(replacement.extensions);
            return Response::from_parts(parts, body);
        }
        // Preserve original headers (including repeated values) and extensions.
        Response::from_parts(parts, Body::from(bytes))
    }
}

fn key(route: &RouteContract) -> String {
    format!("{} {}", route.method.as_str(), route.operation_name)
}
fn safe_failure(request: &FrameworkRequest) -> Response {
    framework_response_to_axum(
        ErrorResponse::new(ErrorKind::Internal, "internal server error")
            .with_correlation_id(request.context.correlation_id.clone())
            .to_response(),
    )
}

#[cfg(test)]
#[path = "response_validation_tests.rs"]
mod tests;
