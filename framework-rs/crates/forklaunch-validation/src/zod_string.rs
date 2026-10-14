//! Strict Zod v3 string operations over the UTF-8 JSON value domain.
use crate::ParseError;
use serde_json::Value;

/// Ordered operations for `ForkLaunch`'s Zod v3 strict string schema.
/// Length bounds are exact unsigned 32-bit integers; they count UTF-16 code
/// units, unlike JSON Schema/TypeBox/AJV Unicode scalar length constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZodStringStep {
    Min(u32),
    Max(u32),
    Length(u32),
    Trim,
}

/// Apply strict Zod v3 string checks/transforms in declaration order.
/// `None` represents undefined; all non-string JSON values are rejected without
/// coercion. Trim uses ECMAScript whitespace and returns a borrowed substring.
/// Checks collect all failures in order, including checks before a later trim.
/// On failure no transformed value is returned, allowing atomic caller rollback.
///
/// Only default Zod v3 diagnostics and these four operations are implemented.
/// Custom messages, refinements, regexes, case conversion, coercive string
/// schemas, TypeBox/AJV length rules and lone UTF-16 surrogates are out of scope.
/// # Errors
/// Returns ordered public root-level parse errors; callers may prepend field paths.
pub fn zod_string<'a>(
    value: Option<&'a Value>,
    steps: &[ZodStringStep],
) -> Result<&'a str, Vec<ParseError>> {
    let mut value = match value {
        Some(Value::String(value)) => value.as_str(),
        other => {
            return Err(vec![ParseError {
                path: Vec::new(),
                message: other.map_or_else(
                    || "Required".into(),
                    |value| {
                        format!(
                            "Expected string, received {}",
                            match value {
                                Value::Null => "null",
                                Value::Bool(_) => "boolean",
                                Value::Number(_) => "number",
                                Value::Array(_) => "array",
                                Value::Object(_) => "object",
                                Value::String(_) => "string",
                            }
                        )
                    },
                ),
            }]);
        }
    };
    let mut length = value.encode_utf16().count() as u64;
    let mut errors = Vec::new();
    for step in steps {
        let failed = match *step {
            ZodStringStep::Min(limit) if length < u64::from(limit) => Some(("at least", limit)),
            ZodStringStep::Max(limit) if length > u64::from(limit) => Some(("at most", limit)),
            ZodStringStep::Length(limit) if length != u64::from(limit) => Some(("exactly", limit)),
            ZodStringStep::Trim => {
                value = value.trim_matches(super::ecmascript_whitespace);
                length = value.encode_utf16().count() as u64;
                None
            }
            _ => None,
        };
        if let Some((comparison, limit)) = failed {
            errors.push(ParseError {
                path: Vec::new(),
                message: format!("String must contain {comparison} {limit} character(s)"),
            });
        }
    }
    if errors.is_empty() {
        Ok(value)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn utf16_length_and_order_match_zod_without_changing_input() {
        assert_eq!(
            zod_string(Some(&json!("💡")), &[ZodStringStep::Length(2)]),
            Ok("💡")
        );
        let input = json!(" a ");
        assert_eq!(
            zod_string(Some(&input), &[ZodStringStep::Min(2), ZodStringStep::Trim]),
            Ok("a")
        );
        assert!(zod_string(Some(&input), &[ZodStringStep::Trim, ZodStringStep::Min(2)]).is_err());
        assert_eq!(input, json!(" a "));
        let errors = zod_string(
            Some(&json!("ab")),
            &[ZodStringStep::Max(1), ZodStringStep::Min(3)],
        )
        .unwrap_err();
        assert_eq!(errors.len(), 2);
        assert!(errors[0].message.contains("at most"));
        assert!(errors[1].message.contains("at least"));
    }
    #[test]
    fn ecmascript_whitespace_differs_from_rust_unicode_whitespace() {
        assert_eq!(
            zod_string(Some(&json!("\u{feff}x\u{feff}")), &[ZodStringStep::Trim]),
            Ok("x")
        );
        assert_eq!(
            zod_string(Some(&json!("\u{0085}x\u{0085}")), &[ZodStringStep::Trim]),
            Ok("\u{0085}x\u{0085}")
        );
        assert_eq!(zod_string(None, &[]).unwrap_err()[0].message, "Required");
    }
}
