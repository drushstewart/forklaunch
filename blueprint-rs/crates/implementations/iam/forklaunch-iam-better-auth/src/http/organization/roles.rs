use super::validation::{received, validation};
use super::{AuthStore, BlueprintResult, DateTime, HashMap, OrganizationService, Utc, Value};
pub(super) fn is_route(path: &str) -> bool {
    matches!(path, "/organization/list-roles" | "/organization/get-role")
}
pub(super) fn validate_query(path: &str, query: &HashMap<String, Value>) -> BlueprintResult<()> {
    if !is_route(path) {
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
    query: &HashMap<String, Value>,
    session: (&str, bool),
    now: DateTime<Utc>,
) -> BlueprintResult<Option<Value>> {
    let (token, transient) = session;
    Ok(Some(match path {
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
