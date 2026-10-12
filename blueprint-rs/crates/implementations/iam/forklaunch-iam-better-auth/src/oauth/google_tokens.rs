//! Google token-response normalization at the trusted server HTTP boundary.
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};

pub(crate) fn normalize(data: &Value, now: DateTime<Utc>) -> Option<Value> {
    if data.is_null() {
        return None;
    }
    let mut tokens = json!({});
    for (source, target) in [
        ("access_token", "accessToken"),
        ("refresh_token", "refreshToken"),
        ("id_token", "idToken"),
    ] {
        if let Some(value) = data.get(source) {
            tokens[target] = value.clone();
        }
    }
    for (source, target) in [
        ("expires_in", "accessTokenExpiresAt"),
        ("refresh_token_expires_in", "refreshTokenExpiresAt"),
    ] {
        if let Some(seconds) = data
            .get(source)
            .filter(|value| crate::keyring::verify::truthy(value))
        {
            let millis = json!(now.timestamp_millis()).as_f64()? + number(seconds) * 1000.0;
            // JS Date TimeClip truncates towards zero; invalid dates serialize as null.
            tokens[target] = if millis.is_finite() && millis.abs() <= 8_640_000_000_000_000.0 {
                #[allow(clippy::cast_possible_truncation)]
                DateTime::<Utc>::from_timestamp_millis(millis.trunc() as i64)
                    .map_or(Value::Null, |date| {
                        json!(date.to_rfc3339_opts(SecondsFormat::Millis, true))
                    })
            } else {
                Value::Null
            };
        }
    }
    let scopes = match &data["scope"] {
        Value::String(scope) => scope
            .split(js_space)
            .filter(|scope| !scope.is_empty())
            .map(str::to_owned)
            .collect(),
        Value::Array(scopes) => scopes
            .iter()
            .filter_map(Value::as_str)
            .map(|scope| scope.trim_matches(js_space))
            .filter(|scope| !scope.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>(),
        _ => vec![],
    };
    tokens["scope"] = json!(scopes.join(","));
    tokens["scopes"] = json!(scopes);
    Some(tokens)
}
pub(crate) fn decode_profile(token: &str) -> Option<Value> {
    let parts = token.split('.').collect::<Vec<_>>();
    if parts.len() != 3 || parts[1].is_empty() {
        return None;
    }
    let bytes = crate::keyring::verify::decode_jwk(parts[1])?;
    let payload = crate::ecmascript::parse(std::str::from_utf8(&bytes).ok()?).ok()?;
    payload.is_object().then_some(payload)
}
fn js_space(character: char) -> bool {
    character == '\u{feff}' || character.is_whitespace() && character != '\u{85}'
}
fn number(value: &Value) -> f64 {
    match value {
        Value::Null => return 0.0,
        Value::Bool(value) => return if *value { 1.0 } else { 0.0 },
        Value::Number(value) => return value.as_f64().unwrap_or(f64::NAN),
        _ => {}
    }
    let Some(value) = crate::ecmascript::string(Some(value)) else {
        return f64::NAN;
    };
    let value = value.trim_matches(js_space);
    if value.is_empty() {
        return 0.0;
    }
    if matches!(value, "Infinity" | "+Infinity") {
        return f64::INFINITY;
    }
    if value == "-Infinity" {
        return f64::NEG_INFINITY;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0b", 2),
        ("0B", 2),
        ("0o", 8),
        ("0O", 8),
    ] {
        if let Some(digits) = value.strip_prefix(prefix) {
            if digits.is_empty() {
                return f64::NAN;
            }
            return digits
                .chars()
                .try_fold(0.0, |number, c| {
                    c.to_digit(radix)
                        .map(|digit| number * f64::from(radix) + f64::from(digit))
                })
                .unwrap_or(f64::NAN);
        }
    }
    if value
        .chars()
        .any(|c| !matches!(c, '0'..='9' | '+' | '-' | '.' | 'e' | 'E'))
    {
        return f64::NAN;
    }
    value.parse().unwrap_or(f64::NAN)
}
