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
    body::Body,
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

mod account_tokens;
#[cfg(feature = "application-http")]
pub mod application;
mod callback;
mod credential;
mod email;
mod external_jwt;
mod jwks_path;
mod jwt_expiration;
mod jwt_payload;
mod jwt_subject;
pub use external_jwt::ExternalJwtSigner;
pub use jwt_subject::JwtSessionContext;
mod observer;
mod organization;
pub use observer::{AuthApiError, AuthApiErrorObserverMode};
mod public;
mod rate;
pub use rate::{
    AuthRateLimitDecision, AuthRateLimitOptions, AuthRateLimitRequest, AuthRateLimitRule,
    AuthRateLimitStorageDecision,
};
#[cfg(feature = "application-http")]
pub mod relay;
mod request;
mod social;
mod social_callback;
mod social_link;
#[cfg(test)]
pub(crate) mod tests;

/// Trusted, tenant-bound dependencies. Requests cannot select storage tenants,
/// signing secrets, clocks or JWT issuer/audience through headers/body fields.
pub struct AuthHttp<S> {
    store: Arc<S>,
    google: Option<crate::oauth::google_client::GoogleClient>,
    external_jwt: Option<ExternalJwtSigner>,
    jwt_header_disabled: bool,
    jwt_expiration: Option<jwt_expiration::JwtExpiration>,
    subject_resolver: Option<jwt_subject::SubjectResolver>,
    payload_resolver: Option<jwt_payload::PayloadResolver>,
    jwks_grace_period: std::time::Duration,
    jwks_path: String,
    error_observer: Option<observer::Observer>,
    error_observer_mode: AuthApiErrorObserverMode,
    secret: Zeroizing<Vec<u8>>,
    base_url: String,
    base_path: String,
    production: bool,
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
        Self {
            store,
            google: None,
            external_jwt: None,
            jwt_header_disabled: false,
            jwt_expiration: None,
            subject_resolver: None,
            payload_resolver: None,
            jwks_grace_period: std::time::Duration::from_secs(2_592_000),
            jwks_path: "/jwks".into(),
            error_observer: None,
            error_observer_mode: AuthApiErrorObserverMode::default(),
            secret,
            base_url,
            base_path: "/api/auth".into(),
            production: std::env::var("NODE_ENV").is_ok_and(|value| value == "production"),
            policy: Arc::new(policy),
            cookie: SessionCookie { secure },
            limiter: rate::RateLimiter::new(),
            clock: Arc::new(Utc::now),
        }
        .with_base_path("/api/auth")
    }
    /// Suppresses automatic JWT signing and the `set-auth-jwt` header on
    /// get-session responses, matching `JwtOptions.disableSettingJwtHeader`.
    /// Explicit token issuance and JWKS publication remain available.
    #[must_use]
    pub const fn with_jwt_header_disabled(mut self, disabled: bool) -> Self {
        self.jwt_header_disabled = disabled;
        self
    }
    /// Sets how long expired public keys remain in the local JWKS response.
    /// Token signing eligibility and remote-JWKS suppression are unchanged.
    /// # Errors
    /// Rejects sub-millisecond precision or durations exceeding signed milliseconds.
    pub fn with_jwks_grace_period(mut self, period: std::time::Duration) -> BlueprintResult<Self> {
        crate::keyring::grace_millis(period)?;
        self.jwks_grace_period = period;
        Ok(self)
    }
    /// Sets a trusted fixed authentication namespace. A path already present
    /// in `base_url` takes precedence, matching Better Auth's URL resolution.
    /// # Errors
    /// Rejects parameterized, encoded or otherwise invalid namespaces.
    pub fn with_base_path(mut self, path: impl Into<String>) -> BlueprintResult<Self> {
        let path = path.into();
        let path = if path.starts_with('/') {
            path
        } else {
            format!("/{path}")
        };
        self.base_path = path;
        let effective = self.auth_base_path();
        if (effective.is_empty() && self.jwks_path == "/")
            || effective.chars().any(|character| {
                character.is_control()
                    || character.is_whitespace()
                    || matches!(character, '{' | '}' | ':' | '*' | '?' | '#' | '%' | '\\')
            })
            || self.base_path.chars().any(|character| {
                character.is_control()
                    || character.is_whitespace()
                    || matches!(character, '?' | '#' | '\\')
            })
            || self
                .base_path
                .split('/')
                .any(|part| matches!(part, "." | ".."))
        {
            return Err(BlueprintError::new(
                "better_auth_invalid_config",
                "Authentication base path must be a fixed namespace",
            ));
        }
        Ok(self)
    }
    /// Configured absolute authentication URL, including its namespace.
    /// # Panics
    /// Only invalid internal configuration can panic; construction validates it.
    #[must_use]
    pub fn auth_base_url(&self) -> String {
        let parsed = Url::parse(&self.base_url).expect("validated auth base URL");
        if parsed.path().trim_matches('/').is_empty() {
            let origin = self.base_url.trim_end_matches('/');
            if self.base_path == "/" {
                origin.to_owned()
            } else {
                format!("{origin}{}", self.base_path)
            }
        } else {
            self.base_url.clone()
        }
    }
    /// Canonical configured origin used by Better Auth's default JWT claims.
    /// # Panics
    /// Only invalid internal configuration can panic; construction validates it.
    #[must_use]
    pub fn auth_origin(&self) -> String {
        Url::parse(&self.base_url)
            .expect("validated auth base URL")
            .origin()
            .ascii_serialization()
    }
    /// Effective mount path, without a trailing slash.
    /// # Panics
    /// Only invalid internal configuration can panic; construction validates it.
    #[must_use]
    pub fn auth_base_path(&self) -> String {
        Url::parse(&self.auth_base_url())
            .expect("validated auth base URL")
            .path()
            .trim_end_matches('/')
            .to_owned()
    }
    /// Installs a trusted configured Google provider. Requests cannot override credentials.
    #[must_use]
    pub fn with_google(mut self, provider: crate::oauth::google_client::GoogleClient) -> Self {
        self.google = Some(provider);
        self
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
    /// Configures rate limits and optional custom storage; enablement is controlled separately.
    #[must_use]
    pub fn with_rate_limit_options(mut self, options: AuthRateLimitOptions) -> Self {
        self.limiter.options = options;
        self
    }
    /// Selects callback dispatch without changing responses or safe server-error
    /// tracing. The default retains the historical all-5xx callback behavior.
    #[must_use]
    pub fn with_api_error_observer_mode(mut self, mode: AuthApiErrorObserverMode) -> Self {
        self.error_observer_mode = mode;
        self
    }

    /// Installs a trusted diagnostic callback for raw-auth server failures.
    /// The default observes all5xx; unexpected-only mode excludes handled errors.
    /// Only sanitized route templates and structural metadata are supplied.
    /// Panics in this callback are isolated from the HTTP response.
    #[must_use]
    pub fn with_api_error_observer(
        mut self,
        observer: impl Fn(&AuthApiError) + Send + Sync + 'static,
    ) -> Self {
        self.error_observer = Some(Arc::new(observer));
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
            .route("/sign-in/social", post(dispatch::<S>))
            .route("/link-social", post(dispatch::<S>))
            .route(
                "/callback/{provider}",
                get(social_callback::dispatch::<S>)
                    .post(social_callback::dispatch::<S>)
                    .head(not_found),
            )
            .route("/error", get(public::error::<S>).head(not_found))
            .route(
                "/open-api/generate-schema",
                get(public::schema::<S>).head(not_found),
            )
            .route("/reference", get(public::reference).head(not_found))
            .route("/list-accounts", get(dispatch::<S>).head(not_found))
            .route("/account-info", get(dispatch::<S>).head(not_found))
            .route("/get-access-token", post(dispatch::<S>))
            .route("/refresh-token", post(dispatch::<S>))
            .route("/unlink-account", post(dispatch::<S>))
            .route("/update-user", post(dispatch::<S>))
            .route("/update-session", post(dispatch::<S>))
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
            .route("/verify-email", get(email::dispatch::<S>).head(not_found))
            .route(&app.jwks_path, get(dispatch::<S>).head(not_found))
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
            .merge(organization::routes::<S>())
            .method_not_allowed_fallback(not_found)
            .fallback(not_found)
            .layer(axum::middleware::from_fn_with_state(
                app.clone(),
                rate::guard::<S>,
            ))
            .layer(axum::middleware::from_fn_with_state(
                app.clone(),
                observer::observe::<S>,
            ))
            .with_state(app)
    }
    async fn session_response(
        &self,
        view: Option<SessionView>,
        pending: &mut HeaderMap,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        if !self.jwt_header_disabled
            && let Some(view) = &view
        {
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
        if self.payload_resolver.is_some() || self.jwt_expiration.is_some() {
            return self.jwt_with_payload(view, now).await;
        }
        let subject = self.jwt_subject(view).await?;
        if let Some(signer) = &self.external_jwt {
            let body = json!({"payload": {
                "iat": now.timestamp(),
                "sub": subject,
                "sessionId": text(&view.session, "id")?,
                "email": text(&view.user, "email")?,
                "activeOrganizationId": view.session["activeOrganizationId"]
            }});
            let result = KeyRing::new(self.store.clone(), self.auth_origin())
                .sign_jwt_with_signer(Some(&body), now, signer)
                .await?;
            return Ok(text(&result, "token")?.to_owned());
        }
        KeyRing::new(self.store.clone(), self.auth_origin())
            .issue_session(
                &subject,
                text(&view.user, "email")?,
                view.session["activeOrganizationId"].as_str(),
                Some(text(&view.session, "id")?),
                now,
            )
            .await
    }
}

async fn dispatch<S: AuthStore + 'static>(
    State(app): State<Arc<AuthHttp<S>>>,
    request: Request,
) -> Response {
    if app.jwks_path == "/"
        && request.uri().path() == "/"
        && request
            .extensions()
            .get::<axum::extract::OriginalUri>()
            .is_some_and(|original| !original.0.path().ends_with('/'))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mut pending = HeaderMap::new();
    let path = request.uri().path().to_owned();
    let response = handle(&app, request, &mut pending).await;
    let mut response = match response {
        Ok(body) => (
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            crate::ecmascript::stringify(&body),
        )
            .into_response(),
        Err(error) => {
            let response = failure_for_path(&error, &path);
            if error.code == "HTTP_INTERNAL_EMPTY"
                || (path == "/sign-up/email"
                    && response.status().is_server_error()
                    && !crate::error_origin::is_handled(&error))
            {
                pending.clear();
            }
            response
        }
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
    organization::validate_query(path, &query)?;
    if path == "/account-info" {
        account_tokens::validate(Some(&json!(query)), "query")?;
    }
    let mut payload = payload.unwrap_or(Value::Null);
    organization::strip_record_keys(path, &mut payload);
    session_headers(path, mutation, pending)?;
    let now = (app.clock)();
    if path == "/sign-in/social" {
        return social::sign_in(app, &payload, &parts.headers, pending, now).await;
    }
    if matches!(path, "/sign-up/email" | "/sign-in/email") {
        request::guard(app, &parts, &payload, &query, true)?;
        return credential::login(app, path, payload, &parts.headers, pending, now).await;
    }
    if let Some(result) = credential::public(app, path, &payload, &query, now).await {
        return result;
    }
    if path == app.jwks_path {
        return app.jwks(now).await;
    }
    let sessions = SessionService::new(app.store.clone());
    if path == "/sign-out" {
        return sign_out(app, &token, pending).await;
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
    refresh_cookie_headers(app, view.as_ref(), &token, pending)?;
    if path == "/get-session" {
        return app.session_response(view, pending, now).await;
    }
    if path.starts_with("/organization/") {
        if !organization::invitations::is_route(path) {
            required_view(view, path, &payload)?;
        }
        return organization::handle(
            app,
            path,
            &payload,
            &query,
            (&token, transient),
            pending,
            now,
        )
        .await;
    }
    let view = required_view(view, path, &payload)?;
    if matches!(
        path,
        "/account-info" | "/get-access-token" | "/refresh-token"
    ) {
        let selection = if path == "/account-info" {
            json!(query)
        } else {
            payload
        };
        return account_tokens::handle(app, path, &selection, text(&view.user, "id")?, now).await;
    }
    session_operation(
        app,
        path,
        payload,
        (&token, transient, view),
        &parts.headers,
        pending,
        now,
    )
    .await
}

async fn session_operation<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    path: &str,
    payload: Value,
    session: (&str, bool, SessionView),
    headers: &HeaderMap,
    pending: &mut HeaderMap,
    now: DateTime<Utc>,
) -> BlueprintResult<Value> {
    let (token, transient, view) = session;
    let sessions = SessionService::new(app.store.clone());
    match path {
        "/verify-password" | "/change-password" | "/list-accounts" | "/unlink-account"
        | "/update-user" | "/change-email" | "/delete-user" => {
            credential::authenticated(
                app,
                path,
                payload,
                (token, transient),
                headers,
                pending,
                now,
            )
            .await
        }
        "/update-session" => {
            for field in ["activeOrganizationId", "activeTeamId"] {
                if crate::keyring::verify::truthy(&payload[field]) {
                    return Err(BlueprintError::new(
                        "FIELD_NOT_ALLOWED",
                        format!("{field} is not allowed to be set"),
                    ));
                }
            }
            Err(BlueprintError::new(
                "HTTP_MESSAGE_ONLY",
                "No fields to update",
            ))
        }
        "/link-social" => social_link::link(app, &payload, &view, pending, now).await,
        "/token" => Ok(json!({"token":app.jwt(&view, now).await?})),
        "/list-sessions" => sessions.list(token, transient, now).await,
        "/revoke-session" => {
            sessions
                .revoke_one(
                    token,
                    payload["token"].as_str().unwrap_or(""),
                    transient,
                    now,
                )
                .await
        }
        "/revoke-sessions" => sessions.revoke_all(token, transient, now).await,
        "/revoke-other-sessions" => sessions.revoke_others(token, transient, now).await,
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
async fn sign_out<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    token: &str,
    pending: &mut HeaderMap,
) -> BlueprintResult<Value> {
    if !token.is_empty()
        && let Err(error) = SessionService::new(app.store.clone()).sign_out(token).await
    {
        crate::error_origin::log_handled_failure("/sign-out", &error);
    }
    for cookie in app.cookie.clear_session() {
        set_cookie(pending, &cookie)?;
    }
    Ok(json!({"success":true}))
}

fn clear(cookie: SessionCookie, headers: &mut HeaderMap) -> BlueprintResult<()> {
    for cookie in cookie.clear_all() {
        set_cookie(headers, &cookie)?;
    }
    Ok(())
}
fn failure_for_path(error: &BlueprintError, path: &str) -> Response {
    if path == "/sign-up/email" {
        let mut response = failure(error);
        if response.status().is_server_error() && !crate::error_origin::is_handled(error) {
            // The actual signup endpoint leaves unexpected adapter/transaction
            // errors to the router's empty500 boundary. Keep private diagnostics.
            *response.body_mut() = Body::empty();
            response
                .headers_mut()
                .remove(axum::http::header::CONTENT_TYPE);
            response
                .headers_mut()
                .remove(axum::http::header::CONTENT_LENGTH);
        }
        return response;
    }
    if error.code == "INVALID_TOKEN" && path == "/reset-password" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"code":error.code,"message":error.message})),
        )
            .into_response();
    }
    if error.code == "USER_IS_NOT_A_MEMBER_OF_THE_TEAM" && path == "/organization/set-active-team" {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"code":error.code,"message":error.message})),
        )
            .into_response();
    }
    if error.code == "UNAUTHORIZED" {
        if matches!(
            path,
            "/account-info" | "/get-access-token" | "/refresh-token"
        ) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        if path == "/organization/get-invitation" {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"message":"Not authenticated"})),
            )
                .into_response();
        }
        if matches!(
            path,
            "/organization/create"
                | "/organization/delete"
                | "/organization/create-team"
                | "/organization/remove-team"
        ) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        if path == "/organization/update" {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"message":"User not found"})),
            )
                .into_response();
        }
    }
    if error.code == "USER_IS_NOT_A_MEMBER_OF_THE_ORGANIZATION" {
        let status = match path {
            "/organization/update"
            | "/organization/delete"
            | "/organization/list-user-teams"
            | "/organization/add-team-member"
            | "/organization/remove-team-member" => StatusCode::BAD_REQUEST,
            "/organization/has-permission" => StatusCode::UNAUTHORIZED,
            _ => StatusCode::FORBIDDEN,
        };
        return (
            status,
            Json(json!({"code":error.code,"message":error.message})),
        )
            .into_response();
    }
    failure(error)
}
fn failure(error: &BlueprintError) -> Response {
    let mut response = failure_response(error);
    if response.status().is_server_error() {
        if crate::error_origin::is_handled(error) {
            response.extensions_mut().insert(observer::HandledResponse);
        }
        response
            .extensions_mut()
            .insert(observer::ErrorMetadata::from_error(error));
    }
    response
}
fn failure_response(error: &BlueprintError) -> Response {
    if error.code == "HTTP_FORBIDDEN_MESSAGE_ONLY" {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"message":error.message})),
        )
            .into_response();
    }
    if error.code == "HTTP_INTERNAL_EMPTY" {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if error.code == "HTTP_BAD_REQUEST_EMPTY" {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if error.code == "HTTP_INTERNAL_MESSAGE_ONLY" {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"message":error.message})),
        )
            .into_response();
    }
    if error.code == "HTTP_UNAUTHORIZED_EMPTY" {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if error.code == "HTTP_NOT_FOUND" {
        return StatusCode::NOT_FOUND.into_response();
    }
    let status = match error.code.as_str() {
        "PROVIDER_NOT_FOUND" => StatusCode::NOT_FOUND,
        "SOCIAL_ACCOUNT_ALREADY_LINKED" => StatusCode::CONFLICT,
        "UNAUTHORIZED"
        | "INVALID_TOKEN"
        | "USER_EMAIL_NOT_FOUND"
        | "FAILED_TO_GET_USER_INFO"
        | "OAUTH_LINK_ERROR"
        | "LINKING_NOT_ALLOWED"
        | "LINKING_DIFFERENT_EMAILS_NOT_ALLOWED"
        | "INVALID_EMAIL_OR_PASSWORD"
        | "YOU_ARE_NOT_ALLOWED_TO_DELETE_THIS_MEMBER" => StatusCode::UNAUTHORIZED,
        "YOU_ARE_NOT_A_MEMBER_OF_THIS_ORGANIZATION"
        | "YOU_ARE_NOT_ALLOWED_TO_ACCESS_THIS_ORGANIZATION"
        | "YOU_ARE_NOT_ALLOWED_TO_INVITE_USERS_TO_THIS_ORGANIZATION"
        | "YOU_ARE_NOT_ALLOWED_TO_CREATE_TEAMS_IN_THIS_ORGANIZATION"
        | "YOU_ARE_NOT_ALLOWED_TO_DELETE_TEAMS_IN_THIS_ORGANIZATION"
        | "YOU_ARE_NOT_ALLOWED_TO_UPDATE_THIS_TEAM"
        | "YOU_ARE_NOT_ALLOWED_TO_DELETE_THIS_TEAM"
        | "YOU_ARE_NOT_ALLOWED_TO_CREATE_A_NEW_TEAM_MEMBER"
        | "YOU_ARE_NOT_ALLOWED_TO_REMOVE_A_TEAM_MEMBER"
        | "YOU_ARE_NOT_ALLOWED_TO_UPDATE_THIS_MEMBER"
        | "YOU_ARE_NOT_ALLOWED_TO_UPDATE_THIS_ORGANIZATION"
        | "YOU_ARE_NOT_ALLOWED_TO_DELETE_THIS_ORGANIZATION"
        | "USER_IS_NOT_A_MEMBER_OF_THE_ORGANIZATION"
        | "YOU_ARE_NOT_THE_RECIPIENT_OF_THE_INVITATION"
        | "EMAIL_VERIFICATION_REQUIRED_FOR_INVITATION"
        | "EMAIL_VERIFICATION_REQUIRED_BEFORE_ACCEPTING_OR_REJECTING_INVITATION"
        | "YOU_ARE_NOT_ALLOWED_TO_CANCEL_THIS_INVITATION"
        | "YOU_ARE_NOT_ALLOWED_TO_INVITE_USER_WITH_THIS_ROLE"
        | "INVITATION_LIMIT_REACHED"
        | "ORGANIZATION_MEMBERSHIP_LIMIT_REACHED"
        | "YOU_ARE_NOT_ALLOWED_TO_LIST_A_ROLE"
        | "YOU_ARE_NOT_ALLOWED_TO_READ_A_ROLE"
        | "YOU_ARE_NOT_ALLOWED_TO_CREATE_A_ROLE"
        | "YOU_ARE_NOT_ALLOWED_TO_UPDATE_A_ROLE"
        | "YOU_ARE_NOT_ALLOWED_TO_DELETE_A_ROLE"
        | "SESSION_NOT_FRESH"
        | "MISSING_OR_NULL_ORIGIN"
        | "INVALID_ORIGIN"
        | "INVALID_CALLBACK_URL"
        | "INVALID_REDIRECT_URL"
        | "INVALID_ERROR_CALLBACK_URL"
        | "INVALID_NEW_USER_CALLBACK_URL"
        | "CROSS_SITE_NAVIGATION_LOGIN_BLOCKED" => StatusCode::FORBIDDEN,
        "USER_ALREADY_EXISTS_USE_ANOTHER_EMAIL" | "FAILED_TO_CREATE_USER" => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        "UNSUPPORTED_MEDIA_TYPE" => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "METHOD_NOT_ALLOWED_DEFER_SESSION_REQUIRED" => StatusCode::METHOD_NOT_ALLOWED,
        "PAYLOAD_TOO_LARGE" => StatusCode::PAYLOAD_TOO_LARGE,
        code if code.starts_with("better_auth_") || code == "INTERNAL_SERVER_ERROR" => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
        _ => StatusCode::BAD_REQUEST,
    };
    let mut body = if matches!(
        error.code.as_str(),
        "INVALID_CALLBACK_VALUE_TYPE" | "HTTP_MESSAGE_ONLY"
    ) {
        json!({"message":error.message})
    } else if status == StatusCode::INTERNAL_SERVER_ERROR {
        json!({"code":"INTERNAL_SERVER_ERROR","message":"Authentication operation failed"})
    } else {
        json!({"code":error.code,"message":error.message})
    };
    if matches!(
        error.code.as_str(),
        "YOU_ARE_NOT_ALLOWED_TO_CREATE_A_ROLE" | "YOU_ARE_NOT_ALLOWED_TO_UPDATE_A_ROLE"
    ) && let Some(missing) = error.details.get("missingPermissions")
    {
        body["missingPermissions"] = missing.clone();
    }
    (status, Json(body)).into_response()
}

async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

fn required_view(
    view: Option<SessionView>,
    path: &str,
    payload: &Value,
) -> BlueprintResult<SessionView> {
    if view.is_none()
        && path == "/organization/remove-team"
        && payload["organizationId"].as_str().is_none_or(str::is_empty)
    {
        return Err(BlueprintError::new(
            "NO_ACTIVE_ORGANIZATION",
            "No active organization",
        ));
    }
    view.ok_or_else(|| BlueprintError::new("UNAUTHORIZED", "Unauthorized"))
}

fn session_headers(path: &str, mutation: bool, pending: &mut HeaderMap) -> BlueprintResult<()> {
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

    Ok(())
}

fn refresh_cookie_headers<S>(
    app: &AuthHttp<S>,
    view: Option<&SessionView>,
    token: &str,
    pending: &mut HeaderMap,
) -> BlueprintResult<()> {
    if let Some(view) = view {
        if view.refreshed {
            set_cookie(pending, &app.cookie.serialize(token, &app.secret, true))?;
        }
    } else if !token.is_empty() {
        clear(app.cookie, pending)?;
    }

    Ok(())
}
