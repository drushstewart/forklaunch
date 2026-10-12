//! Browser callback consumes trusted persisted state before exchanging a code.
use super::{AuthHttp, failure, internal, set_cookie, text};
use crate::{
    oauth::{google_accounts::GoogleAccounts, state::StateService},
    store::AuthStore,
};
use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::Response,
};
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};

const FIELDS: &[&str] = &[
    "code",
    "error",
    "device_id",
    "error_description",
    "state",
    "user",
    "iss",
];
pub(super) async fn dispatch<S: AuthStore + 'static>(
    State(app): State<Arc<AuthHttp<S>>>,
    Path(provider): Path<String>,
    request: Request,
) -> Response {
    let mut pending = HeaderMap::new();
    let result = handle(&app, &provider, request, &mut pending).await;
    let mut response = match result {
        Ok(url) => redirect(&url).unwrap_or_else(|error| failure(&error)),
        Err(error) => {
            if error.code == "HTTP_INTERNAL_EMPTY" {
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
async fn handle<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    provider: &str,
    request: Request,
    pending: &mut HeaderMap,
) -> BlueprintResult<String> {
    let (parts, body) = request.into_parts();
    let query = super::request::parse_query(parts.uri.query().unwrap_or(""));
    let base = auth_base(app)?;
    let default_error = format!("{base}/error");
    if parts.method == Method::POST {
        let payload =
            super::request::parse_body("/callback/google", true, body, &parts.headers).await?;
        super::request::guard(
            app,
            &parts,
            payload.as_ref().unwrap_or(&Value::Null),
            &query,
            false,
        )?;
        validate(payload.as_ref(), "body")?;
        validate(Some(&json!(query)), "query")?;
        let mut params = url::form_urlencoded::Serializer::new(String::new());
        for field in FIELDS {
            if let Some(value) = query
                .get(*field)
                .or_else(|| payload.as_ref().and_then(|body| body.get(*field)))
            {
                params.append_pair(field, value.as_str().ok_or_else(internal)?);
            }
        }
        return Ok(format!("{base}/callback/{provider}?{}", params.finish()));
    }
    validate(Some(&json!(query)), "query")?;
    let state = string(&query, "state");
    if state.is_empty() {
        return with_error(&default_error, "state_not_found", None);
    }
    let now = (app.clock)();
    let states = StateService::new(app.store.clone(), app.secret.to_vec(), app.cookie.secure)?;
    let consumed = match states
        .consume(state, super::value(&parts.headers, "cookie"), now)
        .await
    {
        Ok(consumed) => consumed,
        Err(error) => {
            if error.details.get("clearCookie") == Some(&json!(true)) {
                set_cookie(pending, &states.clear_cookie())?;
            }
            let destination = error
                .details
                .get("errorURL")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .unwrap_or(&default_error);
            let code = if error.code == "state_security_mismatch" {
                "state_mismatch"
            } else {
                &error.code
            };
            return with_error(destination, code, None);
        }
    };
    set_cookie(pending, &consumed.clear_cookie)?;
    let data = consumed.data;
    let error_url = data["errorURL"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or(&default_error);
    let error = string(&query, "error");
    if !error.is_empty() {
        return with_error(error_url, error, Some(string(&query, "error_description")));
    }
    let code = string(&query, "code");
    if code.is_empty() {
        return with_error(error_url, "no_code", None);
    }
    let Some(google) = app.google.as_ref().filter(|_| provider == "google") else {
        return with_error(error_url, "oauth_provider_not_found", None);
    };
    let Ok(tokens) = google
        .exchange_code(
            code,
            text(&data, "codeVerifier")?,
            &format!("{base}/callback/google"),
            now,
        )
        .await
    else {
        return with_error(error_url, "invalid_code", None);
    };
    finish(app, &data, &tokens, &parts.headers, pending, error_url, now).await
}
async fn finish<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    data: &Value,
    tokens: &Value,
    headers: &HeaderMap,
    pending: &mut HeaderMap,
    error_url: &str,
    now: DateTime<Utc>,
) -> BlueprintResult<String> {
    // The code-exchange response comes from the trusted Google token endpoint.
    // Its configured getUserInfo decodes the profile without a second JWKS fetch.
    if !crate::keyring::verify::truthy(&tokens["idToken"]) {
        return with_error(error_url, "unable_to_get_user_info", None);
    }
    let token = tokens["idToken"]
        .as_str()
        .ok_or_else(|| BlueprintError::new("HTTP_INTERNAL_EMPTY", ""))?;
    let profile = crate::oauth::google_tokens::decode_profile(token)
        .ok_or_else(|| BlueprintError::new("HTTP_INTERNAL_EMPTY", ""))?;
    if crate::oauth::google_accounts::account_subject(&profile).is_err() {
        return with_error(error_url, "unable_to_get_user_info", None);
    }
    let callback = text(data, "callbackURL")?;
    if callback.is_empty() {
        return with_error(error_url, "no_callback_url", None);
    }
    let accounts = GoogleAccounts::new(app.store.clone());
    if let Some(link) = data.get("link") {
        match accounts.link_verified(&profile, tokens, link, now).await {
            Ok(()) => return Ok(callback.to_owned()),
            Err(error) => return with_error(error_url, &error.code, None),
        }
    }
    let (ip, agent) = super::request::client_metadata(headers);
    match accounts
        .sign_in_verified(&profile, tokens, &ip, &agent, now)
        .await
    {
        Ok(grant) => {
            set_cookie(
                pending,
                &app.cookie
                    .serialize(text(&grant.auth.session, "token")?, &app.secret, true),
            )?;
            Ok(if grant.registered {
                data["newUserURL"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .unwrap_or(callback)
            } else {
                callback
            }
            .to_owned())
        }
        Err(error) => {
            let code = match error.code.as_str() {
                "USER_EMAIL_NOT_FOUND" => "email_not_found".to_owned(),
                "FAILED_TO_GET_USER_INFO" => "unable_to_get_user_info".to_owned(),
                "OAUTH_LINK_ERROR" => error.message.replace(' ', "_"),
                _ => return Err(error),
            };
            with_error(error_url, &code, None)
        }
    }
}
fn validate(value: Option<&Value>, prefix: &str) -> BlueprintResult<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let mut errors = Vec::new();
    if value.is_object() {
        for field in FIELDS {
            if let Some(value) = value.get(*field).filter(|value| !value.is_string()) {
                errors.push(format!(
                    "[{prefix}.{field}] Invalid input: expected string, received {}",
                    super::request::received(Some(value))
                ));
            }
        }
    } else {
        errors.push(format!(
            "[{prefix}] Invalid input: expected object, received {}",
            super::request::received(Some(value))
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(BlueprintError::new("VALIDATION_ERROR", errors.join("; ")))
    }
}
fn string<'a>(query: &'a HashMap<String, Value>, field: &str) -> &'a str {
    query.get(field).and_then(Value::as_str).unwrap_or("")
}
pub(super) fn auth_base<S>(app: &AuthHttp<S>) -> BlueprintResult<String> {
    let mut base = url::Url::parse(&app.base_url).map_err(|_| internal())?;
    if base.path().trim_matches('/').is_empty() {
        base.set_path("/api/auth");
    }
    Ok(base.to_string().trim_end_matches('/').to_owned())
}
fn with_error(destination: &str, code: &str, description: Option<&str>) -> BlueprintResult<String> {
    let relative = destination.starts_with('/');
    let mut url = if relative {
        url::Url::parse("https://better-auth.invalid").and_then(|base| base.join(destination))
    } else {
        url::Url::parse(destination)
    }
    .map_err(|_| internal())?;
    let mut params = url::form_urlencoded::Serializer::new(String::new());
    params.append_pair("error", code);
    if let Some(description) = description.filter(|value| !value.is_empty()) {
        params.append_pair("error_description", description);
    }
    let old = url.query().unwrap_or("");
    let query = if old.is_empty() {
        params.finish()
    } else {
        format!(
            "{old}{}{}",
            if old.ends_with('&') { "" } else { "&" },
            params.finish()
        )
    };
    url.set_query(Some(&query));
    Ok(if relative {
        url.as_str()
            .strip_prefix("https://better-auth.invalid")
            .ok_or_else(internal)?
            .to_owned()
    } else {
        url.into()
    })
}
fn redirect(url: &str) -> BlueprintResult<Response> {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::FOUND;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(url).map_err(|_| internal())?,
    );
    Ok(response)
}
