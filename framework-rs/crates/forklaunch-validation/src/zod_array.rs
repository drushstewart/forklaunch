//! Opt-in Zod v3 array container checks, independent of element schemas.
use crate::ParseError;
use serde_json::Value;

/// Array schema operations in declaration order. The last operation of each
/// kind wins, matching Zod v3; `Nonempty` replaces the minimum with one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZodArrayStep {
    Min(u32),
    Max(u32),
    Length(u32),
    Nonempty,
}

/// Check a strict JSON array's type and length using default Zod v3 diagnostics.
/// `None` represents undefined. Bounds are exact unsigned 32-bit integers.
/// Diagnostics appear in exact-length, minimum, maximum order, regardless of
/// declaration order. Successful output borrows the original elements unchanged.
///
/// This matches the container behavior of `array(unknown)` in `ForkLaunch`'s
/// Zod factory. It does not validate/transform elements, interpret custom schema
/// refinements/messages, or represent sparse arrays/undefined JavaScript items.
/// Callers composing element checks must still run them after container length
/// failures to collect their diagnostics, and commit output atomically. Existing
/// native JSON Schema validation is unchanged.
/// # Errors
/// Returns public root-level type or container length diagnostics.
pub fn zod_array_shape<'a>(
    input: Option<&'a Value>,
    steps: &[ZodArrayStep],
) -> Result<&'a [Value], Vec<ParseError>> {
    let values = match input {
        Some(Value::Array(values)) => values.as_slice(),
        other => {
            return Err(vec![error(other.map_or_else(
                || "Required".into(),
                |value| {
                    format!(
                        "Expected array, received {}",
                        match value {
                            Value::Null => "null",
                            Value::Bool(_) => "boolean",
                            Value::Number(_) => "number",
                            Value::String(_) => "string",
                            Value::Object(_) => "object",
                            Value::Array(_) => "array",
                        }
                    )
                },
            ))]);
        }
    };
    let (mut minimum, mut maximum, mut exact) = (None, None, None);
    for step in steps {
        match *step {
            ZodArrayStep::Min(value) => minimum = Some(value),
            ZodArrayStep::Max(value) => maximum = Some(value),
            ZodArrayStep::Length(value) => exact = Some(value),
            ZodArrayStep::Nonempty => minimum = Some(1),
        }
    }
    let count = values.len() as u64;
    let errors: Vec<_> = [
        (exact, "exactly"),
        (minimum, "at least"),
        (maximum, "at most"),
    ]
    .into_iter()
    .filter_map(|(bound, comparison)| {
        let bound = bound?;
        let fails = match comparison {
            "exactly" => count != u64::from(bound),
            "at least" => count < u64::from(bound),
            _ => count > u64::from(bound),
        };
        fails.then(|| {
            error(format!(
                "Array must contain {comparison} {bound} element(s)"
            ))
        })
    })
    .collect();
    if errors.is_empty() {
        Ok(values)
    } else {
        Err(errors)
    }
}
fn error(message: String) -> ParseError {
    ParseError {
        path: Vec::new(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn last_constraints_win_and_errors_have_fixed_order() {
        let input = json!([null]);
        let steps = [
            ZodArrayStep::Min(9),
            ZodArrayStep::Max(0),
            ZodArrayStep::Length(3),
            ZodArrayStep::Min(4),
        ];
        let messages: Vec<_> = zod_array_shape(Some(&input), &steps)
            .unwrap_err()
            .into_iter()
            .map(|error| error.message)
            .collect();
        assert_eq!(
            messages,
            [
                "Array must contain exactly 3 element(s)",
                "Array must contain at least 4 element(s)",
                "Array must contain at most 0 element(s)"
            ]
        );
        assert!(
            zod_array_shape(
                Some(&input),
                &[ZodArrayStep::Min(9), ZodArrayStep::Nonempty]
            )
            .is_ok()
        );
        assert!(
            zod_array_shape(
                Some(&input),
                &[ZodArrayStep::Nonempty, ZodArrayStep::Min(9)]
            )
            .is_err()
        );
        assert_eq!(input, json!([null]));
    }
    #[test]
    fn accepted_elements_are_borrowed_without_validation_or_transforms() {
        let input = json!(["  raw  ", {"unknown": 3}, [null]]);
        let output = zod_array_shape(Some(&input), &[ZodArrayStep::Length(3)]).unwrap();
        assert!(std::ptr::eq(output, input.as_array().unwrap().as_slice()));
        assert_eq!(
            zod_array_shape(None, &[]).unwrap_err()[0].message,
            "Required"
        );
    }
}
