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
        (amount * factor * sign + 0.5).floor()
    } else {
        (amount * factor + 0.5).floor() * sign
    };
    if !better && !seconds.is_finite() {
        return Err(invalid());
    }
    Ok(seconds)
}
