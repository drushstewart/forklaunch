mod mutation_validation;
use super::validation::{received, validation};
use super::{AuthStore, BlueprintResult, DateTime, HashMap, OrganizationService, Utc, Value};
pub(super) fn is_route(path: &str) -> bool {
    matches!(
        path,
        "/organization/list-roles"
            | "/organization/get-role"
            | "/organization/create-role"
            | "/organization/update-role"
            | "/organization/delete-role"
    )
}
pub(super) fn validate_query(path: &str, query: &HashMap<String, Value>) -> BlueprintResult<()> {
    if !matches!(path, "/organization/list-roles" | "/organization/get-role") {
        return Ok(());
    }
    let mut errors = Vec::new();
    if let Some(value) = query.get("organizationId")
        && !value.is_string()
    {
        errors.push(format!(
            "[query.organizationId] Invalid input: expected string, received {}",
            received(Some(value))
        ));
    }
    if path == "/organization/get-role" {
        let name = query.get("roleName");
        let id = query.get("roleId");
        if name.and_then(Value::as_str).is_none_or(str::is_empty)
            && id.and_then(Value::as_str).is_none_or(str::is_empty)
        {
            errors.push(if name.is_some_and(Value::is_string) {
                "[query.roleName] Too small: expected string to have >=1 characters".into()
            } else if id.is_some_and(Value::is_string) {
                "[query.roleId] Too small: expected string to have >=1 characters".into()
            } else {
                "[query] Invalid input".into()
            });
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(validation(errors.join("; ")))
    }
}
pub(super) async fn handle<S: AuthStore>(
    service: &OrganizationService<S>,
    path: &str,
    payload: &Value,
    query: &HashMap<String, Value>,
    session: (&str, bool),
    now: DateTime<Utc>,
) -> BlueprintResult<Option<Value>> {
    let (token, transient) = session;
    Ok(Some(match path {
        "/organization/update-role" => {
            service
                .update_organization_role(token, transient, payload, now)
                .await?
        }
        "/organization/delete-role" => {
            service
                .delete_organization_role(token, transient, payload, now)
                .await?
        }
        "/organization/create-role" => {
            service
                .create_organization_role(token, transient, payload, now)
                .await?
        }
        "/organization/list-roles" => {
            service
                .list_organization_roles(
                    token,
                    transient,
                    query.get("organizationId").and_then(Value::as_str),
                    now,
                )
                .await?
        }
        "/organization/get-role" => {
            service
                .get_organization_role(token, transient, &serde_json::json!(query), now)
                .await?
        }
        _ => return Ok(None),
    }))
}

pub(super) fn validate_body(path: &str, body: Option<&Value>) -> BlueprintResult<()> {
    if path != "/organization/create-role" {
        return mutation_validation::validate_body(path, body);
    }
    if !body.is_some_and(Value::is_object) {
        return Err(validation(format!(
            "[body] Invalid input: expected object, received {}",
            received(body)
        )));
    }
    let body = body.expect("validated object");
    let mut errors = Vec::new();
    for (field, kind, required) in [
        ("organizationId", "string", false),
        ("role", "string", true),
        ("permission", "record", true),
        ("additionalFields", "object", false),
    ] {
        let value = body.get(field);
        if value.is_none() && !required {
            continue;
        }
        let valid = value.is_some_and(|v| {
            if kind == "string" {
                v.is_string()
            } else {
                v.is_object()
            }
        });
        if !valid {
            errors.push(format!(
                "[body.{field}] Invalid input: expected {kind}, received {}",
                received(value)
            ));
        } else if field == "permission" {
            for (resource, actions) in value
                .and_then(Value::as_object)
                .expect("validated record")
                .iter()
                .filter(|(key, _)| key.as_str() != "__proto__")
            {
                if let Some(actions) = actions.as_array() {
                    for (i, action) in actions.iter().enumerate() {
                        if !action.is_string() {
                            errors.push(format!("[body.permission.{resource}.{i}] Invalid input: expected string, received {}", received(Some(action))));
                        }
                    }
                } else {
                    errors.push(format!(
                        "[body.permission.{resource}] Invalid input: expected array, received {}",
                        received(Some(actions))
                    ));
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(validation(errors.join("; ")))
    }
}
