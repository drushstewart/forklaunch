//! Atomic dynamic-role admission, resource and per-action grant checks.
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, allowed, json, scope, text, unauthorized,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Creates a lowercased stored role with the configured platform permissions.
    /// # Errors
    /// Requires scoped raw membership, ac:create and every requested action.
    pub async fn create_organization_role(
        &self,
        token: &str,
        transient: bool,
        body: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.session(token, transient, now).await?;
        let mut tx = self.store.begin(true).await?;
        let view = self
            .recheck(&mut tx, token, now)
            .await?
            .ok_or_else(unauthorized)?;
        let org = scope(body["organizationId"].as_str(), &view.session).map_err(|_| {
            BlueprintError::new(
                "YOU_MUST_BE_IN_AN_ORGANIZATION_TO_CREATE_A_ROLE",
                "You must be in an organization to create a role",
            )
        })?;
        let name = text(body, "role")?.to_lowercase();
        check_predefined(&name)?;
        let member = crate::store::find_all(&mut tx, Model::Member, "organizationId", &org)
            .await?
            .into_iter()
            .find(|row| row["organizationId"] == org && row["userId"] == view.user["id"])
            .ok_or_else(|| {
                BlueprintError::new(
                    "YOU_ARE_NOT_A_MEMBER_OF_THIS_ORGANIZATION",
                    "You are not a member of this organization",
                )
            })?;
        let role = text(&member, "role")?;
        if !allowed(&mut tx, &org, role, Some(&json!({"ac":["create"]}))).await? {
            return Err(denied("create"));
        }
        let permission = &body["permission"];
        check_permissions(&mut tx, &org, role, permission, "create").await?;
        if crate::store::find_all(&mut tx, Model::OrganizationRole, "organizationId", &org)
            .await?
            .iter()
            .any(|row| row["organizationId"] == org && row["role"] == name)
        {
            return Err(taken());
        }
        let mut row = json!({"id":uuid::Uuid::new_v4().to_string(), "organizationId":org, "role":name, "permission":crate::ecmascript::stringify(permission), "createdAt":super::super::timestamp(now)});
        tx.insert(Model::OrganizationRole, row.clone()).await?;
        tx.commit().await?;
        row["permission"] = permission.clone();
        Ok(json!({"success":true,"roleData":row,"statements":permission}))
    }
}
pub(super) fn denied(action: &str) -> BlueprintError {
    BlueprintError::new(
        format!("YOU_ARE_NOT_ALLOWED_TO_{}_A_ROLE", action.to_uppercase()),
        format!("You are not allowed to {action} a role"),
    )
}
pub(super) fn taken() -> BlueprintError {
    BlueprintError::new(
        "ROLE_NAME_IS_ALREADY_TAKEN",
        "That role name is already taken",
    )
}
pub(super) fn check_predefined(name: &str) -> BlueprintResult<()> {
    if ["owner", "admin", "editor", "viewer", "system"].contains(&name) {
        Err(taken())
    } else {
        Ok(())
    }
}
pub(super) async fn check_permissions<T: AuthTransaction>(
    tx: &mut T,
    org: &str,
    role: &str,
    permission: &Value,
    action: &str,
) -> BlueprintResult<()> {
    let fields = permission.as_object().ok_or_else(super::super::internal)?;
    if fields.keys().any(|key| key != "platform") {
        return Err(BlueprintError::new(
            "INVALID_RESOURCE",
            "The provided permission includes an invalid resource",
        ));
    }
    let mut missing = Vec::new();
    for (resource, actions) in fields {
        for action in actions.as_array().ok_or_else(super::super::internal)? {
            if !allowed(tx, org, role, Some(&json!({resource:[action]}))).await? {
                missing.push(format!(
                    "{resource}:{}",
                    action.as_str().ok_or_else(super::super::internal)?
                ));
            }
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(denied(action).with_detail("missingPermissions", json!(missing)))
    }
}
