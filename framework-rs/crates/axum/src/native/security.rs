//! Secure defaults around native routers; application authorization remains explicit.
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use std::{fmt, time::Duration};
use tower_http::limit::RequestBodyLimitLayer;

const MANAGED_RESPONSE_HEADERS: [(&str, &str); 5] = [
    ("cache-control", "private, no-store"),
    ("pragma", "no-cache"),
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "DENY"),
    ("referrer-policy", "no-referrer"),
];

/// Configuration for a private API behind its trusted TLS ingress.
///
/// Origin is checked on every request (including WebSocket upgrades). Requests
/// without browser headers are allowed for native clients; this is not an
/// authentication mechanism. Keep session/tenant verification in typed extractors.
#[derive(Clone, Debug)]
pub struct NativeHttpPolicy {
    origin: HeaderValue,
    max_body_bytes: usize,
    response_timeout: Duration,
}

/// Invalid startup security configuration. Never contains configuration values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeHttpPolicyError;
impl fmt::Display for NativeHttpPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid native HTTP security policy")
    }
}
impl std::error::Error for NativeHttpPolicyError {}

impl NativeHttpPolicy {
    /// Configure a single exact public HTTP(S) origin, without a path or slash.
    ///
    /// # Errors
    /// Rejects malformed origins, credentials, paths, query strings and fragments.
    /// Plain HTTP is accepted only for explicit loopback development origins.
    pub fn new(public_origin: &str) -> Result<Self, NativeHttpPolicyError> {
        let uri: Uri = public_origin.parse().map_err(|_| NativeHttpPolicyError)?;
        let scheme = uri.scheme_str().ok_or(NativeHttpPolicyError)?;
        let authority = uri.authority().ok_or(NativeHttpPolicyError)?;
        let host = authority
            .host()
            .trim_start_matches('[')
            .trim_end_matches(']');
        let loopback = host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        if (scheme == "http" && !loopback)
            || !matches!(scheme, "http" | "https")
            || authority.as_str().contains('@')
            || authority.host().is_empty()
            || public_origin != format!("{scheme}://{authority}")
        {
            return Err(NativeHttpPolicyError);
        }
        Ok(Self {
            origin: public_origin.parse().map_err(|_| NativeHttpPolicyError)?,
            max_body_bytes: 2 * 1024 * 1024,
            response_timeout: Duration::from_secs(30),
        })
    }

    /// Validate response header declarations against the managed security boundary.
    /// Call this before [`secure`] when composing a router without `NativeApplication`.
    /// # Errors
    /// Rejects schemas that cannot admit the actual fixed security header value.
    // Keep admission on the policy instance so future configurable values share this boundary.
    #[allow(clippy::unused_self)]
    pub fn validate_contracts(
        &self,
        registry: &forklaunch_core::contract::ContractRegistry,
    ) -> Result<(), NativeHttpPolicyError> {
        use forklaunch_core::contract::schema_validation::compile_checked;
        for header in registry
            .routes()
            .iter()
            .flat_map(|route| &route.responses)
            .flat_map(|response| &response.headers)
        {
            let Some((_, value)) = MANAGED_RESPONSE_HEADERS
                .iter()
                .find(|(name, _)| header.name.eq_ignore_ascii_case(name))
            else {
                continue;
            };
            let schema = header
                .schema
                .json_schema
                .as_ref()
                .ok_or(NativeHttpPolicyError)?;
            let schema = serde_json::from_str(schema).map_err(|_| NativeHttpPolicyError)?;
            let validator = compile_checked(&schema).map_err(|_| NativeHttpPolicyError)?;
            validator(&serde_json::Value::String((*value).into()))
                .map_err(|_| NativeHttpPolicyError)?;
        }
        Ok(())
    }

    /// Set the maximum incoming body size, including streamed/chunked bodies.
    /// # Errors
    /// Rejects zero (which is usually a configuration mistake).
    pub fn with_max_body_bytes(mut self, bytes: usize) -> Result<Self, NativeHttpPolicyError> {
        if bytes == 0 {
            return Err(NativeHttpPolicyError);
        }
        self.max_body_bytes = bytes;
        Ok(self)
    }

    /// Bound time to response headers; SSE bodies and upgraded sockets are not timed out.
    /// # Errors
    /// Rejects zero durations.
    pub fn with_response_timeout(
        mut self,
        timeout: Duration,
    ) -> Result<Self, NativeHttpPolicyError> {
        if timeout.is_zero() {
            return Err(NativeHttpPolicyError);
        }
        self.response_timeout = timeout;
        Ok(self)
    }

    /// Validate browser provenance before any authentication or application side effects.
    ///
    /// # Errors
    /// Returns a fixed forbidden response for hostile, duplicated or malformed headers.
    pub fn check_origin(&self, headers: &HeaderMap) -> Result<(), NativeHttpRejection> {
        let mut origins = headers.get_all("origin").iter();
        if let Some(origin) = origins.next()
            && (origin != self.origin || origins.next().is_some())
        {
            return Err(NativeHttpRejection::OriginDenied);
        }
        let mut sites = headers.get_all("sec-fetch-site").iter();
        if let Some(site) = sites.next()
            && (!matches!(site.as_bytes(), b"same-origin" | b"none") || sites.next().is_some())
        {
            return Err(NativeHttpRejection::OriginDenied);
        }
        Ok(())
    }
}

/// Framework failures expose fixed messages, never internal errors or request data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeHttpRejection {
    OriginDenied,
    BodyTooLarge,
    Timeout,
}
impl IntoResponse for NativeHttpRejection {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::OriginDenied => (StatusCode::FORBIDDEN, "Origin denied"),
            Self::BodyTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "Request body too large"),
            Self::Timeout => (StatusCode::REQUEST_TIMEOUT, "Request timed out"),
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

async fn protect(State(policy): State<NativeHttpPolicy>, request: Request, next: Next) -> Response {
    let mut response = match policy.check_origin(request.headers()) {
        Err(rejection) => rejection.into_response(),
        Ok(()) => match tokio::time::timeout(policy.response_timeout, next.run(request)).await {
            Ok(response) if response.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                NativeHttpRejection::BodyTooLarge.into_response()
            }
            Ok(response) => response,
            Err(_) => NativeHttpRejection::Timeout.into_response(),
        },
    };
    // Applied outside rejection/limit/timeout paths and overwrite unsafe handler defaults.
    for (name, value) in MANAGED_RESPONSE_HEADERS {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    response
}

/// Install `ForkLaunch`'s private API boundary after composing all routes and MCP.
///
/// Returns an ordinary Axum router. Add routes before calling this function so all
/// routes receive its layers. Ingress still owns TLS/HSTS; applications still own
/// tenant authorization and SSE/socket session revalidation. Native requests may
/// omit Origin, so this does not replace authentication or signed service credentials.
#[must_use = "use the returned router to install the security boundary"]
pub fn secure<S: Clone + Send + Sync + 'static>(
    router: Router<S>,
    policy: NativeHttpPolicy,
) -> Router<S> {
    super::instrument(
        router
            .layer(DefaultBodyLimit::max(policy.max_body_bytes))
            .layer(RequestBodyLimitLayer::new(policy.max_body_bytes))
            .layer(middleware::from_fn_with_state(policy, protect)),
    )
}

#[cfg(test)]
#[path = "security_tests.rs"]
mod tests;
