use super::validation::{received, validation};
use super::{AuthStore, BlueprintResult, DateTime, HashMap, OrganizationService, Utc, Value};
pub(super) fn is_route(path: &str) -> bool {
    matches!(
        path,
        "/organization/list-teams"
            | "/organization/list-user-teams"
            | "/organization/list-team-members"
            | "/organization/set-active-team"
    )
}
pub(super) async fn handle<S: AuthStore>(
    service: &OrganizationService<S>,
    path: &str,
    query: &HashMap<String, Value>,
    session: (&str, bool),
    now: DateTime<Utc>,
) -> BlueprintResult<Option<Value>> {
    let (token, transient) = session;
    let query = serde_json::json!(query);
    Ok(Some(match path {
        "/organization/list-teams" => service.list_teams(token, transient, &query, now).await?,
        "/organization/list-user-teams" => {
            service
                .list_user_teams(token, transient, &query, now)
                .await?
        }
        "/organization/list-team-members" => {
            service
                .list_team_members(token, transient, &query, now)
                .await?
        }
        _ => return Ok(None),
    }))
}
pub(super) fn validate_body(path: &str, body: Option<&Value>) -> BlueprintResult<()> {
    if path != "/organization/set-active-team" {
        return Ok(());
    }
    if !body.is_some_and(Value::is_object) {
        return Err(validation(format!(
            "[body] Invalid input: expected object, received {}",
            received(body)
        )));
    }
    if let Some(value) = body.and_then(|body| body.get("teamId"))
        && !value.is_null()
        && !value.is_string()
    {
        return Err(validation(format!(
            "[body.teamId] Invalid input: expected string, received {}",
            received(Some(value))
        )));
    }
    Ok(())
}
pub(super) fn validate_query(path: &str, query: &HashMap<String, Value>) -> BlueprintResult<()> {
    let fields: &[&str] = match path {
        "/organization/list-teams" => &["organizationId"],
        "/organization/list-user-teams" => &["userId", "organizationId"],
        "/organization/list-team-members" => &["teamId"],
        _ => return Ok(()),
    };
    let mut errors = Vec::new();
    for field in fields {
        if let Some(value) = query.get(*field)
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
