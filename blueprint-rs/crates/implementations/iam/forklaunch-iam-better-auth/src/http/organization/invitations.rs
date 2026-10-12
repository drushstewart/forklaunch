use super::validation::{received, validation};
use super::{AuthStore, BlueprintResult, DateTime, HashMap, OrganizationService, Utc, Value};
pub(in crate::http) fn is_route(path: &str) -> bool {
    matches!(
        path,
        "/organization/get-invitation"
            | "/organization/list-invitations"
            | "/organization/list-user-invitations"
    )
}
pub(super) fn validate_query(path: &str, query: &HashMap<String, Value>) -> BlueprintResult<()> {
    let (field, required) = match path {
        "/organization/get-invitation" => ("id", true),
        "/organization/list-invitations" => ("organizationId", false),
        "/organization/list-user-invitations" => ("email", false),
        _ => return Ok(()),
    };
    let value = query.get(field);
    if value.is_some_and(|v| !v.is_string()) || required && value.is_none() {
        return Err(validation(format!(
            "[query.{field}] Invalid input: expected string, received {}",
            received(value)
        )));
    }
    Ok(())
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
        "/organization/get-invitation" => {
            service
                .get_invitation(token, transient, query["id"].as_str().unwrap_or(""), now)
                .await?
        }
        "/organization/list-invitations" => {
            service
                .list_invitations(
                    token,
                    transient,
                    query.get("organizationId").and_then(Value::as_str),
                    now,
                )
                .await?
        }
        "/organization/list-user-invitations" => {
            if query
                .get("email")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty())
            {
                return Err(super::BlueprintError::new(
                    "HTTP_MESSAGE_ONLY",
                    "User email cannot be passed for client side API calls.",
                ));
            }
            service
                .list_user_invitations(token, transient, None, now)
                .await?
        }
        _ => return Ok(None),
    }))
}
