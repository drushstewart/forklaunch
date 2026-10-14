//! Runtime Zod v3 bounds, independent of JSON Schema's finite-number domain.

/// An IEEE-754 bound for Zod v3's ordered runtime comparison checks.
/// All values are accepted, including NaN (which never rejects) and infinities.
/// This does not declare or export a JSON Schema numeric bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZodFloatBound(u64);

impl ZodFloatBound {
    #[must_use]
    pub const fn new(value: f64) -> Self {
        Self(value.to_bits())
    }

    pub(super) fn rejection(self, value: f64, minimum: bool, inclusive: bool) -> Option<String> {
        let bound = f64::from_bits(self.0);
        let rejected = match (minimum, inclusive) {
            (true, true) => value < bound,
            (true, false) => value <= bound,
            (false, true) => value > bound,
            (false, false) => value >= bound,
        };
        rejected.then(|| {
            let comparison = if minimum { "greater" } else { "less" };
            let equality = if inclusive { " or equal to" } else { "" };
            format!(
                "Number must be {comparison} than{equality} {}",
                ryu_js::Buffer::new().format(bound)
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nan_bounds_are_inert_and_infinity_equality_is_ordered() {
        for minimum in [false, true] {
            for inclusive in [false, true] {
                assert!(
                    ZodFloatBound::new(f64::NAN)
                        .rejection(1.0, minimum, inclusive)
                        .is_none()
                );
            }
        }
        assert!(
            ZodFloatBound::new(f64::INFINITY)
                .rejection(f64::INFINITY, true, true)
                .is_none()
        );
        assert_eq!(
            ZodFloatBound::new(f64::INFINITY)
                .rejection(f64::INFINITY, true, false)
                .unwrap(),
            "Number must be greater than Infinity"
        );
        assert_eq!(
            ZodFloatBound::new(-0.0)
                .rejection(-0.0, false, false)
                .unwrap(),
            "Number must be less than 0"
        );
    }
}
