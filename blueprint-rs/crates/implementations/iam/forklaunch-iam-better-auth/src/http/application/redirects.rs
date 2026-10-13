//! Configured popup helpers. Exchange through the existing trusted auth router,
//! avoiding an outbound request whose authority could depend on client headers.
use super::super::{AuthHttp, request};
use crate::{cookies::SessionCookie, origins::OriginPolicy, store::AuthStore};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};
use tower::ServiceExt;
use url::Url;
mod accept;

pub(super) struct Settings {
    base: String,
    auth_base: String,
    auth_path: String,
    policy: Arc<OriginPolicy>,
    cookie: SessionCookie,
}
#[derive(Clone)]
struct Redirects {
    settings: Arc<Settings>,
    raw: Router,
}
impl Settings {
    pub(super) fn new<S: AuthStore + 'static>(auth: &AuthHttp<S>) -> Self {
        Self {
            base: auth.base_url.clone(),
            auth_base: auth.auth_base_url().trim_end_matches('/').to_owned(),
            auth_path: auth.auth_base_path(),
            policy: auth.policy.clone(),
            cookie: auth.cookie,
        }
    }
    pub(super) fn mount(self, raw: Router) -> Router {
        Router::new()
            .route("/test-callback", get(exchange))
            .route("/oauth-redirect", get(oauth))
            .with_state(Redirects {
                settings: Arc::new(self),
                raw: raw.clone(),
            })
            .merge(raw)
    }
    fn callback(&self, value: &str) -> Option<Url> {
        let callback = Url::parse(&self.base)
            .ok()?
            .join("/")
            .ok()?
            .join(value)
            .ok()?;
        (matches!(callback.scheme(), "http" | "https")
            && callback.username().is_empty()
            && callback.password().is_none()
            && self.policy.trusted(callback.as_str(), false))
        .then_some(callback)
    }
}
fn query(request: &Request) -> HashMap<String, Value> {
    request::parse_query(request.uri().query().unwrap_or(""))
}
fn string(query: &HashMap<String, Value>, name: &str) -> String {
    query
        .get(name)
        .filter(|value| crate::keyring::verify::truthy(value))
        .and_then(|value| crate::ecmascript::string(Some(value)))
        .unwrap_or_default()
}
fn script_json(value: &Value) -> String {
    crate::ecmascript::stringify(value)
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}
async fn oauth(State(state): State<Redirects>, request: Request) -> Response {
    let query = query(&request);
    let callback = string(&query, "callbackURL");
    let Some(callback) = state
        .settings
        .callback(&callback)
        .filter(|_| !callback.is_empty())
    else {
        return bad("Invalid callbackURL");
    };
    let sso = query.get("endpoint").and_then(Value::as_str) == Some("sso");
    let endpoint = if sso {
        format!("{}/sign-in/sso", state.settings.auth_path)
    } else {
        format!("{}/sign-in/social", state.settings.auth_path)
    };
    let intermediate = format!(
        "{}/test-callback?callbackUrl={}",
        state.settings.auth_base,
        encode_component(callback.as_str())
    );
    let body = if sso {
        json!({"organizationId":string(&query,"organizationId"),"callbackURL":intermediate})
    } else {
        json!({"provider":string(&query,"provider"),"callbackURL":intermediate})
    };
    let endpoint = script_json(&json!(endpoint));
    let body = script_json(&json!(crate::ecmascript::stringify(&body)));
    Html(format!("<!DOCTYPE html><html><body><p>Redirecting…</p><script>\n    fetch({endpoint}, {{\n      method: 'POST',\n      headers: {{ 'Content-Type': 'application/json' }},\n      body: {body},\n      credentials: 'include',\n    }})\n    .then(function(r) {{ return r.json(); }})\n    .then(function(data) {{\n      if (data.url) window.location.href = data.url;\n      else document.body.innerText = 'Error: ' + JSON.stringify(data);\n    }})\n    .catch(function(e) {{ document.body.innerText = 'Error: ' + e.message; }});\n  </script></body></html>")).into_response()
}
async fn read(raw: Router, path: &str, cookie: Option<HeaderValue>) -> Result<Value, ()> {
    let mut request = Request::builder()
        .uri(path)
        .body(Body::empty())
        .map_err(|_| ())?;
    request.headers_mut().insert(
        header::COOKIE,
        cookie.unwrap_or_else(|| HeaderValue::from_static("")),
    );
    let response = raw.oneshot(request).await.map_err(|_| ())?;
    if !response.status().is_success() {
        return Ok(Value::Null);
    }
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .map_err(|_| ())?;
    crate::ecmascript::parse(std::str::from_utf8(&body).map_err(|_| ())?).map_err(|_| ())
}
async fn exchange(State(state): State<Redirects>, request: Request) -> Response {
    let query = query(&request);
    let callback = string(&query, "callbackUrl");
    if callback.is_empty() {
        return bad("Missing callbackUrl");
    }
    let Some(mut callback) = state.settings.callback(&callback) else {
        return bad("Invalid callbackUrl");
    };
    let cookie = request.headers().get(header::COOKIE).cloned();
    let (token, session) = tokio::join!(
        read(state.raw.clone(), "/token", cookie.clone()),
        read(state.raw, "/get-session", cookie)
    );
    let (Ok(token), Ok(session)) = (token, session) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html("Failed to exchange session: Invalid authentication response"),
        )
            .into_response();
    };
    let mut fragment = url::form_urlencoded::Serializer::new(String::new());
    fragment
        .append_pair("token", token["token"].as_str().unwrap_or(""))
        .append_pair(
            "sessionToken",
            session["session"]["token"].as_str().unwrap_or(""),
        )
        .append_pair(
            "email",
            session["user"]["email"]
                .as_str()
                .filter(|email| !email.is_empty())
                .unwrap_or("unknown"),
        );
    callback.set_fragment(Some(&fragment.finish()));
    let mut response = redirect(callback.as_str(), request.headers());
    let clear = format!(
        "{}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax{}",
        state.settings.cookie.name(),
        if state.settings.cookie.secure {
            "; Secure"
        } else {
            ""
        }
    );
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&clear).expect("fixed cookie"),
    );
    response
}
fn bad(message: &'static str) -> Response {
    (StatusCode::BAD_REQUEST, Html(message)).into_response()
}
pub(crate) fn redirect(location: &str, headers: &HeaderMap) -> Response {
    let media = accept::preferred(headers);
    let body = match media {
        Some("text/html") => format!("<p>Found. Redirecting to {}</p>", escape_html(location)),
        Some(_) => format!("Found. Redirecting to {location}"),
        None => String::new(),
    };
    let length = body.len();
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = StatusCode::FOUND;
    if let Some(media) = media {
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(if media == "text/html" {
                "text/html; charset=utf-8"
            } else {
                "text/plain; charset=utf-8"
            }),
        );
    }
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string()).expect("body length"),
    );
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Accept"));
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(location).expect("serialized URL"),
    );
    response
}
fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn encode_component(value: &str) -> String {
    const COMPONENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'!')
        .remove(b'~')
        .remove(b'*')
        .remove(b'\'')
        .remove(b'(')
        .remove(b')');
    percent_encoding::utf8_percent_encode(value, COMPONENT).to_string()
}
