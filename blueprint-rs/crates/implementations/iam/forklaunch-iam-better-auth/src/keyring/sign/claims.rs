use super::{BlueprintResult, Value, duration, error, json};
fn nullish<'a>(value: Option<&'a Value>, fallback: &'a Value) -> &'a Value {
    value.filter(|value| !value.is_null()).unwrap_or(fallback)
}
pub(super) enum Expiry {
    Claim(Value),
    InvalidNumber,
}
pub(super) fn expiry(payload: &Value, options: &Value, now: f64) -> BlueprintResult<Expiry> {
    let default = json!("15m");
    let expiration = nullish(options["jwt"].get("expirationTime"), &default);
    let default_exp = if expiration.is_number() {
        Expiry::Claim(expiration.clone())
    } else {
        let seconds = duration::parse(expiration, true)?;
        let clock = json!(now);
        let iat = nullish(payload.get("iat"), &clock);
        let primitive = match iat {
            Value::String(_) | Value::Array(_) | Value::Object(_) => Some(
                crate::ecmascript::string(Some(iat))
                    .ok_or_else(|| error("Cannot convert object to primitive value"))?,
            ),
            _ => None,
        };
        if let Some(text) = primitive {
            Expiry::Claim(json!(format!(
                "{text}{}",
                ryu_js::Buffer::new().format(seconds)
            )))
        } else {
            let base = match iat {
                Value::Bool(value) => {
                    if *value {
                        1.0
                    } else {
                        0.0
                    }
                }
                Value::Number(value) => number(value),
                _ => 0.0,
            };
            if (base + seconds).is_finite() {
                Expiry::Claim(json!(base + seconds))
            } else {
                Expiry::InvalidNumber
            }
        }
    };
    Ok(payload
        .get("exp")
        .filter(|value| !value.is_null())
        .map_or(default_exp, |value| Expiry::Claim(value.clone())))
}
pub(super) fn build(
    payload: &Value,
    options: &Value,
    expiry: Expiry,
    base: &str,
    now: f64,
) -> BlueprintResult<Value> {
    let mut result = payload.clone();
    if let Some(record) = result.as_object_mut() {
        record.shift_remove("__proto__");
    }
    result["exp"] = match expiry {
        Expiry::Claim(value) => numeric_date(&value, now, "setExpirationTime")?,
        Expiry::InvalidNumber => return Err(error("Invalid setExpirationTime input")),
    };
    let base = json!(base);
    for claim in ["iss", "aud"] {
        let option = if claim == "iss" { "issuer" } else { "audience" };
        let value = nullish(
            payload.get(claim),
            nullish(options["jwt"].get(option), &base),
        );
        let valid = if claim == "iss" {
            value.is_string()
        } else {
            value.is_string()
                || value
                    .as_array()
                    .is_some_and(|items| items.iter().all(Value::is_string))
        };
        if !valid {
            return Err(error(if claim == "iss" {
                "\"iss\" claim must be a string"
            } else {
                "\"aud\" claim must be a string or an array of strings"
            }));
        }
        result[claim] = value.clone();
    }
    for (claim, label) in [
        ("iat", Some("setIssuedAt")),
        ("sub", None),
        ("nbf", Some("setNotBefore")),
        ("jti", None),
    ] {
        if let Some(value) = payload.get(claim).filter(|value| truthy(value)) {
            if let Some(label) = label {
                result[claim] = numeric_date(value, now, label)?;
            } else if !value.is_string() {
                return Err(error(format!("\"{claim}\" claim must be a string")));
            }
        }
    }
    Ok(result)
}
fn number(value: &serde_json::Number) -> f64 {
    value
        .as_f64()
        .unwrap_or_else(|| value.to_string().parse().expect("JSON number"))
}
fn numeric_date(value: &Value, now: f64, label: &str) -> BlueprintResult<Value> {
    let number = if let Value::Number(value) = value {
        number(value)
    } else {
        now + duration::parse(value, false)?
    };
    if !number.is_finite() {
        return Err(error(format!("Invalid {label} input")));
    }
    Ok(json!(number))
}
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::String(value) => !value.is_empty(),
        Value::Number(value) => {
            let n = number(value);
            n != 0.0 && !n.is_nan()
        }
        Value::Array(_) | Value::Object(_) => true,
    }
}
