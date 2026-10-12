//! Stored dynamic grants extend the configured platform-only static roles.
use super::{AuthTransaction, BlueprintError, BlueprintResult, Model, Value, text};
use serde_json::value::RawValue;
use std::collections::BTreeMap;

type Statements = BTreeMap<String, Vec<String>>;
pub(super) async fn allowed<T: AuthTransaction>(
    tx: &mut T,
    organization: &str,
    roles: &str,
    requested: Option<&Value>,
) -> BlueprintResult<bool> {
    allowed_with_creator(tx, organization, roles, requested, false).await
}
pub(super) async fn allowed_with_creator<T: AuthTransaction>(
    tx: &mut T,
    organization: &str,
    roles: &str,
    requested: Option<&Value>,
    creator_all: bool,
) -> BlueprintResult<bool> {
    let mut grants: BTreeMap<String, Statements> = BTreeMap::new();
    for role in ["owner", "admin", "editor", "system", "viewer"] {
        let mut actions = vec!["platform:read".to_owned()];
        if role != "viewer" {
            actions.push("platform:write".to_owned());
        }
        grants.insert(
            role.to_owned(),
            BTreeMap::from([("platform".to_owned(), actions)]),
        );
    }
    for row in tx
        .list(Model::OrganizationRole)
        .await?
        .into_iter()
        .filter(|row| row["organizationId"] == organization)
        .take(100)
    {
        let role = text(&row, "role")?;
        let permission = parse_permissions(&row["permission"], role)?;
        let current = grants.entry(role.to_owned()).or_default();
        for (resource, actions) in permission {
            let merged = current.entry(resource).or_default();
            for action in actions {
                if !merged.contains(&action) {
                    merged.push(action);
                }
            }
        }
    }
    let Some(requested) = requested.and_then(Value::as_object) else {
        return Ok(false);
    };
    if creator_all && roles.split(',').any(|role| role == "owner") {
        return Ok(true);
    }
    if requested.is_empty() {
        return Ok(false);
    }
    Ok(roles.split(',').any(|role| {
        grants.get(role).is_some_and(|statements| {
            requested.iter().all(|(resource, actions)| {
                let Some(actions) = actions.as_array().filter(|actions| !actions.is_empty()) else {
                    return false;
                };
                statements.get(resource).is_some_and(|allowed| {
                    actions.iter().all(|action| {
                        action
                            .as_str()
                            .is_some_and(|action| allowed.iter().any(|allowed| allowed == action))
                    })
                })
            })
        })
    }))
}
fn parse_permissions(value: &Value, role: &str) -> BlueprintResult<Statements> {
    let encoded = match value {
        Value::String(value) => value.clone(),
        Value::Object(_) | Value::Array(_) => return Err(empty_error()),
        _ => value.to_string(),
    };
    let raw: Box<RawValue> = serde_json::from_str(&encoded).map_err(|_| empty_error())?;
    // Raw values preserve JavaScript's overflow-number parsing: syntactically
    // valid JSON numbers still fail the permission schema with its own error.
    let fields: BTreeMap<String, Box<RawValue>> =
        serde_json::from_str(raw.get()).map_err(|_| invalid(role))?;
    fields
        .into_iter()
        .map(|(resource, actions)| {
            serde_json::from_str(actions.get())
                .map(|actions| (resource, actions))
                .map_err(|_| invalid(role))
        })
        .collect()
}
fn empty_error() -> BlueprintError {
    BlueprintError::new("HTTP_INTERNAL_EMPTY", "Invalid stored permissions")
}
fn invalid(role: &str) -> BlueprintError {
    BlueprintError::new(
        "HTTP_INTERNAL_MESSAGE_ONLY",
        format!("Invalid permissions for role {role}"),
    )
}
