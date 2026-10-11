use super::{AuthHttp, value};
use crate::{origins::RequestOrigins, store::AuthStore};
use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, request::Parts},
};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};
use std::collections::HashMap;

pub(super) async fn parse_body(
    path: &str,
    mutation: bool,
    body: Body,
    headers: &HeaderMap,
) -> BlueprintResult<Option<Value>> {
    if !mutation {
        return Ok(None);
    }
    let bytes = to_bytes(body, 1024 * 1024)
        .await
        .map_err(|_| BlueprintError::new("PAYLOAD_TOO_LARGE", "Request body is too large"))?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let mime = value(headers, "content-type")
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    let form = matches!(path, "/sign-in/email" | "/sign-up/email");
    let allowed = if form {
        "application/x-www-form-urlencoded, application/json"
    } else {
        "application/json"
    };
    match mime {
        "application/json" => serde_json::from_slice::<Value>(&bytes)
            .map(Some)
            .map_err(|_| BlueprintError::new("BAD_REQUEST", "Invalid JSON in request body")),
        "application/x-www-form-urlencoded" if form => Ok(Some(Value::Object(
            url::form_urlencoded::parse(&bytes)
                .map(|(key, value)| (key.into_owned(), json!(value)))
                .collect(),
        ))),
        _ => Err(BlueprintError::new(
            "UNSUPPORTED_MEDIA_TYPE",
            format!("Content-Type \"{mime}\" is not allowed. Allowed types: {allowed}"),
        )),
    }
}
pub(super) fn guard<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    parts: &Parts,
    payload: &Value,
    query: &HashMap<String, String>,
) -> BlueprintResult<()> {
    let protocol = if app.cookie.secure { "https" } else { "http" };
    let request_origin = format!(
        "{protocol}://{}",
        value(&parts.headers, "host").unwrap_or("")
    );
    let origins = RequestOrigins {
        cookie_present: parts.headers.contains_key("cookie"),
        origin: value(&parts.headers, "origin"),
        referer: value(&parts.headers, "referer"),
        fetch_site: value(&parts.headers, "sec-fetch-site"),
        fetch_mode: value(&parts.headers, "sec-fetch-mode"),
        fetch_dest: value(&parts.headers, "sec-fetch-dest"),
        request_origin: &request_origin,
    };
    let mut callbacks = payload.clone();
    let fallback = match &callbacks["callbackURL"] {
        Value::Null | Value::Bool(false) => true,
        Value::Number(value) => value.as_f64() == Some(0.0),
        Value::String(value) => value.is_empty(),
        _ => false,
    };
    if fallback && let Some(callback) = query.get("callbackURL") {
        if !callbacks.is_object() {
            callbacks = json!({});
        }
        callbacks["callbackURL"] = json!(callback);
    }
    app.policy.mutation(&origins, false, &callbacks)
}

pub(super) fn session_credentials<S: AuthStore>(
    app: &AuthHttp<S>,
    headers: &HeaderMap,
) -> (String, bool) {
    use crate::cookies::{cookie_value, verify_cookie_value};
    let cookie_header = value(headers, "cookie").unwrap_or("");
    let signed = |name| {
        cookie_value(cookie_header, name)
            .and_then(|value| verify_cookie_value(&value, &app.secret).map(str::to_owned))
    };
    (
        signed(app.cookie.name()).unwrap_or_default(),
        signed(app.cookie.dont_remember_name()).is_some(),
    )
}

pub(super) fn validate_body(path: &str, payload: Option<&Value>) -> BlueprintResult<()> {
    let (optional, fields): (bool, &[(&str, &str, bool)]) = match path {
        "/revoke-session" => (false, &[("token", "string", true)]),
        "/sign-out" => (
            true,
            &[
                ("callbackURL", "string", false),
                ("disableRedirect", "boolean", false),
                ("state", "string", false),
            ],
        ),
        _ => return Ok(()),
    };
    if optional && payload.is_none() {
        return Ok(());
    }
    if !payload.is_some_and(Value::is_object) {
        return Err(BlueprintError::new(
            "VALIDATION_ERROR",
            format!(
                "[body] Invalid input: expected object, received {}",
                received(payload)
            ),
        ));
    }
    let mut errors = Vec::new();
    for (field, expected, required) in fields {
        let value = payload.and_then(|body| body.get(*field));
        let valid = match *expected {
            "boolean" => value.is_some_and(Value::is_boolean),
            _ => value.is_some_and(Value::is_string),
        };
        if !valid && (*required || value.is_some()) {
            errors.push(format!(
                "[body.{field}] Invalid input: expected {expected}, received {}",
                received(value)
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(BlueprintError::new("VALIDATION_ERROR", errors.join("; ")))
    }
}
fn received(value: Option<&Value>) -> &'static str {
    match value {
        None => "undefined",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
    }
}
