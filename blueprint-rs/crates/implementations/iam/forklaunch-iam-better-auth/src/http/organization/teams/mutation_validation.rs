use super::super::validation::{received, validation};
use super::{BlueprintResult, Value};
pub(super) fn validate(path: &str, body: Option<&Value>) -> BlueprintResult<()> {
    if !body.is_some_and(Value::is_object) {
        return Err(validation(format!(
            "[body] Invalid input: expected object, received {}",
            received(body)
        )));
    }
    let body = body.expect("validated object");
    let mut errors = Vec::new();
    let fields: &[(&str, bool)] = match path {
        "/organization/create-team" => &[("name", true), ("organizationId", false)],
        "/organization/remove-team" => &[("teamId", true), ("organizationId", false)],
        _ => &[("teamId", true)],
    };
    for (field, required) in fields {
        string(&mut errors, body, field, *required, "body");
    }
    if path == "/organization/update-team" {
        if let Some(data) = body.get("data").filter(|value| value.is_object()) {
            string(&mut errors, data, "name", false, "body.data");
            if data["name"].as_str() == Some("") {
                errors.push(
                    "[body.data.name] Too small: expected string to have >=1 characters".into(),
                );
            }
            string(&mut errors, data, "organizationId", false, "body.data");
        } else {
            errors.push(format!(
                "[body.data] Invalid input: expected object, received {}",
                received(body.get("data"))
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(validation(errors.join("; ")))
    }
}
fn string(errors: &mut Vec<String>, body: &Value, field: &str, required: bool, prefix: &str) {
    let value = body.get(field);
    if (required || value.is_some()) && !value.is_some_and(Value::is_string) {
        errors.push(format!(
            "[{prefix}.{field}] Invalid input: expected string, received {}",
            received(value)
        ));
    }
}
