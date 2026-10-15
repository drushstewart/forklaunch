//! Stored dynamic grants extend the trusted static role map.
use super::{AuthTransaction, BlueprintError, BlueprintResult, Model, Value, text};
use serde_json::value::RawValue;
use std::collections::BTreeMap;

pub(super) mod cache;
mod grants;
use grants::{Actions, Grants};
type Statements = BTreeMap<String, Vec<String>>;
pub(super) async fn allowed<T: AuthTransaction>(
    config: &super::Authorization,
    tx: &mut T,
    organization: &str,
    roles: &str,
    requested: Option<&Value>,
) -> BlueprintResult<bool> {
    allowed_with_creator(config, tx, organization, roles, requested, None).await
}
pub(super) async fn allowed_with_creator<T: AuthTransaction>(
    config: &super::Authorization,
    tx: &mut T,
    organization: &str,
    roles: &str,
    requested: Option<&Value>,
    creator_role: Option<&str>,
) -> BlueprintResult<bool> {
    let mut grants = Grants::new(config.roles());
    if config.dynamic_enabled {
        for row in
            crate::store::find_all(tx, Model::OrganizationRole, "organizationId", organization)
                .await?
                .into_iter()
                .filter(|row| row["organizationId"] == organization)
                .take(100)
        {
            let role = text(&row, "role")?;
            let permission = parse_permissions(&row["permission"], role)?;
            grants.merge(role, permission)?;
        }
    }
    let grants = std::sync::Arc::new(grants);
    config
        .cache()
        .publish(organization, config, grants.clone())?;
    evaluate(config, &grants, roles, requested, creator_role)
}
pub(super) fn cached(
    slot: &cache::Slot,
    roles: &str,
    requested: Option<&Value>,
) -> BlueprintResult<bool> {
    let snapshot = slot.snapshot()?;
    evaluate(&snapshot.config, &snapshot.grants, roles, requested, None)
}
fn evaluate(
    config: &super::Authorization,
    grants: &Grants,
    roles: &str,
    requested: Option<&Value>,
    creator_role: Option<&str>,
) -> BlueprintResult<bool> {
    let Some(requested) = requested.and_then(Value::as_object) else {
        return Ok(false);
    };
    if creator_role.is_some_and(|creator| roles.split(',').any(|role| role == creator)) {
        return Ok(true);
    }
    for role in roles.split(',') {
        if let Some(statements) = grants.get(role)? {
            let custom = if grants.replaced(role) {
                None
            } else {
                config.custom(role, requested)
            };
            let accepted = match custom {
                Some(result) => result?,
                None => authorize(statements, requested)?,
            };
            if accepted {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn authorize(
    statements: &BTreeMap<String, Actions>,
    requested: &serde_json::Map<String, Value>,
) -> BlueprintResult<bool> {
    let mut present = false;
    for (resource, actions) in own_entries(requested)
        .into_iter()
        .filter(|(key, _)| key.as_str() != "__proto__")
    {
        let Some(actions) = actions.as_array().filter(|actions| !actions.is_empty()) else {
            return Ok(false);
        };
        let Some(allowed) = statements.get(resource) else {
            if prototype_name(resource) {
                return Err(empty_error());
            }
            return Ok(false);
        };
        if !actions.iter().all(|action| {
            action
                .as_str()
                .is_some_and(|action| allowed.includes(action))
        }) {
            return Ok(false);
        }
        present = true;
    }
    Ok(present)
}
pub(super) fn own_entries(fields: &serde_json::Map<String, Value>) -> Vec<(&String, &Value)> {
    let mut entries: Vec<_> = fields.iter().collect();
    // Stable sort preserves insertion order among non-index string keys.
    entries.sort_by_key(|(key, _)| {
        key.parse::<u32>()
            .ok()
            .filter(|index| *index != u32::MAX && index.to_string() == key.as_str())
            .map_or((1, 0), |index| (0, index))
    });
    entries
}
fn prototype_name(value: &str) -> bool {
    matches!(
        value,
        "__proto__"
            | "constructor"
            | "toString"
            | "toLocaleString"
            | "valueOf"
            | "hasOwnProperty"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "__defineGetter__"
            | "__defineSetter__"
            | "__lookupGetter__"
            | "__lookupSetter__"
    )
}
/// Zod record admission ignores this special key without validating its value.
/// Call after validating the remaining permission record, before persistence.
pub(super) fn admitted_record(value: &Value) -> Value {
    value.as_object().map_or_else(
        || value.clone(),
        |fields| {
            Value::Object(
                fields
                    .iter()
                    .filter(|(key, _)| key.as_str() != "__proto__")
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            )
        },
    )
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
        .filter(|(resource, _)| resource != "__proto__")
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
    crate::error_origin::handled(BlueprintError::new(
        "HTTP_INTERNAL_MESSAGE_ONLY",
        format!("Invalid permissions for role {role}"),
    ))
}
