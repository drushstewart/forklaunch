//! Fixture-owned rules for the canonical Zod query schema, not a general translator.
use forklaunch_blueprint_core::http::{FrameworkRequest, FrameworkResponse};
use forklaunch_validation::{ParseError, typescript_request_validation_error_response};

pub fn reject(request: &FrameworkRequest, raw: Option<&str>) -> Option<FrameworkResponse> {
    let fields: Vec<_> = form_urlencoded::parse(raw.unwrap_or_default().as_bytes()).collect();
    let mut errors = Vec::new();
    for name in ["limit", "cursor"] {
        let values: Vec<_> = fields
            .iter()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| value.as_ref())
            .collect();
        let message = match (name, values.as_slice()) {
            (_, []) | ("cursor", [_]) => None,
            ("cursor", _) => Some("Expected string, received array"),
            ("limit", [value]) => {
                let number = number(value);
                if number.is_nan() {
                    Some("Expected number, received nan")
                } else if !number.is_finite() || number.fract() != 0.0 {
                    Some("Expected integer, received float")
                } else {
                    None
                }
            }
            _ => Some("Expected number, received nan"),
        };
        if let Some(message) = message {
            errors.push(ParseError {
                path: vec!["query".into(), name.into()],
                message: message.into(),
            });
        }
    }
    (!errors.is_empty()).then(|| {
        typescript_request_validation_error_response(
            Some(&errors),
            Some(&request.context.correlation_id),
        )
    })
}

fn number(input: &str) -> f64 {
    let input = input.trim_matches(|c: char| {
        matches!(c,
            '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' |
            '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' |
            '\u{205f}' | '\u{3000}' | '\u{feff}'
        )
    });
    if input.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0b", 2),
        ("0B", 2),
        ("0o", 8),
        ("0O", 8),
    ] {
        if let Some(digits) = input.strip_prefix(prefix) {
            if digits.is_empty() {
                return f64::NAN;
            }
            return digits
                .chars()
                .try_fold(0.0, |n, c| {
                    c.to_digit(radix)
                        .map(|d| n * f64::from(radix) + f64::from(d))
                })
                .unwrap_or(f64::NAN);
        }
    }
    // Rust accepts `inf`; JavaScript Number accepts only spelled-out Infinity.
    if input.to_ascii_lowercase().contains("inf")
        && !matches!(input, "Infinity" | "+Infinity" | "-Infinity")
    {
        return f64::NAN;
    }
    input.parse().unwrap_or(f64::NAN)
}
