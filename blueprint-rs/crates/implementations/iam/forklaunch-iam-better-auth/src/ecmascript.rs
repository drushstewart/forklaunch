//! String coercion at configured Zod server/HTTP boundaries.
use serde_json::Value;
mod json;
pub(crate) use json::parse;
pub(crate) fn string(value: Option<&Value>) -> Option<String> {
    Some(match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::String(value)) => value.clone(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Number(value)) => ryu_js::Buffer::new().format(number(value)).to_owned(),
        Some(Value::Object(value)) => {
            // A JSON own toString property shadows the inherited callable. The
            // default valueOf still returns an object, so ToPrimitive throws.
            if value.contains_key("toString") {
                return None;
            }
            "[object Object]".into()
        }
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                if value.is_null() {
                    Some(String::new())
                } else {
                    string(Some(value))
                }
            })
            .collect::<Option<Vec<_>>>()?
            .join(","),
    })
}

/// JSON.parse numbers use IEEE doubles, including overflow to infinity.
fn number(value: &serde_json::Number) -> f64 {
    value
        .as_f64()
        .unwrap_or_else(|| value.to_string().parse().expect("valid JSON number"))
}

/// JSON.stringify orders array-index keys first and rounds all numbers to doubles.
pub(crate) fn stringify(value: &Value) -> String {
    match value {
        Value::Number(value) => {
            let value = number(value);
            if value.is_finite() {
                ryu_js::Buffer::new().format(value).to_owned()
            } else {
                "null".into()
            }
        }
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(stringify).collect::<Vec<_>>().join(",")
        ),
        Value::Object(values) => {
            let index = |key: &str| {
                key.parse::<u32>()
                    .ok()
                    .filter(|i| *i != u32::MAX && i.to_string() == key)
            };
            let mut indexed = values
                .iter()
                .filter_map(|(k, v)| index(k).map(|i| (i, k, v)))
                .collect::<Vec<_>>();
            indexed.sort_by_key(|(i, _, _)| *i);
            let fields = indexed
                .into_iter()
                .map(|(_, k, v)| (k, v))
                .chain(values.iter().filter(|(k, _)| index(k).is_none()));
            format!(
                "{{{}}}",
                fields
                    .map(|(k, v)| format!(
                        "{}:{}",
                        serde_json::to_string(k).expect("JSON string"),
                        stringify(v)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        _ => serde_json::to_string(value).expect("JSON primitive"),
    }
}

// The configured update adapter's strict better-json parser rejects these keys
// recursively, after persistence. Creation and raw reads use different parsers.
pub(crate) fn unsafe_metadata(value: &Value) -> bool {
    match value {
        Value::Object(values) => values.iter().any(|(key, value)| {
            matches!(key.as_str(), "__proto__" | "constructor") || unsafe_metadata(value)
        }),
        Value::Array(values) => values.iter().any(unsafe_metadata),
        _ => false,
    }
}

/// The update adapter revives ISO strings through Date.UTC, including its
/// millisecond overflow and 0..99 year rules, before serializing the response.
pub(crate) fn revive_dates(value: &mut Value) {
    match value {
        Value::String(text) => {
            if let Some(date) = iso_date(text) {
                *text = date;
            }
        }
        Value::Array(values) => values.iter_mut().for_each(revive_dates),
        Value::Object(values) => values.values_mut().for_each(revive_dates),
        _ => {}
    }
}
fn iso_date(text: &str) -> Option<String> {
    use chrono::{Duration, NaiveDate, SecondsFormat};
    static ISO: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern = ISO.get_or_init(|| regex::Regex::new(r"^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,7}))?(?:Z|([+-])(\d{2}):(\d{2}))$").expect("ISO date pattern"));
    let parts = pattern.captures(text)?;
    let n = |i| {
        parts
            .get(i)
            .and_then(|part| part.as_str().parse::<i64>().ok())
    };
    let mut year = n(1)?;
    if year < 100 {
        year += 1900;
    }
    let month = n(2)? - 1;
    year += month.div_euclid(12);
    let month = month.rem_euclid(12) + 1;
    let millis = parts.get(7).map_or(0, |part| {
        format!("{:0<3}", part.as_str())
            .parse::<i64>()
            .expect("millisecond digits")
    });
    let offset = if parts.get(8).is_some_and(|sign| sign.as_str() == "+") {
        1
    } else {
        -1
    };
    let seconds = (n(3)? - 1) * 86400 + n(4)? * 3600 + n(5)? * 60 + n(6)?
        - offset * (n(9).unwrap_or(0) * 3600 + n(10).unwrap_or(0) * 60);
    let date = NaiveDate::from_ymd_opt(year.try_into().ok()?, month.try_into().ok()?, 1)?
        .and_hms_opt(0, 0, 0)?
        .checked_add_signed(Duration::seconds(seconds))?
        .checked_add_signed(Duration::milliseconds(millis))?;
    Some(date.and_utc().to_rfc3339_opts(SecondsFormat::Millis, true))
}
