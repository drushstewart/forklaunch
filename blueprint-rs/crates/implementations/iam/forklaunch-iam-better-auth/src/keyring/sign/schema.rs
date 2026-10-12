use super::{BlueprintError, BlueprintResult, Value};
pub(super) fn validate(body: Option<&Value>) -> BlueprintResult<()> {
    let mut errors = Vec::new();
    if let Some(body) = body.filter(|body| body.is_object()) {
        for (field, required) in [("payload", true), ("overrideOptions", false)] {
            let value = body.get(field);
            if value.is_none() && !required {
                continue;
            }
            if !value.is_some_and(Value::is_object) {
                errors.push(format!(
                    "[body.{field}] Invalid input: expected record, received {}",
                    received(value)
                ));
            }
        }
    } else {
        errors.push(format!(
            "[body] Invalid input: expected object, received {}",
            received(body)
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(BlueprintError::new("VALIDATION_ERROR", errors.join("; ")))
    }
}
fn received(value: Option<&Value>) -> &'static str {
    match value {
        None => "undefined",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
    }
}
