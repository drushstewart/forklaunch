//! Stored dynamic roles and explicit access-control grants.
mod create;
pub(super) mod limit;
mod mutation;
use super::permission::allowed;
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, json, text, unauthorized,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Lists the first 100 stored dynamic roles, parsing permission JSON.
    /// # Errors
    /// Requires scoped raw membership and ac:read, with authoritative sessions.
    pub async fn list_organization_roles(
        &self,
        token: &str,
        transient: bool,
        org: Option<&str>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.session(token, transient, now).await?;
        let mut tx = self.store.begin(true).await?;
        let view = self
            .recheck(&mut tx, token, now)
            .await?
            .ok_or_else(unauthorized)?;
        let org = scope(org, &view.session)?;
        authorize(
            &self.access_control,
            &mut tx,
            &org,
            text(&view.user, "id")?,
            true,
        )
        .await?;
        let roles =
            crate::store::find_all(&mut tx, Model::OrganizationRole, "organizationId", &org)
                .await?
                .into_iter()
                .filter(|row| row["organizationId"] == org)
                .take(100)
                .map(public)
                .collect::<BlueprintResult<Vec<_>>>()?;
        tx.commit().await?;
        Ok(json!(roles))
    }
    /// Reads a stored role by exact name or ID; name has selection precedence.
    /// # Errors
    /// Requires scoped membership/ac:read and an existing role with valid JSON.
    pub async fn get_organization_role(
        &self,
        token: &str,
        transient: bool,
        query: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.session(token, transient, now).await?;
        let mut tx = self.store.begin(true).await?;
        let view = self
            .recheck(&mut tx, token, now)
            .await?
            .ok_or_else(unauthorized)?;
        let org = scope(query["organizationId"].as_str(), &view.session)?;
        authorize(
            &self.access_control,
            &mut tx,
            &org,
            text(&view.user, "id")?,
            false,
        )
        .await?;
        let (field, key) = if let Some(name) = query["roleName"].as_str().filter(|s| !s.is_empty())
        {
            ("role", name)
        } else {
            (
                "id",
                query["roleId"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(not_found)?,
            )
        };
        let role = crate::store::find_all(&mut tx, Model::OrganizationRole, "organizationId", &org)
            .await?
            .into_iter()
            .find(|row| row["organizationId"] == org && row[field] == key)
            .ok_or_else(not_found)?;
        let role = public(role)?;
        tx.commit().await?;
        Ok(role)
    }
}
fn scope(explicit: Option<&str>, session: &Value) -> BlueprintResult<String> {
    explicit
        .or_else(|| session["activeOrganizationId"].as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| BlueprintError::new("NO_ACTIVE_ORGANIZATION", "No active organization"))
}
async fn authorize<T: AuthTransaction>(
    config: &crate::organizations::Authorization,
    tx: &mut T,
    org: &str,
    user: &str,
    list: bool,
) -> BlueprintResult<()> {
    let member = crate::store::find_all(tx, Model::Member, "userId", user)
        .await?
        .into_iter()
        .find(|row| row["organizationId"] == org && row["userId"] == user)
        .ok_or_else(|| {
            BlueprintError::new(
                "YOU_ARE_NOT_A_MEMBER_OF_THIS_ORGANIZATION",
                "You are not a member of this organization",
            )
        })?;
    if !allowed(
        config,
        tx,
        org,
        text(&member, "role")?,
        Some(&json!({"ac":["read"]})),
    )
    .await?
    {
        return Err(if list {
            BlueprintError::new(
                "YOU_ARE_NOT_ALLOWED_TO_LIST_A_ROLE",
                "You are not allowed to list a role",
            )
        } else {
            BlueprintError::new(
                "YOU_ARE_NOT_ALLOWED_TO_READ_A_ROLE",
                "You are not allowed to read a role",
            )
        });
    }
    Ok(())
}
fn not_found() -> BlueprintError {
    BlueprintError::new("ROLE_NOT_FOUND", "Role not found")
}
fn public(mut role: Value) -> BlueprintResult<Value> {
    role["permission"] = role["permission"]
        .as_str()
        .and_then(|text| crate::ecmascript::parse(text).ok())
        .ok_or_else(|| BlueprintError::new("HTTP_INTERNAL_EMPTY", "Invalid role permissions"))?;
    Ok(role)
}
