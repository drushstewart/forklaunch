//! Configured public documentation and error responses. The default page is
//! adapted from Better Auth under assets/BETTER-AUTH-LICENSE.md.
use super::{AuthHttp, AuthStore};
use axum::{
    extract::{Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) async fn error<S: AuthStore + 'static>(
    State(app): State<Arc<AuthHttp<S>>>,
    request: Request,
) -> Response {
    // URLSearchParams.get selects the first value, including an empty value.
    let pairs: Vec<_> = url::form_urlencoded::parse(request.uri().query().unwrap_or("").as_bytes())
        .into_owned()
        .collect();
    let first = |key| {
        pairs
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    };
    let code = first("error")
        .filter(|value| !value.is_empty())
        .unwrap_or("UNKNOWN");
    let code = if code
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"'_-".contains(&byte))
    {
        code
    } else {
        "UNKNOWN"
    };
    let description = first("error_description").filter(|value| !value.is_empty());
    if app.production {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("error", code);
        if let Some(description) = description {
            query.append_pair("error_description", description);
        }
        return (
            StatusCode::FOUND,
            [(header::LOCATION, format!("/?{}", query.finish()))],
        )
            .into_response();
    }
    let encoded = encode_component(code);
    let description = description.map_or_else(
        || format!("We encountered an unexpected error. Please try again or return to the home page. If you're a developer, you can find <a href='https://better-auth.com/docs/reference/errors/{encoded}' target='_blank' rel=\"noopener noreferrer\" style='color: var(--foreground); text-decoration: underline;'>more information about the error</a>."),
        sanitize,
    );
    // Substitute each original marker once; never interpret inserted user text.
    let question = encode_component(&format!("What does the error code {code} mean?"));
    let display = sanitize(code);
    let replacements = [
        ("FL_ERROR_QUESTION", question.as_str()),
        ("FL_ERROR_CODE", encoded.as_str()),
        ("FL_ERROR_DISPLAY", display.as_str()),
        ("FL_ERROR_DESCRIPTION", description.as_str()),
    ];
    let mut rest = include_str!("assets/error.html");
    let mut page = String::with_capacity(rest.len() + description.len());
    while let Some((index, marker, value)) = replacements
        .iter()
        .filter_map(|(marker, value)| rest.find(marker).map(|index| (index, *marker, *value)))
        .min_by_key(|(index, _, _)| *index)
    {
        page.push_str(&rest[..index]);
        page.push_str(value);
        rest = &rest[index + marker.len()..];
    }
    page.push_str(rest);
    ([(header::CONTENT_TYPE, "text/html")], page).into_response()
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

fn sanitize(value: &str) -> String {
    // Upstream preserves already encoded named and numeric entities.
    let escaped = value
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;");
    let mut result = String::with_capacity(escaped.len());
    for (index, character) in escaped.char_indices() {
        if character == '&' && !entity(&escaped[index + 1..]) {
            result.push_str("&amp;");
        } else {
            result.push(character);
        }
    }
    result
}
fn entity(value: &str) -> bool {
    if ["amp;", "lt;", "gt;", "quot;", "#39;"]
        .iter()
        .any(|prefix| value.starts_with(prefix))
    {
        return true;
    }
    let (digits, hex) = if let Some(value) = value.strip_prefix("#x") {
        (value, true)
    } else if let Some(value) = value.strip_prefix('#') {
        (value, false)
    } else {
        return false;
    };
    let length = digits
        .bytes()
        .take_while(|byte| {
            if hex {
                byte.is_ascii_hexdigit()
            } else {
                byte.is_ascii_digit()
            }
        })
        .count();
    length > 0 && digits.as_bytes().get(length) == Some(&b';')
}

/// The configured TypeScript plugin explicitly disables its reference UI.
pub(super) async fn reference() -> Response {
    (
        StatusCode::NOT_FOUND,
        [(header::CONTENT_TYPE, "application/json")],
        "",
    )
        .into_response()
}

pub(super) async fn schema<S: AuthStore + 'static>(
    State(app): State<Arc<AuthHttp<S>>>,
) -> Response {
    let schema = openapi_schema(&app.base_url);
    (
        [(header::CONTENT_TYPE, "application/json")],
        crate::ecmascript::stringify(&schema),
    )
        .into_response()
}

/// Fixed preset schema shared by the raw endpoint and the application's docs.
pub(super) fn openapi_schema(base_url: &str) -> Value {
    let mut schema: Value = serde_json::from_str(include_str!("assets/openapi.json"))
        .expect("embedded configured auth contract is valid JSON");
    schema["servers"] = json!([{ "url": format!("{}/api/auth", base_url.trim_end_matches('/')) }]);
    schema
}
