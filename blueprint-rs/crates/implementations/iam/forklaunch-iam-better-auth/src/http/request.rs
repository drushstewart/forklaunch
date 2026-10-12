use super::{AuthHttp, value};
use crate::{origins::RequestOrigins, store::AuthStore};
use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, request::Parts},
};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};
use std::{collections::HashMap, fmt::Write};

pub(super) async fn parse_body(
    path: &str,
    mutation: bool,
    body: Body,
    headers: &HeaderMap,
) -> BlueprintResult<Option<Value>> {
    static JSON: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    if !mutation {
        return Ok(None);
    }
    let bytes = to_bytes(body, 1024 * 1024)
        .await
        .map_err(|_| BlueprintError::new("PAYLOAD_TOO_LARGE", "Request body is too large"))?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let mime = value(headers, "content-type").unwrap_or("");
    let normalized = mime.to_lowercase();
    let base = normalized.split(';').next().unwrap_or("").trim();
    let form = matches!(path, "/sign-in/email" | "/sign-up/email");
    let allowed = if form {
        "application/x-www-form-urlencoded, application/json"
    } else {
        "application/json"
    };
    if !(base.contains("application/json")
        || form && base.contains("application/x-www-form-urlencoded"))
    {
        let message = if mime.is_empty() {
            format!("Content-Type is required. Allowed types: {allowed}")
        } else {
            format!("Content-Type \"{mime}\" is not allowed. Allowed types: {allowed}")
        };
        return Err(BlueprintError::new("UNSUPPORTED_MEDIA_TYPE", message));
    }
    // Better Call accepts JSON prefixes after its allowed-media substring check.
    let json = JSON.get_or_init(|| {
        regex::Regex::new(r"^application/([a-z0-9.+-]*\+)?json")
            .expect("fixed JSON MIME expression")
    });
    if json.is_match(&normalized) {
        return serde_json::from_slice::<Value>(&bytes)
            .map(Some)
            .map_err(|_| BlueprintError::new("BAD_REQUEST", "Invalid JSON in request body"));
    }
    if normalized.contains("application/x-www-form-urlencoded") {
        return Ok(Some(Value::Object(
            url::form_urlencoded::parse(&bytes)
                .map(|(key, value)| (key.into_owned(), json!(value)))
                .collect(),
        )));
    }
    if normalized.contains("text/plain") {
        return Ok(Some(json!(String::from_utf8_lossy(&bytes))));
    }
    // The upstream fallback passes its ReadableStream object to validation.
    Ok(Some(json!({})))
}
pub(super) fn guard<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    parts: &Parts,
    payload: &Value,
    query: &HashMap<String, Value>,
    login: bool,
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
    let mut callbacks = json!({});
    for field in [
        "callbackURL",
        "redirectTo",
        "errorCallbackURL",
        "newUserCallbackURL",
    ] {
        if let Some(value) = payload.get(field) {
            callbacks[field] = value.clone();
        }
    }
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
    app.policy.mutation(&origins, login, &callbacks)
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
    let (optional, fields) = body_fields(path);
    if path == "/update-user" {
        if !payload.is_some_and(Value::is_object) {
            return Err(BlueprintError::new(
                "VALIDATION_ERROR",
                format!(
                    "[body] Invalid input: expected record, received {}",
                    received(payload)
                ),
            ));
        }
        return Ok(());
    }
    if fields.is_empty() {
        return Ok(());
    }
    if optional && payload.is_none() {
        return Ok(());
    }
    if !payload.is_some_and(Value::is_object) {
        let received = received(payload);
        let mut message = format!("[body] Invalid input: expected object, received {received}");
        if path == "/sign-up/email" {
            write!(
                message,
                "; [body] Invalid input: expected record, received {received}"
            )
            .expect("writing a String succeeds");
        }
        return Err(BlueprintError::new("VALIDATION_ERROR", message));
    }
    let mut errors = Vec::new();
    for (field, expected, required) in fields {
        let value = payload.and_then(|body| body.get(*field));
        let valid = match *expected {
            "boolean" => value.is_some_and(Value::is_boolean),
            _ => value.is_some_and(Value::is_string),
        };
        if valid
            && matches!(
                path,
                "/sign-up/email"
                    | "/request-password-reset"
                    | "/send-verification-email"
                    | "/change-email"
            )
        {
            if matches!(*field, "email" | "newEmail")
                && crate::accounts::validate_email(value.and_then(Value::as_str).unwrap_or(""))
                    .is_err()
            {
                errors.push(format!("[body.{field}] Invalid email address"));
            }
            if *field == "password"
                && matches!(path, "/sign-in/email" | "/sign-up/email")
                && value.and_then(Value::as_str) == Some("")
            {
                errors.push(
                    "[body.password] Too small: expected string to have >=1 characters".into(),
                );
            }
        }
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

fn body_fields(path: &str) -> (bool, &'static [(&'static str, &'static str, bool)]) {
    match path {
        "/sign-up/email" => (
            false,
            &[
                ("name", "string", true),
                ("email", "string", true),
                ("password", "string", true),
                ("image", "string", false),
                ("callbackURL", "string", false),
                ("rememberMe", "boolean", false),
            ],
        ),
        "/sign-in/email" => (
            false,
            &[
                ("email", "string", true),
                ("password", "string", true),
                ("callbackURL", "string", false),
                ("rememberMe", "boolean", false),
            ],
        ),
        "/request-password-reset" => (
            false,
            &[("email", "string", true), ("redirectTo", "string", false)],
        ),
        "/send-verification-email" => (
            false,
            &[("email", "string", true), ("callbackURL", "string", false)],
        ),
        "/change-email" => (
            false,
            &[
                ("newEmail", "string", true),
                ("callbackURL", "string", false),
            ],
        ),
        "/delete-user" => (
            false,
            &[
                ("callbackURL", "string", false),
                ("password", "string", false),
                ("token", "string", false),
            ],
        ),
        "/verify-password" => (false, &[("password", "string", true)]),
        "/change-password" => (
            false,
            &[
                ("newPassword", "string", true),
                ("currentPassword", "string", true),
                ("revokeOtherSessions", "boolean", false),
            ],
        ),
        "/unlink-account" => (false, &[("accountId", "string", true)]),
        "/revoke-session" => (false, &[("token", "string", true)]),
        "/sign-out" => (
            true,
            &[
                ("callbackURL", "string", false),
                ("disableRedirect", "boolean", false),
                ("state", "string", false),
            ],
        ),
        _ => (true, &[]),
    }
}

pub(super) fn client_metadata(headers: &HeaderMap) -> (String, String) {
    (
        client_ip(headers),
        value(headers, "user-agent").unwrap_or("").to_owned(),
    )
}
pub(super) fn client_ip(headers: &HeaderMap) -> String {
    use std::net::IpAddr;
    let mut addresses = value(headers, "x-forwarded-for")
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|ip| !ip.is_empty());
    let candidate = addresses.next();
    let ip = if addresses.next().is_some() {
        None
    } else {
        candidate.and_then(|ip| ip.parse::<IpAddr>().ok())
    };
    ip.map_or_else(fallback_ip, |ip| match ip {
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or_else(
            || {
                let segments = ip.segments();
                format!(
                    "{:04x}:{:04x}:{:04x}:{:04x}:0000:0000:0000:0000",
                    segments[0], segments[1], segments[2], segments[3]
                )
            },
            |ip| ip.to_string(),
        ),
    })
}

fn fallback_ip() -> String {
    if std::env::var("NODE_ENV").is_ok_and(|env| matches!(env.as_str(), "development" | "test")) {
        "127.0.0.1".into()
    } else {
        String::new()
    }
}

pub(super) fn parse_query(raw: &str) -> HashMap<String, Value> {
    let mut query = HashMap::new();
    for (key, value) in url::form_urlencoded::parse(raw.as_bytes()) {
        let value = json!(value);
        query
            .entry(key.into_owned())
            .and_modify(|current: &mut Value| {
                if let Some(values) = current.as_array_mut() {
                    values.push(value.clone());
                } else {
                    *current = json!([current.clone(), value]);
                }
            })
            .or_insert(value);
    }
    query
}
