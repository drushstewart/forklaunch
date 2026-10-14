use super::{BlueprintResult, Value, error};
pub(super) fn parse(value: &Value, better: bool) -> BlueprintResult<f64> {
    let text = crate::ecmascript::string(Some(value))
        .ok_or_else(|| error("Cannot convert object to primitive value"))?;
    let pattern = if better {
        r"(?i-u)^(\+|\-)? ?([0-9]+|[0-9]+\.[0-9]+) ?(seconds?|secs?|s|minutes?|mins?|m|hours?|hrs?|h|days?|d|weeks?|w|months?|mo|years?|yrs?|y)(?: (ago|from now))?$"
    } else {
        r"(?i-u)^(\+|\-)? ?([0-9]+|[0-9]+\.[0-9]+) ?(seconds?|secs?|s|minutes?|mins?|m|hours?|hrs?|h|days?|d|weeks?|w|years?|yrs?|y)(?: (ago|from now))?$"
    };
    let matched = regex::Regex::new(pattern)
        .expect("fixed time grammar")
        .captures(&text)
        .filter(|m| m.get(1).is_none() || m.get(4).is_none());
    let invalid = || {
        error(if better {
            format!(
                "Invalid time string format: \"{text}\". Use formats like \"7d\", \"30m\", \"1 hour\", etc."
            )
        } else {
            "Invalid time period format".into()
        })
    };
    if !better && !value.is_string() {
        return Err(invalid());
    }
    let matched = matched.ok_or_else(invalid)?;
    let amount = matched[2].parse::<f64>().map_err(|_| invalid())?;
    let unit = matched[3].to_ascii_lowercase();
    let factor = if unit == "mo" || unit.starts_with("month") {
        2_592_000.0
    } else {
        match unit.as_bytes()[0] {
            b's' => 1.0,
            b'm' => 60.0,
            b'h' => 3_600.0,
            b'd' => 86_400.0,
            b'w' => 604_800.0,
            _ => 31_557_600.0,
        }
    };
    let negative = matched.get(1).is_some_and(|m| m.as_str() == "-")
        || matched.get(4).is_some_and(|m| m.as_str() == "ago");
    let sign = if negative { -1.0 } else { 1.0 };
    let seconds = if better {
        round_js((amount * (factor * 1_000.0) * sign) / 1_000.0)
    } else {
        (amount * factor + 0.5).floor() * sign
    };
    if !better && !seconds.is_finite() {
        return Err(invalid());
    }
    Ok(seconds)
}

// Math.round preserves integral large values and chooses +infinity at half ties.
// Adding 0.5 first changes already-integral safe-boundary inputs through rounding.
fn round_js(value: f64) -> f64 {
    if !value.is_finite() || value.fract() == 0.0 {
        return value;
    }
    let floor = value.floor();
    let rounded = if value - floor < 0.5 {
        floor
    } else {
        floor + 1.0
    };
    if rounded == 0.0 && value.is_sign_negative() {
        -0.0
    } else {
        rounded
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_better_auth_rounding_keeps_safe_boundary_and_negative_zero() {
        for (text, expected) in [
            ("9007199254740991s", 9_007_199_254_740_991.0_f64),
            ("0.49999999999999994s", 0.0),
            ("-0.5s", -0.0),
            ("-1.5s", -1.0),
        ] {
            assert_eq!(
                parse(&Value::String(text.into()), true).unwrap().to_bits(),
                expected.to_bits(),
                "{text}"
            );
        }
    }
}
