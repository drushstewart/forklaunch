use super::super::validation::{received, validation};
use forklaunch_blueprint_core::BlueprintResult;
use serde_json::Value;
pub(super) fn validate(body: Option<&Value>) -> BlueprintResult<()> {
    if !body.is_some_and(Value::is_object) {
        return Err(validation(format!(
            "[body] Invalid input: expected object, received {}",
            received(body)
        )));
    }
    let mut errors = Vec::new();
    for (field, kind, required) in [
        ("email", "string", true),
        ("role", "union", true),
        ("organizationId", "string", false),
        ("resend", "boolean", false),
        ("teamId", "union", false),
    ] {
        let value = body.and_then(|body| body.get(field));
        if value.is_none() && !required {
            continue;
        }
        let valid = match kind {
            "union" => value.is_some_and(|v| {
                v.is_string()
                    || v.as_array()
                        .is_some_and(|values| values.iter().all(Value::is_string))
            }),
            "boolean" => value.is_some_and(Value::is_boolean),
            _ => value.is_some_and(Value::is_string),
        };
        if !valid {
            errors.push(if kind == "union" {
                format!("[body.{field}] Invalid input")
            } else {
                format!(
                    "[body.{field}] Invalid input: expected {kind}, received {}",
                    received(value)
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
