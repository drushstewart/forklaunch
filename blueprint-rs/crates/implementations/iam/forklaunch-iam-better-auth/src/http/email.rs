//! Stateless signed email verification and configured email-change token flows.
use super::{AuthHttp, callback, clear, failure, internal, request, set_cookie, text};
use crate::{
    credentials::{EmailTokenError, VerifiedEmailToken, verify_email_token},
    sessions::{SessionService, load, public_user},
    store::{AuthStore, AuthTransaction, Model},
};
use axum::{
    Json,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};
use std::sync::Arc;
use url::Url;

pub(super) async fn dispatch<S: AuthStore + 'static>(
    State(app): State<Arc<AuthHttp<S>>>,
    request: Request,
) -> Response {
    let (parts, _) = request.into_parts();
    let mut pending = HeaderMap::new();
    let mut response = match handle(&app, parts, &mut pending).await {
        Ok(response) => response,
        Err(error) => {
            if matches!(
                error.code.as_str(),
                "EMAIL_VERIFICATION_OBSERVER_FAILED" | "EMAIL_VERIFICATION_SESSION_FAILED"
            ) {
                pending.clear();
            }
            failure(&error)
        }
    };
    for (name, value) in &pending {
        response.headers_mut().append(name, value.clone());
    }
    response
}
// Keep token validation, existing change-email branches, and post-hook login ordering together.
#[allow(clippy::too_many_lines)]
async fn handle<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    parts: Parts,
    pending: &mut HeaderMap,
) -> BlueprintResult<Response> {
    let query = super::request::parse_query(parts.uri.query().unwrap_or(""));
    let errors: Vec<_> = [("token", true), ("callbackURL", false)]
        .into_iter()
        .filter_map(|(field, required)| {
            callback::query_string(&query, field, required)
                .err()
                .map(|error| error.message)
        })
        .collect();
    if !errors.is_empty() {
        return Err(BlueprintError::new("VALIDATION_ERROR", errors.join("; ")));
    }
    let token = callback::query_string(&query, "token", true)?.unwrap_or("");
    let destination =
        callback::query_string(&query, "callbackURL", false)?.filter(|value| !value.is_empty());
    if let Some(destination) = destination {
        callback::trusted(app, destination)?;
    }
    let now = (app.clock)();
    let claims = match verify_email_token(token, &app.secret, now) {
        Ok(claims) => claims,
        Err(EmailTokenError::InvalidClaims) => {
            return Ok(StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
        Err(EmailTokenError::InvalidToken) => {
            return token_error(destination, "INVALID_TOKEN", "Invalid token");
        }
        Err(EmailTokenError::TokenExpired) => {
            return token_error(destination, "TOKEN_EXPIRED", "Token expired");
        }
    };
    let mut tx = app.store.begin(false).await?;
    let user = tx
        .find_unique(Model::User, "email", &claims.email.to_lowercase())
        .await?;
    tx.commit().await?;
    let Some(user) = user else {
        return token_error(destination, "USER_NOT_FOUND", "User not found");
    };
    let (session_token, transient) = request::session_credentials(app, &parts.headers);
    let changing = claims
        .update_to
        .as_ref()
        .is_some_and(|email| !email.is_empty());
    let mut active = None;
    if changing {
        active = SessionService::new(app.store.clone())
            .with_session_policy(app.session_policy)
            .get(&session_token, transient, false, now)
            .await?;
        if let Some(view) = &active {
            if view.refreshed {
                set_cookie(
                    pending,
                    &app.cookie.serialize_with_policy(
                        &session_token,
                        &app.secret,
                        true,
                        app.session_policy,
                    ),
                )?;
            }
            if view.user["email"] != claims.email {
                return token_error(destination, "INVALID_USER", "Invalid user");
            }
        } else if !session_token.is_empty() {
            clear(app.cookie, pending)?;
        }
        if claims.request_type.as_deref() == Some("change-email-confirmation") {
            return Ok(success(destination, json!({"status":true})));
        }
    }
    if !changing
        && user["emailVerified"] != true
        && let Some(observer) = &app.before_email_verification
    {
        observe(
            observer.as_ref(),
            crate::accounts::email_observer::context(&user)?,
        )
        .await?;
    }
    let (ip, agent) = request::client_metadata(&parts.headers);
    let result = apply(
        app,
        &claims,
        &user,
        active.as_ref().map(|_| session_token.as_str()),
        &ip,
        &agent,
        now,
    )
    .await;
    let applied = match result {
        Ok(result) => result,
        Err(error) if matches!(error.code.as_str(), "INVALID_USER" | "USER_NOT_FOUND") => {
            return token_error(destination, &error.code, &error.message);
        }
        Err(error) => return Err(error),
    };
    let login = login_request(
        app.auto_sign_in_after_verification,
        &user,
        &claims,
        session_token,
        (ip, agent),
    )?;
    complete(app, pending, applied, destination, transient, login).await
}
async fn complete<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    pending: &mut HeaderMap,
    applied: Applied,
    destination: Option<&str>,
    transient: bool,
    login: Option<LoginRequest>,
) -> BlueprintResult<Response> {
    if let Some(context) = applied.after
        && let Some(observer) = &app.after_email_verification
    {
        observe(observer.as_ref(), context?).await?;
    }
    let session = if let Some(login) = login {
        Some(auto_login(app, pending, login, transient).await?)
    } else {
        applied.session
    };
    if let Some(session) = session {
        set_cookie(
            pending,
            &app.cookie.serialize_with_policy(
                text(&session, "token")?,
                &app.secret,
                !transient,
                app.session_policy,
            ),
        )?;
        if transient {
            set_cookie(pending, &app.cookie.transient_flag(&app.secret))?;
        }
    }
    Ok(success(destination, applied.body))
}
struct LoginRequest {
    user_id: String,
    email: String,
    token: String,
    ip: String,
    agent: String,
}
fn login_request(
    enabled: bool,
    user: &Value,
    claims: &VerifiedEmailToken,
    token: String,
    metadata: (String, String),
) -> BlueprintResult<Option<LoginRequest>> {
    if !enabled
        || user["emailVerified"] == true
        || claims
            .update_to
            .as_ref()
            .is_some_and(|email| !email.is_empty())
    {
        return Ok(None);
    }
    Ok(Some(LoginRequest {
        user_id: text(user, "id")?.to_owned(),
        email: claims.email.to_lowercase(),
        token,
        ip: metadata.0,
        agent: metadata.1,
    }))
}
fn login_failure() -> BlueprintError {
    BlueprintError::new(
        "EMAIL_VERIFICATION_SESSION_FAILED",
        "Authentication operation failed",
    )
}
async fn auto_login<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    pending: &mut HeaderMap,
    request: LoginRequest,
    transient: bool,
) -> BlueprintResult<Value> {
    // Expired-session cleanup/refresh commits independently before a new grant,
    // matching upstream's getSession-before-create partial-failure boundary.
    let view = SessionService::new(app.store.clone())
        .with_session_policy(app.session_policy)
        .get(&request.token, transient, false, (app.clock)())
        .await?;
    if let Some(view) = &view {
        if view.refreshed {
            set_cookie(
                pending,
                &app.cookie.serialize_with_policy(
                    &request.token,
                    &app.secret,
                    true,
                    app.session_policy,
                ),
            )?;
        }
    } else if !request.token.is_empty() {
        for cookie in app.cookie.clear_session() {
            set_cookie(pending, &cookie)?;
        }
    }
    let mut tx = app.store.begin(true).await.map_err(|_| login_failure())?;
    let now = (app.clock)();
    let user = tx
        .get(Model::User, &request.user_id)
        .await
        .map_err(|_| login_failure())?
        .filter(|user| {
            user["email"].as_str() == Some(request.email.as_str()) && user["emailVerified"] == true
        })
        .ok_or_else(|| BlueprintError::new("UNAUTHORIZED", "Unauthorized"))?;
    let current = load(&mut tx, &request.token, true, now)
        .await
        .map_err(|_| login_failure())?;
    let session = if let Some(current) = current.filter(|current| {
        current.user["id"] == user["id"] && current.user["email"] == user["email"]
    }) {
        current.session
    } else {
        let session = crate::sessions::new_session_with_policy(
            &request.user_id,
            &request.ip,
            &request.agent,
            now,
            app.session_policy,
            true,
        )
        .map_err(|_| login_failure())?;
        tx.insert(Model::Session, session.clone())
            .await
            .map_err(|_| login_failure())?;
        session
    };
    tx.commit().await.map_err(|_| login_failure())?;
    Ok(session)
}
struct Applied {
    body: Value,
    session: Option<Value>,
    after: Option<BlueprintResult<crate::accounts::EmailVerificationObserverContext>>,
}
async fn observe(
    observer: &dyn crate::accounts::EmailVerificationObserver,
    context: crate::accounts::EmailVerificationObserverContext,
) -> BlueprintResult<()> {
    observer
        .prepare(context)
        .map_err(|_| crate::accounts::email_observer::failure())?
        .await
        .map_err(|_| crate::accounts::email_observer::failure())
}
async fn apply<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    claims: &VerifiedEmailToken,
    previous: &Value,
    active_token: Option<&str>,
    ip: &str,
    agent: &str,
    now: DateTime<Utc>,
) -> BlueprintResult<Applied> {
    let mut tx = app.store.begin(true).await?;
    let mut user = tx
        .get(Model::User, text(previous, "id")?)
        .await?
        .ok_or_else(|| BlueprintError::new("USER_NOT_FOUND", "User not found"))?;
    if user["email"].as_str() != Some(&claims.email.to_lowercase()) {
        return Err(BlueprintError::new("INVALID_USER", "Invalid user"));
    }
    let mut session = None;
    let mut after = None;
    let body = if let Some(email) = claims.update_to.as_ref().filter(|email| !email.is_empty()) {
        let current = if let Some(token) = active_token {
            let current = load(&mut tx, token, true, now)
                .await?
                .ok_or_else(|| BlueprintError::new("INVALID_USER", "Invalid user"))?;
            if current.user["id"] != user["id"] || current.user["email"] != claims.email {
                return Err(BlueprintError::new("INVALID_USER", "Invalid user"));
            }
            current.session
        } else {
            let current = crate::sessions::new_session_with_policy(
                text(&user, "id")?,
                ip,
                agent,
                now,
                app.session_policy,
                true,
            )?;
            tx.insert(Model::Session, current.clone()).await?;
            current
        };
        user["email"] = json!(email.to_lowercase());
        user["emailVerified"] =
            json!(claims.request_type.as_deref() == Some("change-email-verification"));
        user["updatedAt"] = json!(now.to_rfc3339_opts(SecondsFormat::Millis, true));
        tx.replace(Model::User, user.clone()).await?;
        session = Some(current);
        if claims.request_type.as_deref() == Some("change-email-verification")
            && app.after_email_verification.is_some()
        {
            after = Some(crate::accounts::email_observer::context(&user));
        }
        json!({"status":true,"user":public_user(&user)})
    } else {
        if user["emailVerified"] != true {
            user["emailVerified"] = json!(true);
            user["updatedAt"] = json!(now.to_rfc3339_opts(SecondsFormat::Millis, true));
            tx.replace(Model::User, user.clone()).await?;
        }
        if previous["emailVerified"] != true && app.after_email_verification.is_some() {
            after = Some(crate::accounts::email_observer::context(&user));
        }
        json!({"status":true,"user":null})
    };
    tx.commit().await?;
    Ok(Applied {
        body,
        session,
        after,
    })
}
fn success(destination: Option<&str>, body: Value) -> Response {
    destination.map_or_else(|| Json(body).into_response(), redirect)
}
fn token_error(destination: Option<&str>, code: &str, message: &str) -> BlueprintResult<Response> {
    let Some(destination) = destination else {
        return Ok((
            StatusCode::UNAUTHORIZED,
            Json(json!({"code":code,"message":message})),
        )
            .into_response());
    };
    let relative = destination.starts_with('/');
    let origin = Url::parse("https://better-auth.invalid").map_err(|_| internal())?;
    let mut url = origin.join(destination).map_err(|_| internal())?;
    let query = url.query().unwrap_or("");
    let query = if query.is_empty() {
        format!("error={code}")
    } else {
        format!(
            "{query}{}error={code}",
            if query.ends_with('&') { "" } else { "&" }
        )
    };
    url.set_query(Some(&query));
    let destination = if relative {
        url.as_str()
            .strip_prefix("https://better-auth.invalid")
            .ok_or_else(internal)?
    } else {
        url.as_str()
    };
    Ok(redirect(destination))
}
fn redirect(destination: &str) -> Response {
    let bytes: Option<Vec<u8>> = destination
        .trim_matches([' ', '\t', '\r', '\n'])
        .chars()
        .map(|value| u8::try_from(u32::from(value)).ok())
        .collect();
    let Some(bytes) = bytes else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(location) = HeaderValue::from_bytes(&bytes) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::FOUND;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response.headers_mut().insert(header::LOCATION, location);
    response
}

impl<S: AuthStore + 'static> AuthHttp<S> {
    /// Enables explicit verification email requests. Delivery is always awaited;
    /// global background dispatch does not apply to this upstream endpoint.
    #[must_use]
    pub fn with_email_verification_delivery(
        mut self,
        delivery: Arc<dyn crate::accounts::EmailVerificationDelivery>,
    ) -> Self {
        self.email_verification_delivery = Some(delivery);
        self
    }
}

pub(super) async fn send<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    payload: &Value,
    credentials: (&str, bool),
    pending: &mut HeaderMap,
    now: DateTime<Utc>,
) -> BlueprintResult<Value> {
    let hook = app
        .email_verification_delivery
        .as_deref()
        .ok_or_else(internal)?;
    let email = text(payload, "email")?;
    let callback = payload["callbackURL"].as_str().unwrap_or("");
    if callback.len() > 8192 {
        return Err(BlueprintError::new(
            "INVALID_CALLBACK_URL",
            "Invalid callbackURL",
        ));
    }
    let view = SessionService::new(app.store.clone())
        .with_session_policy(app.session_policy)
        .get(credentials.0, credentials.1, false, now)
        .await?;
    super::refresh_cookie_headers(app, view.as_ref(), credentials.0, pending)?;
    if let Some(view) = view {
        if text(&view.user, "email")?.to_lowercase() != email.to_lowercase() {
            return Err(BlueprintError::new("EMAIL_MISMATCH", "Email mismatch"));
        }
        if view.user["emailVerified"] == true {
            return Err(BlueprintError::new(
                "EMAIL_ALREADY_VERIFIED",
                "Email is already verified",
            ));
        }
        crate::accounts::email_delivery::deliver(
            hook,
            &view.user,
            &app.auth_base_url(),
            callback,
            &app.secret,
            now,
        )
        .await?;
    } else {
        let start = tokio::time::Instant::now();
        let mut tx = app.store.begin(false).await?;
        let user = tx
            .find_unique(Model::User, "email", &email.to_lowercase())
            .await?;
        tx.commit().await?;
        let result = match user.filter(|u| u["emailVerified"] != true) {
            Some(user) => {
                crate::accounts::email_delivery::deliver(
                    hook,
                    &user,
                    &app.auth_base_url(),
                    callback,
                    &app.secret,
                    now,
                )
                .await
            }
            None => crate::accounts::email_delivery::token(email, &app.secret, now).map(|_| ()),
        };
        tokio::time::sleep_until(start + std::time::Duration::from_millis(500)).await;
        result?;
    }
    Ok(json!({"status":true}))
}

impl<S: AuthStore + 'static> AuthHttp<S> {
    /// Installs the awaited ordinary-proof pre-update `beforeEmailVerification` hook.
    #[must_use]
    pub fn with_before_email_verification(
        mut self,
        observer: Arc<dyn crate::accounts::EmailVerificationObserver>,
    ) -> Self {
        self.before_email_verification = Some(observer);
        self
    }
    /// Installs `afterEmailVerification` after committed ordinary verification or
    /// change-email verification, before response cookies. Does not use background dispatch.
    #[must_use]
    pub fn with_after_email_verification(
        mut self,
        observer: Arc<dyn crate::accounts::EmailVerificationObserver>,
    ) -> Self {
        self.after_email_verification = Some(observer);
        self
    }
}

#[cfg(test)]
#[path = "email_observer_tests.rs"]
pub(crate) mod observer_tests;

impl<S: AuthStore + 'static> AuthHttp<S> {
    /// Enables ordinary newly verified proof login after the after-verification hook.
    /// Defaults to false. Already-verified replay and change-email flows are unchanged.
    #[must_use]
    pub fn with_auto_sign_in_after_verification(mut self, enabled: bool) -> Self {
        self.auto_sign_in_after_verification = enabled;
        self
    }
}

#[cfg(test)]
#[path = "email_auto_tests.rs"]
pub(crate) mod auto_tests;
