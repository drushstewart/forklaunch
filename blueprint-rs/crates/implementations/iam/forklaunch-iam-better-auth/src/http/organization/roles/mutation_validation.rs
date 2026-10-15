use super::{BlueprintResult, Value, received, validation};
pub(super) fn validate_body(path: &str, body: Option<&Value>) -> BlueprintResult<()> {
    if !matches!(
        path,
        "/organization/update-role" | "/organization/delete-role"
    ) {
        return Ok(());
    }
    if !body.is_some_and(Value::is_object) {
        return Err(validation(format!(
            "[body] Invalid input: expected object, received {}; [body] Invalid input",
            received(body)
        )));
    }
    let body = body.expect("validated object");
    let mut errors = Vec::new();
    if let Some(value) = body.get("organizationId")
        && !value.is_string()
    {
        errors.push(format!(
            "[body.organizationId] Invalid input: expected string, received {}",
            received(Some(value))
        ));
    }
    if path == "/organization/update-role" {
        if body.get("data").is_some_and(Value::is_object) {
            data_errors(&body["data"], &mut errors);
        } else {
            errors.push(format!(
                "[body.data] Invalid input: expected object, received {}",
                received(body.get("data"))
            ));
        }
    }
    let name = body.get("roleName");
    let id = body.get("roleId");
    if name.and_then(Value::as_str).is_none_or(str::is_empty)
        && id.and_then(Value::as_str).is_none_or(str::is_empty)
    {
        errors.push(if name.is_some_and(Value::is_string) {
            "[body.roleName] Too small: expected string to have >=1 characters".into()
        } else if id.is_some_and(Value::is_string) {
            "[body.roleId] Too small: expected string to have >=1 characters".into()
        } else {
            "[body] Invalid input".into()
        });
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(validation(errors.join("; ")))
    }
}
fn data_errors(data: &Value, errors: &mut Vec<String>) {
    if let Some(permission) = data.get("permission") {
        if let Some(fields) = permission.as_object() {
            for (resource, actions) in fields.iter().filter(|(key, _)| key.as_str() != "__proto__")
            {
                if let Some(actions) = actions.as_array() {
                    for (i, action) in actions.iter().enumerate() {
                        if !action.is_string() {
                            errors.push(format!("[body.data.permission.{resource}.{i}] Invalid input: expected string, received {}", received(Some(action))));
                        }
                    }
                } else {
                    errors.push(format!("[body.data.permission.{resource}] Invalid input: expected array, received {}", received(Some(actions))));
                }
            }
        } else {
            errors.push(format!(
                "[body.data.permission] Invalid input: expected record, received {}",
                received(Some(permission))
            ));
        }
    }
    if let Some(name) = data.get("roleName")
        && !name.is_string()
    {
        errors.push(format!(
            "[body.data.roleName] Invalid input: expected string, received {}",
            received(Some(name))
        ));
    }
}
