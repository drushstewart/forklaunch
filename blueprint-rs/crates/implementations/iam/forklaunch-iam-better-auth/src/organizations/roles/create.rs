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
        self.organization_features.require_roles()?;
        if let Some(hook) = &self.role_limit {
            return self
                .create_role_with_limit(token, transient, body, now, hook.as_ref())
                .await;
        }
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
        check_predefined(&self.access_control, &name)?;
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
        if !allowed(
            &self.access_control,
            &mut tx,
            &org,
            role,
            Some(&json!({"ac":["create"]})),
        )
        .await?
        {
            return Err(denied("create"));
        }
        if let Some(limit) = self.controls.maximum_roles {
            let count = tx
                .find_window(Model::OrganizationRole, "organizationId", &org, 0, 1)
                .await?
                .total;
            if u64::try_from(count).unwrap_or(u64::MAX) >= u64::from(limit) {
                return Err(BlueprintError::new(
                    "TOO_MANY_ROLES",
                    "This organization has too many roles",
                ));
            }
        }
        let permission = &body["permission"];
        check_permissions(
            &self.access_control,
            &mut tx,
            &org,
            role,
            permission,
            "create",
        )
        .await?;
        let permission = super::super::permission::admitted_record(permission);
        if crate::store::find_all(&mut tx, Model::OrganizationRole, "organizationId", &org)
            .await?
            .iter()
            .any(|row| row["organizationId"] == org && row["role"] == name)
        {
            return Err(taken());
        }
        let mut row = json!({"id":uuid::Uuid::new_v4().to_string(), "organizationId":org, "role":name, "permission":crate::ecmascript::stringify(&permission), "createdAt":super::super::timestamp(now)});
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
pub(super) fn check_predefined(
    config: &crate::organizations::OrganizationAccessControl,
    name: &str,
) -> BlueprintResult<()> {
    if config.predefined(name) {
        Err(taken())
    } else {
        Ok(())
    }
}
pub(super) async fn check_permissions<T: AuthTransaction>(
    config: &crate::organizations::Authorization,
    tx: &mut T,
    org: &str,
    role: &str,
    permission: &Value,
    action: &str,
) -> BlueprintResult<()> {
    let fields = permission.as_object().ok_or_else(super::super::internal)?;
    if fields
        .keys()
        .any(|key| key != "__proto__" && !config.resource(key))
    {
        return Err(BlueprintError::new(
            "INVALID_RESOURCE",
            "The provided permission includes an invalid resource",
        ));
    }
    let mut missing = Vec::new();
    for (resource, actions) in super::super::permission::own_entries(fields)
        .into_iter()
        .filter(|(key, _)| key.as_str() != "__proto__")
    {
        for action in actions.as_array().ok_or_else(super::super::internal)? {
            if !allowed(config, tx, org, role, Some(&json!({resource:[action]}))).await? {
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
