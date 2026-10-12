use serde_json::Value;
pub(super) fn string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::String(value)) => value.clone(),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                if value.is_null() {
                    String::new()
                } else {
                    string(Some(value))
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
        Some(value) => value.to_string(),
    }
}
pub(super) fn number(value: Option<&Value>) -> f64 {
    if let Some(Value::Number(value)) = value {
        return value.as_f64().unwrap_or(f64::NAN);
    }
    match value {
        None => return f64::NAN,
        Some(Value::Null) => return 0.0,
        Some(Value::Bool(value)) => return if *value { 1.0 } else { 0.0 },
        _ => {}
    }
    let value = string(value);
    let value = super::super::trim(&value);
    if value.is_empty() {
        return 0.0;
    }
    if matches!(value, "Infinity" | "+Infinity") {
        return f64::INFINITY;
    }
    if value == "-Infinity" {
        return f64::NEG_INFINITY;
    }
    for (prefixes, radix) in [(["0x", "0X"], 16), (["0b", "0B"], 2), (["0o", "0O"], 8)] {
        if prefixes.iter().any(|prefix| value.starts_with(prefix)) {
            let digits = &value[2..];
            if digits.is_empty() {
                return f64::NAN;
            }
            return digits
                .chars()
                .try_fold(0.0, |number, character| {
                    character
                        .to_digit(radix)
                        .map(|digit| number * f64::from(radix) + f64::from(digit))
                })
                .unwrap_or(f64::NAN);
        }
    }
    // Rust also accepts inf/NaN spellings that ECMAScript Number rejects.
    if value
        .chars()
        .any(|character| !matches!(character, '0'..='9' | '+' | '-' | '.' | 'e' | 'E'))
    {
        return f64::NAN;
    }
    value.parse().unwrap_or(f64::NAN)
}
pub(super) fn index(value: f64, length: usize) -> usize {
    // Store lists are bounded; converting through decimal avoids unchecked
    // machine-sized floating-point casts at the pagination boundary.
    let bound = length.to_string().parse::<f64>().unwrap_or(f64::INFINITY);
    let value = if value.is_nan() {
        0.0
    } else if value < 0.0 {
        (bound + value.trunc()).max(0.0)
    } else {
        value.trunc().min(bound)
    };
    value.to_string().parse().unwrap_or(length).min(length)
}
