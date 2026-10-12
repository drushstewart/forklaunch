//! Raw authentication routes, mounted with `ForkLaunchRouter::nest_internal`.
//! This API is deliberately separate from the module's contract/MCP registry.
use crate::{
    cookies::SessionCookie,
    keyring::KeyRing,
    origins::OriginPolicy,
    sessions::{SessionService, SessionView},
    store::AuthStore,
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};
use std::sync::Arc;
use url::Url;
use zeroize::Zeroizing;

mod callback;
mod credential;
mod rate;
mod request;
#[cfg(test)]
pub(crate) mod tests;

/// Trusted, tenant-bound dependencies. Requests cannot select storage tenants,
/// signing secrets, clocks or JWT issuer/audience through headers/body fields.
pub struct AuthHttp<S> {
    store: Arc<S>,
    secret: Zeroizing<Vec<u8>>,
    base_url: String,
    policy: Arc<OriginPolicy>,
    cookie: SessionCookie,
    limiter: rate::RateLimiter,
    clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}
impl<S: AuthStore + 'static> AuthHttp<S> {
    /// # Errors
    /// Rejects invalid base URLs or signing secrets shorter than 32 bytes.
    pub fn new(
        store: Arc<S>,
        base_url: String,
        secret: Vec<u8>,
        trusted: Vec<String>,
    ) -> BlueprintResult<Self> {
        let secret = Zeroizing::new(secret);
        if secret.len() < 32 {
            return Err(BlueprintError::new(
                "better_auth_invalid_config",
                "Authentication secret must contain at least 32 bytes",
            ));
        }
        let policy = OriginPolicy::new(&base_url, trusted)?;
        let secure = Url::parse(&base_url).is_ok_and(|url| url.scheme() == "https");
        Ok(Self {
            store,
            secret,
            base_url,
            policy: Arc::new(policy),
            cookie: SessionCookie { secure },
            limiter: rate::RateLimiter::new(),
            clock: Arc::new(Utc::now),
        })
    }
    /// Injects trusted server time for deterministic integration fixtures.
    #[must_use]
    pub fn with_clock(mut self, clock: impl Fn() -> DateTime<Utc> + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }
    /// Trusted operator override of the configured default (enabled when
    /// `NODE_ENV` is production). This setting cannot be selected by a request.
    #[must_use]
    pub fn with_rate_limiting(mut self, enabled: bool) -> Self {
        self.limiter.enabled = enabled;
        self
    }
    /// Credentialed browser policy for the outer `ForkLaunchRouter`. Install this
    /// as `RouterConfig.cors` so global middleware preserves explicit JWT header
    /// exposure; a wildcard expose policy does not work with browser cookies.
    pub fn cors_layer(&self) -> tower_http::cors::CorsLayer {
        use tower_http::cors::{AllowHeaders, AllowOrigin, CorsLayer};
        let policy = self.policy.clone();
        CorsLayer::new()
            .allow_credentials(true)
            .allow_origin(AllowOrigin::predicate(move |origin, _| {
                origin
                    .to_str()
                    .is_ok_and(|origin| policy.trusted(origin, false))
            }))
            .allow_methods([
                Method::GET,
                Method::HEAD,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers(AllowHeaders::mirror_request())
            .expose_headers([
                header::HeaderName::from_static("set-auth-jwt"),
                header::HeaderName::from_static("x-request-id"),
            ])
    }
    /// Credential/session/JWT routes. Remaining user/account/provider/organization
    /// endpoints are added incrementally without permissive placeholders.
    pub fn router(self) -> Router {
        let app = Arc::new(self);
        Router::new()
            .route("/sign-up/email", post(dispatch::<S>))
            .route("/sign-in/email", post(dispatch::<S>))
            .route("/list-accounts", get(dispatch::<S>).head(not_found))
            .route("/unlink-account", post(dispatch::<S>))
            .route("/update-user", post(dispatch::<S>))
            .route("/reset-password", post(dispatch::<S>))
            .route(
                "/reset-password/{token}",
                get(callback::reset::<S>).head(not_found),
            )
            .route(
                "/delete-user/callback",
                get(callback::delete::<S>).head(not_found),
            )
            .route("/request-password-reset", post(dispatch::<S>))
            .route("/send-verification-email", post(dispatch::<S>))
            .route("/change-email", post(dispatch::<S>))
            .route("/delete-user", post(dispatch::<S>))
            .route("/verify-password", post(dispatch::<S>))
            .route("/change-password", post(dispatch::<S>))
            .route(
                "/ok",
                get(|| async { Json(json!({"ok":true})) }).head(not_found),
            )
            .route("/jwks", get(dispatch::<S>).head(not_found))
            .route("/token", get(dispatch::<S>).head(not_found))
            .route(
                "/get-session",
                get(dispatch::<S>).head(not_found).post(dispatch::<S>),
            )
            .route("/list-sessions", get(dispatch::<S>).head(not_found))
            .route("/revoke-session", post(dispatch::<S>))
            .route("/revoke-sessions", post(dispatch::<S>))
            .route("/revoke-other-sessions", post(dispatch::<S>))
            .route("/sign-out", post(dispatch::<S>))
            .method_not_allowed_fallback(not_found)
            .fallback(not_found)
            .layer(axum::middleware::from_fn_with_state(
                app.clone(),
                rate::guard::<S>,
            ))
            .with_state(app)
    }
    async fn session_response(
        &self,
        view: Option<SessionView>,
        pending: &mut HeaderMap,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        if let Some(view) = &view {
            let jwt = self.jwt(view, now).await?;
            pending.insert(
                "set-auth-jwt",
                HeaderValue::from_str(&jwt).map_err(|_| internal())?,
            );
            pending.insert(
                header::ACCESS_CONTROL_EXPOSE_HEADERS,
                HeaderValue::from_static("set-auth-jwt"),
            );
        }
        Ok(view.map_or(Value::Null, |view| view.body()))
    }
    async fn jwt(&self, view: &SessionView, now: DateTime<Utc>) -> BlueprintResult<String> {
        KeyRing::new(self.store.clone(), &self.base_url)
            .issue(
                text(&view.user, "id")?,
                text(&view.user, "email")?,
                view.session["activeOrganizationId"].as_str(),
                now,
            )
            .await
    }
}

async fn dispatch<S: AuthStore + 'static>(
    State(app): State<Arc<AuthHttp<S>>>,
    request: Request,
) -> Response {
    let mut pending = HeaderMap::new();
    let response = handle(&app, request, &mut pending).await;
    let mut response = match response {
        Ok(body) => Json(body).into_response(),
        Err(error) => failure(&error),
    };
    for (name, value) in &pending {
        response.headers_mut().append(name, value.clone());
    }
    response
}
async fn handle<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    request: Request,
    pending: &mut HeaderMap,
) -> BlueprintResult<Value> {
    let (parts, body) = request.into_parts();
    let path = parts.uri.path();
    let query = request::parse_query(parts.uri.query().unwrap_or(""));
    let (token, transient) = request::session_credentials(app, &parts.headers);
    let mutation = parts.method == Method::POST;
    let payload = request::parse_body(path, mutation, body, &parts.headers).await?;
    if mutation {
        request::guard(
            app,
            &parts,
            payload.as_ref().unwrap_or(&Value::Null),
            &query,
            false,
        )?;
    }
    request::validate_body(path, payload.as_ref())?;
    let payload = payload.unwrap_or(Value::Null);
    if path == "/get-session" {
        pending.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        pending.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
        if mutation {
            return Err(BlueprintError::new(
                "METHOD_NOT_ALLOWED_DEFER_SESSION_REQUIRED",
                "POST method requires deferSessionRefresh to be enabled in session config",
            ));
        }
    }
    let now = (app.clock)();
    if matches!(path, "/sign-up/email" | "/sign-in/email") {
        request::guard(app, &parts, &payload, &query, true)?;
        return credential::login(app, path, payload, &parts.headers, pending, now).await;
    }
    if let Some(result) = credential::public(app, path, &payload, &query, now).await {
        return result;
    }
    if path == "/jwks" {
        return KeyRing::new(app.store.clone(), &app.base_url)
            .jwks(now)
            .await;
    }
    let sessions = SessionService::new(app.store.clone());
    if path == "/sign-out" {
        sessions.sign_out(&token).await?;
        clear(app.cookie, pending)?;
        return Ok(json!({"success":true}));
    }
    let disable_refresh = query
        .get("disableRefresh")
        .is_some_and(|value| value.as_str().is_none_or(|value| !value.is_empty()));
    let view = sessions
        .get(
            &token,
            transient,
            disable_refresh && path == "/get-session",
            now,
        )
        .await?;
    if let Some(view) = &view {
        if view.refreshed {
            set_cookie(pending, &app.cookie.serialize(&token, &app.secret, true))?;
        }
    } else if !token.is_empty() {
        clear(app.cookie, pending)?;
    }
    if path == "/get-session" {
        return app.session_response(view, pending, now).await;
    }
    let view = view.ok_or_else(|| BlueprintError::new("UNAUTHORIZED", "Unauthorized"))?;
    match path {
        "/verify-password" | "/change-password" | "/list-accounts" | "/unlink-account"
        | "/update-user" | "/change-email" | "/delete-user" => {
            credential::authenticated(
                app,
                path,
                payload,
                (&token, transient),
                &parts.headers,
                pending,
                now,
            )
            .await
        }
        "/token" => Ok(json!({"token":app.jwt(&view, now).await?})),
        "/list-sessions" => sessions.list(&token, transient, now).await,
        "/revoke-session" => {
            sessions
                .revoke_one(
                    &token,
                    payload["token"].as_str().unwrap_or(""),
                    transient,
                    now,
                )
                .await
        }
        "/revoke-sessions" => sessions.revoke_all(&token, transient, now).await,
        "/revoke-other-sessions" => sessions.revoke_others(&token, transient, now).await,
        _ => Err(internal()),
    }
}
fn value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}
fn text<'a>(row: &'a Value, field: &str) -> BlueprintResult<&'a str> {
    row[field].as_str().ok_or_else(internal)
}
fn internal() -> BlueprintError {
    BlueprintError::new("INTERNAL_SERVER_ERROR", "Authentication operation failed")
}
fn set_cookie(headers: &mut HeaderMap, cookie: &str) -> BlueprintResult<()> {
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(cookie).map_err(|_| internal())?,
    );
    Ok(())
}
fn clear(cookie: SessionCookie, headers: &mut HeaderMap) -> BlueprintResult<()> {
    for cookie in cookie.clear_all() {
        set_cookie(headers, &cookie)?;
    }
    Ok(())
}
fn failure(error: &BlueprintError) -> Response {
    if error.code == "HTTP_NOT_FOUND" {
        return StatusCode::NOT_FOUND.into_response();
    }
    let status = match error.code.as_str() {
        "UNAUTHORIZED" | "INVALID_EMAIL_OR_PASSWORD" => StatusCode::UNAUTHORIZED,
        "SESSION_NOT_FRESH"
        | "MISSING_OR_NULL_ORIGIN"
        | "INVALID_ORIGIN"
        | "INVALID_CALLBACK_URL"
        | "INVALID_REDIRECT_URL"
        | "INVALID_ERROR_CALLBACK_URL"
        | "INVALID_NEW_USER_CALLBACK_URL"
        | "CROSS_SITE_NAVIGATION_LOGIN_BLOCKED" => StatusCode::FORBIDDEN,
        "USER_ALREADY_EXISTS_USE_ANOTHER_EMAIL" => StatusCode::UNPROCESSABLE_ENTITY,
        "UNSUPPORTED_MEDIA_TYPE" => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "METHOD_NOT_ALLOWED_DEFER_SESSION_REQUIRED" => StatusCode::METHOD_NOT_ALLOWED,
        "PAYLOAD_TOO_LARGE" => StatusCode::PAYLOAD_TOO_LARGE,
        code if code.starts_with("better_auth_") || code == "INTERNAL_SERVER_ERROR" => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
        _ => StatusCode::BAD_REQUEST,
    };
    let body = if matches!(
        error.code.as_str(),
        "INVALID_CALLBACK_VALUE_TYPE" | "HTTP_MESSAGE_ONLY"
    ) {
        json!({"message":error.message})
    } else if status == StatusCode::INTERNAL_SERVER_ERROR {
        json!({"code":"INTERNAL_SERVER_ERROR","message":"Authentication operation failed"})
    } else {
        json!({"code":error.code,"message":error.message})
    };
    (status, Json(body)).into_response()
}

async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}
