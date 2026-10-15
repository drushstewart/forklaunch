//! Regex-aware ordered strings with operational failures outside parse errors.
use super::super::{ZodStringCheck, ZodStringRule, strict_string, zod_string_checks};
use super::{Budget, Parsed, Status, ZodJsonUnsupported};
use crate::ParseError;
use forklaunch_common::zod_regex::{RegexMatchLimits, ZodRegex};
use serde_json::Value;

/// Ordered scalar operations and precompiled bounded regex assertions.
/// Compiled regex ownership/admission remains with the caller; this borrows it.
#[derive(Clone, Copy, Debug)]
pub enum ZodJsonStringCheck<'a> {
    Scalar(ZodStringCheck<'a>),
    Regex {
        compiled: &'a ZodRegex,
        limits: RegexMatchLimits,
        message: Option<&'a str>,
    },
}
impl ZodJsonStringCheck<'_> {
    pub(super) fn bytes(self) -> usize {
        match self {
            Self::Regex { message, .. } => message.map_or(0, str::len),
            Self::Scalar(check) => {
                check
                    .message
                    .map_or(0, str::len)
                    .saturating_add(match check.rule {
                        ZodStringRule::StartsWith(value)
                        | ZodStringRule::EndsWith(value)
                        | ZodStringRule::Includes { needle: value, .. } => value.len(),
                        _ => 0,
                    })
            }
        }
    }
}
pub(super) fn evaluate(
    input: Option<&Value>,
    checks: &[ZodJsonStringCheck<'_>],
    budget: &mut Budget,
) -> Result<Parsed, ZodJsonUnsupported> {
    let mut value = match zod_string_checks(input, &[]) {
        Ok(value) => value,
        Err(errors) => return Ok(Parsed::invalid(errors, Status::Aborted)),
    };
    let mut length = value.encode_utf16().count() as u64;
    let mut errors = Vec::new();
    for check in checks {
        match *check {
            ZodJsonStringCheck::Scalar(check) => {
                if let Some(error) = strict_string::apply_check(&mut value, &mut length, check) {
                    errors.push(error);
                }
            }
            ZodJsonStringCheck::Regex {
                compiled,
                limits,
                message,
            } => {
                let work = limits
                    .input_units
                    .checked_mul(2)
                    .and_then(|v| v.checked_add(1))
                    .and_then(|v| v.checked_add(limits.work))
                    .ok_or(ZodJsonUnsupported::RegexResource)?;
                budget.regex_work = budget
                    .regex_work
                    .checked_sub(work)
                    .ok_or(ZodJsonUnsupported::RegexResource)?;
                if !compiled
                    .test(value, limits)
                    .map_err(|_| ZodJsonUnsupported::RegexResource)?
                {
                    errors.push(ParseError {
                        path: Vec::new(),
                        message: message.unwrap_or("Invalid").to_owned(),
                    });
                }
            }
        }
    }
    Ok(if errors.is_empty() {
        Parsed::valid(Some(Value::String(value.to_owned())))
    } else {
        Parsed::invalid(errors, Status::Dirty)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typescript_coerce::{ZodJsonOutcome, ZodJsonSchema, ZodStringStep, zod_json};
    use forklaunch_common::zod_regex::RegexCompileLimits;
    use serde_json::json;
    fn pattern(source: &str) -> ZodRegex {
        ZodRegex::compile(
            &source.encode_utf16().collect::<Vec<_>>(),
            "",
            RegexCompileLimits::default(),
        )
        .unwrap()
    }
    #[test]
    fn transforms_and_diagnostics_are_ordered_and_atomic() {
        let regex = pattern("^a$");
        let check = ZodJsonStringCheck::Regex {
            compiled: &regex,
            limits: RegexMatchLimits::default(),
            message: Some("regex"),
        };
        let trim = ZodJsonStringCheck::Scalar(ZodStringCheck {
            rule: ZodStringRule::Step(ZodStringStep::Trim),
            message: None,
        });
        let input = json!(" a ");
        assert_eq!(
            zod_json(
                Some(&input),
                &ZodJsonSchema::CheckedString(vec![trim, check])
            )
            .unwrap(),
            ZodJsonOutcome::Valid(Some(json!("a")))
        );
        let ZodJsonOutcome::Invalid(errors) = zod_json(
            Some(&input),
            &ZodJsonSchema::CheckedString(vec![check, trim, check]),
        )
        .unwrap() else {
            panic!("expected invalid")
        };
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "regex");
        assert_eq!(input, json!(" a "));
    }
    #[test]
    fn operational_failure_cannot_be_swallowed_by_union() {
        let regex = pattern("^(a+)+$");
        let schema = ZodJsonSchema::Union(vec![
            ZodJsonSchema::CheckedString(vec![ZodJsonStringCheck::Regex {
                compiled: &regex,
                limits: RegexMatchLimits {
                    work: 1000,
                    ..Default::default()
                },
                message: None,
            }]),
            ZodJsonSchema::Unknown,
        ]);
        assert_eq!(
            zod_json(Some(&json!(format!("{}!", "a".repeat(40)))), &schema),
            Err(ZodJsonUnsupported::RegexResource)
        );
    }
    #[test]
    fn array_items_share_aggregate_work() {
        let regex = pattern("a");
        let schema = ZodJsonSchema::Array {
            element: Box::new(ZodJsonSchema::CheckedString(vec![
                ZodJsonStringCheck::Regex {
                    compiled: &regex,
                    limits: RegexMatchLimits::default(),
                    message: None,
                },
            ])),
            checks: vec![],
        };
        assert!(matches!(
            zod_json(Some(&json!(["a", "a"])), &schema),
            Ok(ZodJsonOutcome::Valid(_))
        ));
        assert_eq!(
            zod_json(Some(&json!(vec!["a"; 40])), &schema),
            Err(ZodJsonUnsupported::RegexResource)
        );
    }
}
