//! Authorization bound to the exact verified user, organization and session.
//! Never infer scope from another session or cache current membership decisions.
use crate::store::{AuthStore, AuthTransaction, Model};
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::BlueprintResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// Construct from an already verified JWT/session payload, never untrusted claims.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_field_names)] // Exact configured authorization DTO field names.
pub struct AuthorizationScope {
    pub user_id: String,
    pub organization_id: String,
    pub session_id: String,
}
impl AuthorizationScope {
    #[must_use]
    pub fn from_payload(payload: &Value) -> Option<Self> {
        let scope = Self {
            user_id: payload.get("sub")?.as_str()?.to_owned(),
            organization_id: payload.get("activeOrganizationId")?.as_str()?.to_owned(),
            session_id: payload.get("sessionId")?.as_str()?.to_owned(),
        };
        scope.valid().then_some(scope)
    }
    #[must_use]
    pub fn valid(&self) -> bool {
        [&self.user_id, &self.organization_id, &self.session_id]
            .iter()
            .all(|value| {
                value.len() == 36
                    && value.bytes().enumerate().all(|(index, byte)| {
                        if [8, 13, 18, 23].contains(&index) {
                            byte == b'-'
                        } else {
                            byte.is_ascii_hexdigit()
                        }
                    })
            })
    }
}

pub struct SurfacingService<S> {
    store: Arc<S>,
}
impl<S: AuthStore> SurfacingService<S> {
    #[must_use]
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }
    /// Retired user-only lookup. No user ID can securely select a session.
    /// # Errors
    /// Never fails; retained for old callers to fail closed.
    #[allow(clippy::unused_async)] // Preserve the retired async API while failing closed without storage.
    pub async fn surface_role(
        &self,
        _user_id: &str,
        _now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        Ok(Value::Null)
    }
    /// Retired user-only lookup; use explicit authenticated scope instead.
    /// # Errors
    /// Never fails; retained for old callers to fail closed.
    #[allow(clippy::unused_async)] // Preserve the retired async API while failing closed without storage.
    pub async fn surface_permissions(
        &self,
        _user_id: &str,
        _now: DateTime<Utc>,
    ) -> BlueprintResult<Vec<Value>> {
        Ok(vec![])
    }
    /// # Errors
    /// Returns trusted storage failures. Invalid or inactive scopes return null.
    pub async fn surface_role_scoped(
        &self,
        scope: &AuthorizationScope,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        if !scope.valid() {
            return Ok(Value::Null);
        }
        let mut tx = self.store.begin(false).await?;
        let member = current_member(&mut tx, scope, now).await?;
        tx.commit().await?;
        Ok(member
            .and_then(|row| row.get("role").cloned())
            .unwrap_or(Value::Null))
    }
    /// Builtins retain their declared grants; custom roles retain opaque strings.
    /// # Errors
    /// Returns trusted storage failures. Invalid/inactive scopes return no grants.
    pub async fn surface_permissions_scoped(
        &self,
        scope: &AuthorizationScope,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Vec<Value>> {
        if !scope.valid() {
            return Ok(vec![]);
        }
        let mut tx = self.store.begin(false).await?;
        let Some(member) = current_member(&mut tx, scope, now).await? else {
            tx.commit().await?;
            return Ok(vec![]);
        };
        let role = member.get("role").cloned().unwrap_or(Value::Null);
        let permissions = match role.as_str() {
            Some("owner" | "admin" | "editor" | "system") => {
                vec![Value::from("platform:read"), Value::from("platform:write")]
            }
            Some("viewer") => vec![Value::from("platform:read")],
            _ => crate::store::find_all(
                &mut tx,
                Model::OrganizationRole,
                "organizationId",
                &scope.organization_id,
            )
            .await?
            .into_iter()
            .filter(|row| {
                row.get("organizationId").and_then(Value::as_str)
                    == Some(scope.organization_id.as_str())
                    && row.get("role") == Some(&role)
            })
            .map(|row| row.get("permission").cloned().unwrap_or(Value::Null))
            .collect(),
        };
        tx.commit().await?;
        Ok(permissions)
    }
}
async fn current_member<T: AuthTransaction>(
    tx: &mut T,
    scope: &AuthorizationScope,
    now: DateTime<Utc>,
) -> BlueprintResult<Option<Value>> {
    // Session primary key and user FK are SQL UUIDs; organization/member fields
    // are opaque strings and retain exact comparisons below.
    let Some(session) = tx
        .get(Model::Session, &scope.session_id.to_ascii_lowercase())
        .await?
    else {
        return Ok(None);
    };
    if session
        .get("userId")
        .and_then(Value::as_str)
        .is_none_or(|id| !id.eq_ignore_ascii_case(&scope.user_id))
        || session.get("activeOrganizationId").and_then(Value::as_str)
            != Some(scope.organization_id.as_str())
        || instant(session.get("expiresAt")).is_none_or(|expiry| expiry <= now.timestamp_millis())
    {
        return Ok(None);
    }
    Ok(
        crate::store::find_all(tx, Model::Member, "userId", &scope.user_id)
            .await?
            .into_iter()
            .find(|row| {
                row.get("userId").and_then(Value::as_str) == Some(scope.user_id.as_str())
                    && row.get("organizationId").and_then(Value::as_str)
                        == Some(scope.organization_id.as_str())
            }),
    )
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
