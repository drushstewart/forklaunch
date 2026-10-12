//! Membership schema ordering and raw HTTP dispatch.
use super::validation::{received, validation};
use super::{
    AuthStore, BlueprintResult, DateTime, HashMap, OrganizationService, Utc, Value, internal,
};
pub(super) fn is_route(path: &str) -> bool {
    matches!(
        path,
        "/organization/get-active-member"
            | "/organization/get-active-member-role"
            | "/organization/leave"
            | "/organization/update-member-role"
            | "/organization/remove-member"
    )
}
pub(super) async fn handle<S: AuthStore>(
    service: &OrganizationService<S>,
    path: &str,
    body: &Value,
    query: &HashMap<String, Value>,
    session: (&str, bool),
    now: DateTime<Utc>,
) -> BlueprintResult<Option<Value>> {
    let (token, transient) = session;
    Ok(Some(match path {
        "/organization/get-active-member" => service.active_member(token, transient, now).await?,
        "/organization/get-active-member-role" => {
            service
                .member_role(token, transient, &serde_json::json!(query), now)
                .await?
        }
        "/organization/leave" => {
            service
                .leave(
                    token,
                    transient,
                    body["organizationId"].as_str().ok_or_else(internal)?,
                    now,
                )
                .await?
        }
        "/organization/update-member-role" => {
            service
                .update_member_role(token, transient, body, now)
                .await?
        }
        "/organization/remove-member" => service.remove_member(token, transient, body, now).await?,
        _ => return Ok(None),
    }))
}
pub(super) fn validate_body(path: &str, body: Option<&Value>) -> BlueprintResult<()> {
    if matches!(
        path,
        "/organization/get-active-member" | "/organization/get-active-member-role"
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
    let mut errors = Vec::new();
    let fields: &[(&str, bool)] = match path {
        "/organization/leave" => &[("organizationId", true)],
        "/organization/remove-member" => &[("memberIdOrEmail", true), ("organizationId", false)],
        "/organization/update-member-role" => {
            if !body.get("role").is_some_and(|role| {
                role.is_string()
                    || role
                        .as_array()
                        .is_some_and(|roles| roles.iter().all(Value::is_string))
            }) {
                errors.push("[body.role] Invalid input".into());
            }
            &[("memberId", true), ("organizationId", false)]
        }
        _ => return Err(internal()),
    };
    for (field, required) in fields {
        let value = body.get(*field);
        if (*required || value.is_some()) && !value.is_some_and(Value::is_string) {
            errors.push(format!(
                "[body.{field}] Invalid input: expected string, received {}",
                received(value)
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(validation(errors.join("; ")))
    }
}
pub(super) fn validate_query(path: &str, query: &HashMap<String, Value>) -> BlueprintResult<()> {
    if path != "/organization/get-active-member-role" {
        return Ok(());
    }
    let mut errors = Vec::new();
    for field in ["userId", "organizationId", "organizationSlug"] {
        if let Some(value) = query.get(field)
            && !value.is_string()
        {
            errors.push(format!(
                "[query.{field}] Invalid input: expected string, received {}",
                received(Some(value))
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(validation(errors.join("; ")))
    }
}
