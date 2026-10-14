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

/// Borrowed ordered string rules, including literal matching in UTF-16 positions.
/// Existing scalar operations remain available through `Step` without changing
/// the original `ZodStringStep` API. Needles are literal strings, not regexes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZodStringRule<'a> {
    Step(ZodStringStep),
    StartsWith(&'a str),
    EndsWith(&'a str),
    Includes {
        needle: &'a str,
        position: Option<f64>,
    },
}

/// One ordered string rule with an optional literal diagnostic override.
/// Type errors are unaffected; empty messages are retained exactly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZodStringCheck<'a> {
    pub rule: ZodStringRule<'a>,
    pub message: Option<&'a str>,
}

/// Apply strict Zod v3 string checks/transforms in declaration order.
/// `None` represents undefined; all non-string JSON values are rejected without
/// coercion. Trim uses ECMAScript whitespace and returns a borrowed substring.
/// Checks collect all failures in order, including checks before a later trim.
/// On failure no transformed value is returned, allowing atomic caller rollback.
///
/// Only default Zod v3 diagnostics and these four operations are implemented.
/// Use [`zod_string_checks`] for literal messages. Refinements, regexes, case conversion, coercive string
/// schemas, TypeBox/AJV length rules and lone UTF-16 surrogates are out of scope.
/// # Errors
/// Returns ordered public root-level parse errors; callers may prepend field paths.
pub fn zod_string<'a>(
    value: Option<&'a Value>,
    steps: &[ZodStringStep],
) -> Result<&'a str, Vec<ParseError>> {
    evaluate(
        value,
        steps.iter().copied().map(|step| ZodStringCheck {
            rule: ZodStringRule::Step(step),
            message: None,
        }),
    )
}

/// Apply literal checks and existing string operations in declaration order.
/// Includes positions use JavaScript's UTF-16 indexing and numeric clamping;
/// default diagnostics retain the original supplied position. This preserves
/// borrowed output and atomic failure semantics of [`zod_string`]. UTF-8 strings
/// cannot represent lone UTF-16 surrogates; regexes remain
/// outside this explicitly Zod v3 helper.
/// # Errors
/// Returns all ordered default Zod v3 diagnostics at the root path.
pub fn zod_string_rules<'a>(
    value: Option<&'a Value>,
    rules: &[ZodStringRule<'_>],
) -> Result<&'a str, Vec<ParseError>> {
    evaluate(
        value,
        rules.iter().copied().map(|rule| ZodStringCheck {
            rule,
            message: None,
        }),
    )
}

/// Apply ordered checks with exact literal messages, including empty strings.
/// Successful transforms remain borrowed; failed checks return no partial
/// transformed output. Strict type errors and arbitrary error maps are outside
/// the per-constraint override mechanism.
/// # Errors
/// Returns ordered root-level default or explicitly supplied diagnostics.
pub fn zod_string_checks<'a>(
    value: Option<&'a Value>,
    checks: &[ZodStringCheck<'_>],
) -> Result<&'a str, Vec<ParseError>> {
    evaluate(value, checks.iter().copied())
}

fn evaluate<'a, 'r>(
    value: Option<&'a Value>,
    checks: impl IntoIterator<Item = ZodStringCheck<'r>>,
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
    for check in checks {
        let step = match check.rule {
            ZodStringRule::Step(step) => step,
            literal => {
                if let Some(message) = literal_error(value, literal) {
                    errors.push(ParseError {
                        path: Vec::new(),
                        message: check.message.map_or(message, str::to_owned),
                    });
                }
                continue;
            }
        };
        let failed = match step {
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
                message: check.message.map_or_else(
                    || format!("String must contain {comparison} {limit} character(s)"),
                    str::to_owned,
                ),
            });
        }
    }
    if errors.is_empty() {
        Ok(value)
    } else {
        Err(errors)
    }
}

fn literal_error(value: &str, rule: ZodStringRule<'_>) -> Option<String> {
    match rule {
        ZodStringRule::StartsWith(needle) if !value.starts_with(needle) => {
            Some(format!("Invalid input: must start with \"{needle}\""))
        }
        ZodStringRule::EndsWith(needle) if !value.ends_with(needle) => {
            Some(format!("Invalid input: must end with \"{needle}\""))
        }
        ZodStringRule::Includes { needle, position } if !includes(value, needle, position) => {
            let suffix = position.map_or_else(String::new, |position| {
                format!(
                    " at one or more positions greater than or equal to {}",
                    ryu_js::Buffer::new().format(position)
                )
            });
            Some(format!("Invalid input: must include \"{needle}\"{suffix}"))
        }
        _ => None,
    }
}

#[allow(clippy::cast_precision_loss)]
fn includes(value: &str, needle: &str, position: Option<f64>) -> bool {
    if needle.is_empty() {
        return true;
    }
    let position = position.unwrap_or(0.0);
    let start = if position.is_nan() || position < 0.0 {
        0.0
    } else {
        position.trunc()
    };
    let mut units = 0usize;
    for (byte, character) in value.char_indices() {
        if units as f64 >= start {
            return value[byte..].contains(needle);
        }
        units += character.len_utf16();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn literal_messages_preserve_type_errors_and_atomic_transforms() {
        let checks = [
            ZodStringCheck {
                rule: ZodStringRule::Step(ZodStringStep::Trim),
                message: Some("unused"),
            },
            ZodStringCheck {
                rule: ZodStringRule::StartsWith("z"),
                message: Some(""),
            },
        ];
        assert_eq!(
            zod_string_checks(None, &checks).unwrap_err()[0].message,
            "Required"
        );
        let value = json!(" a ");
        assert_eq!(
            zod_string_checks(Some(&value), &checks).unwrap_err()[0].message,
            ""
        );
        assert_eq!(value, json!(" a "));
    }

    #[test]
    fn literal_positions_and_order_preserve_atomic_output() {
        let value = json!("💡a");
        assert_eq!(
            zod_string_rules(
                Some(&value),
                &[ZodStringRule::Includes {
                    needle: "a",
                    position: Some(1.5)
                }]
            ),
            Ok("💡a")
        );
        assert!(
            zod_string_rules(
                Some(&value),
                &[ZodStringRule::Includes {
                    needle: "💡",
                    position: Some(1.0)
                }]
            )
            .is_err()
        );
        let value = json!(" a ");
        let errors = zod_string_rules(
            Some(&value),
            &[
                ZodStringRule::StartsWith("a"),
                ZodStringRule::Step(ZodStringStep::Trim),
                ZodStringRule::EndsWith("b"),
            ],
        )
        .unwrap_err();
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].message, "Invalid input: must start with \"a\"");
        assert_eq!(errors[1].message, "Invalid input: must end with \"b\"");
        assert_eq!(value, json!(" a "));
    }

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
