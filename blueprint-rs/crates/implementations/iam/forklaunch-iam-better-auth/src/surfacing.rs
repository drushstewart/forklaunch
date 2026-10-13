//! Configured application role/permission surfacing from the newest active
//! organization session. Permissions are stored opaque strings, not expanded
//! or unioned authorization grants.
use crate::store::{AuthStore, AuthTransaction, Model};
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::BlueprintResult;
use serde_json::Value;
use std::{cmp::Ordering, sync::Arc};

pub struct SurfacingService<S> {
    store: Arc<S>,
}
impl<S: AuthStore> SurfacingService<S> {
    #[must_use]
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }
    /// # Errors
    /// Returns trusted store failures without selecting a different tenant.
    pub async fn surface_role(&self, user_id: &str, now: DateTime<Utc>) -> BlueprintResult<Value> {
        let mut tx = self.store.begin(false).await?;
        let organization = active_organization(&mut tx, user_id, now).await?;
        let role = role(&mut tx, user_id, &organization).await?;
        tx.commit().await?;
        Ok(role)
    }
    /// # Errors
    /// Returns trusted store failures. Permission JSON remains opaque.
    pub async fn surface_permissions(
        &self,
        user_id: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Vec<Value>> {
        let mut tx = self.store.begin(false).await?;
        let organization = active_organization(&mut tx, user_id, now).await?;
        let role = role(&mut tx, user_id, &organization).await?;
        let permissions = if crate::keyring::verify::truthy(&organization)
            && crate::keyring::verify::truthy(&role)
        {
            crate::store::find_all_value(
                &mut tx,
                Model::OrganizationRole,
                "organizationId",
                &organization,
            )
            .await?
            .into_iter()
            .filter(|row| {
                row.get("organizationId") == Some(&organization) && row.get("role") == Some(&role)
            })
            .map(|row| row.get("permission").cloned().unwrap_or(Value::Null))
            .collect()
        } else {
            vec![]
        };
        tx.commit().await?;
        Ok(permissions)
    }
}
async fn active_organization<T: AuthTransaction>(
    tx: &mut T,
    user_id: &str,
    now: DateTime<Utc>,
) -> BlueprintResult<Value> {
    let mut sessions: Vec<_> = crate::store::find_all(tx, Model::Session, "userId", user_id)
        .await?
        .into_iter()
        .filter(|row| {
            row.get("userId").and_then(Value::as_str) == Some(user_id)
                && row
                    .get("activeOrganizationId")
                    .is_some_and(|value| !value.is_null())
                && instant(row.get("expiresAt"))
                    .is_some_and(|expiry| expiry > now.timestamp_millis())
        })
        .collect();
    sessions.sort_by(|left, right| {
        match (
            instant(left.get("createdAt")),
            instant(right.get("createdAt")),
        ) {
            (Some(left), Some(right)) => right.cmp(&left),
            _ => Ordering::Equal,
        }
    });
    Ok(sessions
        .first()
        .and_then(|row| row.get("activeOrganizationId"))
        .cloned()
        .unwrap_or(Value::Null))
}
async fn role<T: AuthTransaction>(
    tx: &mut T,
    user_id: &str,
    organization: &Value,
) -> BlueprintResult<Value> {
    if !crate::keyring::verify::truthy(organization) {
        return Ok(Value::Null);
    }
    Ok(crate::store::find_all(tx, Model::Member, "userId", user_id)
        .await?
        .into_iter()
        .find(|row| {
            row.get("userId").and_then(Value::as_str) == Some(user_id)
                && row.get("organizationId") == Some(organization)
        })
        .and_then(|row| row.get("role").cloned())
        .unwrap_or(Value::Null))
}
fn instant(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::String(value) => DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|value| value.timestamp_millis()),
        Value::Number(value) => value.as_i64(),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests;
