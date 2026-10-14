//! Ordered Zod v3 number constraints after `ForkLaunch` number preprocessing.
use crate::ParseError;
use serde_json::Value;

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// A bound whose integer value and decimal diagnostic are exact in JavaScript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZodIntegerBound(i64);
impl ZodIntegerBound {
    /// Returns `None` outside JavaScript's safe integer range.
    #[must_use]
    pub const fn new(value: i64) -> Option<Self> {
        if value >= -MAX_SAFE_INTEGER && value <= MAX_SAFE_INTEGER {
            Some(Self(value))
        } else {
            None
        }
    }
}

/// Ordered constraints for the repository's Zod v3 number preprocessing.
/// Min/max bounds are inclusive and restricted to exact safe integers; input
/// numbers may be fractional, infinite or outside the safe integer range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZodNumberStep {
    Int,
    Finite,
    /// Checks only magnitude, as Zod v3 does; this does not require an integer.
    Safe,
    Min(ZodIntegerBound),
    Max(ZodIntegerBound),
}

/// Apply ordered constraints after the existing JavaScript number coercion.
/// This matches `number.pipe(number.innerType().checks...)` using the actual
/// `ForkLaunch` Zod v3 factory. Coercion failures stop checks; check failures
/// accumulate in order. Successful output retains IEEE-754 bits, including
/// negative zero and infinities unless a constraint excludes them.
///
/// This is a bounded helper, not a general Zod or JSON Schema interpreter.
/// Fractional/nonfinite bounds, exclusive bounds, multiple-of, custom messages,
/// refinements and arbitrary JavaScript objects are not implemented. Successful
/// nonfinite results cannot be represented as a JSON number; callers must not
/// silently serialize them as null. Existing native validation stays unchanged.
/// # Errors
/// Returns default Zod v3 public diagnostics at the root path.
pub fn zod_number(input: Option<&Value>, steps: &[ZodNumberStep]) -> Result<f64, Vec<ParseError>> {
    let value = super::number(input).map_err(|error| vec![error])?;
    let mut errors = Vec::new();
    for step in steps {
        match *step {
            ZodNumberStep::Int if !value.is_finite() || value.fract() != 0.0 => {
                errors.push(diagnostic("Expected integer, received float"));
            }
            ZodNumberStep::Finite if !value.is_finite() => {
                errors.push(diagnostic("Number must be finite"));
            }
            ZodNumberStep::Safe => {
                bound(value, -MAX_SAFE_INTEGER, true, &mut errors);
                bound(value, MAX_SAFE_INTEGER, false, &mut errors);
            }
            ZodNumberStep::Min(limit) => bound(value, limit.0, true, &mut errors),
            ZodNumberStep::Max(limit) => bound(value, limit.0, false, &mut errors),
            _ => {}
        }
    }
    if errors.is_empty() {
        Ok(value)
    } else {
        Err(errors)
    }
}
fn diagnostic(message: impl Into<String>) -> ParseError {
    ParseError {
        path: Vec::new(),
        message: message.into(),
    }
}
#[allow(clippy::cast_precision_loss)] // Constructor guarantees an exact safe integer.
fn bound(value: f64, limit: i64, minimum: bool, errors: &mut Vec<ParseError>) {
    if if minimum {
        value < limit as f64
    } else {
        value > limit as f64
    } {
        let comparison = if minimum { "greater" } else { "less" };
        errors.push(diagnostic(format!(
            "Number must be {comparison} than or equal to {limit}"
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn constraints_preserve_order_and_safe_does_not_mean_integer() {
        assert_eq!(
            zod_number(Some(&json!(0.5)), &[ZodNumberStep::Safe]),
            Ok(0.5)
        );
        let errors = zod_number(
            Some(&json!("Infinity")),
            &[
                ZodNumberStep::Int,
                ZodNumberStep::Finite,
                ZodNumberStep::Safe,
            ],
        )
        .unwrap_err();
        assert_eq!(
            errors
                .iter()
                .map(|error| error.message.as_str())
                .collect::<Vec<_>>(),
            [
                "Expected integer, received float",
                "Number must be finite",
                "Number must be less than or equal to 9007199254740991"
            ]
        );
        assert_eq!(
            zod_number(Some(&json!("-0")), &[]).unwrap().to_bits(),
            (-0.0f64).to_bits()
        );
        assert_eq!(
            zod_number(
                Some(&json!("NaN")),
                &[ZodNumberStep::Int, ZodNumberStep::Finite]
            )
            .unwrap_err()
            .len(),
            1
        );
    }
    #[test]
    fn bounds_require_exact_javascript_integers() {
        assert!(ZodIntegerBound::new(MAX_SAFE_INTEGER).is_some());
        assert!(ZodIntegerBound::new(-MAX_SAFE_INTEGER).is_some());
        for limit in [
            MAX_SAFE_INTEGER + 1,
            -MAX_SAFE_INTEGER - 1,
            i64::MIN,
            i64::MAX,
        ] {
            assert!(ZodIntegerBound::new(limit).is_none());
        }
    }
}
