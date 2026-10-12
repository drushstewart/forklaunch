//! Configured Zod schema ordering for organization inputs.
use super::{BlueprintError, BlueprintResult, HashMap, Value, internal};
pub(crate) fn validate_body(path: &str, body: Option<&Value>) -> BlueprintResult<()> {
    if super::roles::is_route(path) {
        return super::roles::validate_body(path, body);
    }
    if super::invitations::is_route(path) {
        return super::invitations::validate_body(path, body);
    }
    if super::teams::is_route(path) {
        return super::teams::validate_body(path, body);
    }
    if super::members::is_route(path) {
        return super::members::validate_body(path, body);
    }
    if matches!(
        path,
        "/organization/update" | "/organization/has-permission"
    ) {
        return super::mutation_validation::validate_body(path, body);
    }
    if matches!(
        path,
        "/organization/list"
            | "/organization/get-full-organization"
            | "/organization/get-organization"
    ) {
        return Ok(());
    }
    if !body.is_some_and(Value::is_object) {
        return Err(validation(format!(
            "[body] Invalid input: expected object, received {}",
            received(body)
        )));
    }
    let body = body.ok_or_else(internal)?;
    let fields: &[(&str, &str, bool, bool)] = match path {
        "/organization/create" => &[
            ("name", "string", true, false),
            ("slug", "string", true, false),
            ("logo", "string", false, true),
            ("metadata", "record", false, false),
            ("keepCurrentActiveOrganization", "boolean", false, false),
            ("domain", "string", false, true),
            ("subscription", "string", false, true),
            ("status", "string", false, true),
        ],
        "/organization/delete" => &[("organizationId", "string", true, false)],
        "/organization/check-slug" => &[("slug", "string", true, false)],
        "/organization/set-active" => &[
            ("organizationId", "string", false, true),
            ("organizationSlug", "string", false, false),
        ],
        _ => return Err(internal()),
    };
    let mut errors = Vec::new();
    for (field, kind, required, nullable) in fields {
        let value = body.get(*field);
        if value.is_none() && !required || *nullable && value == Some(&Value::Null) {
            continue;
        }
        let valid = match *kind {
            "record" => value.is_some_and(Value::is_object),
            "boolean" => value.is_some_and(Value::is_boolean),
            _ => value.is_some_and(Value::is_string),
        };
        if !valid {
            errors.push(format!(
                "[body.{field}] Invalid input: expected {kind}, received {}",
                received(value)
            ));
        } else if path == "/organization/create"
            && matches!(*field, "name" | "slug")
            && value.and_then(Value::as_str) == Some("")
        {
            errors.push(format!(
                "[body.{field}] Too small: expected string to have >=1 characters"
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(validation(errors.join("; ")))
    }
}
pub(crate) fn validate_query(path: &str, query: &HashMap<String, Value>) -> BlueprintResult<()> {
    super::roles::validate_query(path, query)?;
    super::invitations::validate_query(path, query)?;
    super::teams::validate_query(path, query)?;
    super::members::validate_query(path, query)?;
    if !matches!(
        path,
        "/organization/get-full-organization" | "/organization/get-organization"
    ) {
        return Ok(());
    }
    let mut errors = Vec::new();
    for field in ["organizationId", "organizationSlug", "membersLimit"] {
        if field == "membersLimit" && path == "/organization/get-organization" {
            continue;
        }
        if let Some(value) = query.get(field)
            && !value.is_string()
        {
            errors.push(if field == "membersLimit" {
                "[query.membersLimit] Invalid input".into()
            } else {
                format!(
                    "[query.{field}] Invalid input: expected string, received {}",
                    received(Some(value))
                )
            });
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(validation(errors.join("; ")))
    }
}
pub(super) fn validation(message: String) -> BlueprintError {
    BlueprintError::new("VALIDATION_ERROR", message)
}
pub(super) fn received(value: Option<&Value>) -> &'static str {
    match value {
        None => "undefined",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(value))
            if value
                .to_string()
                .parse::<f64>()
                .is_ok_and(|n| n == f64::INFINITY) =>
        {
            "Infinity"
        }
        Some(Value::Number(value))
            if value
                .to_string()
                .parse::<f64>()
                .is_ok_and(|n| n == f64::NEG_INFINITY) =>
        {
            "-Infinity"
        }
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
    }
}
pub(super) fn integer(value: &str) -> Option<i64> {
    let value=value.trim_start_matches(|character:char|matches!(character,'\u{0009}'..='\u{000d}'|'\u{0020}'|'\u{00a0}'|'\u{1680}'|'\u{2000}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{202f}'|'\u{205f}'|'\u{3000}'|'\u{feff}'));
    let (negative, value) = if let Some(value) = value.strip_prefix('-') {
        (true, value)
    } else {
        (false, value.strip_prefix('+').unwrap_or(value))
    };
    let (radix, value) = if value.starts_with("0x") || value.starts_with("0X") {
        (16, &value[2..])
    } else {
        (10, value)
    };
    let digits: String = value
        .chars()
        .take_while(|character| character.is_digit(radix))
        .collect();
    if digits.is_empty() {
        return None;
    }
    let number = i64::from_str_radix(&digits, radix).unwrap_or(i64::MAX);
    Some(if negative { -number } else { number })
}
