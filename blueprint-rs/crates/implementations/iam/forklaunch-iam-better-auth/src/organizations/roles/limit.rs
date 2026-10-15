//! Awaited role quota with current count/name reads and cached permission grants.
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, allowed, json, scope, text,
};
use std::{future::Future, pin::Pin};
/// Full IEEE callback result, including fractional values, NaN and infinities.
pub type RoleLimitFuture = Pin<Box<dyn Future<Output = BlueprintResult<f64>> + Send + 'static>>;
/// Trusted `maximumRolesPerOrganization` callback. The only argument is an owned
/// organization ID (at most 4096 UTF-8 bytes). No transaction spans the future.
/// Preparation and future errors become sanitized empty HTTP 500 responses;
/// Rust panic and JavaScript receiver/non-number coercion semantics are excluded.
pub trait RoleLimitPolicy: Send + Sync {
    /// # Errors
    /// A preparation error aborts creation before any role write.
    fn prepare(&self, organization_id: String) -> BlueprintResult<RoleLimitFuture>;
}
fn failure() -> BlueprintError {
    BlueprintError::new("HTTP_INTERNAL_EMPTY", "Role limit policy failed")
}
impl<S: AuthStore> OrganizationService<S> {
    pub(super) async fn create_role_with_limit(
        &self,
        token: &str,
        transient: bool,
        body: &Value,
        now: DateTime<Utc>,
        hook: &dyn RoleLimitPolicy,
    ) -> BlueprintResult<Value> {
        let view = self.session(token, transient, now).await?;
        let org = scope(body["organizationId"].as_str(), &view.session).map_err(|_| {
            BlueprintError::new(
                "YOU_MUST_BE_IN_AN_ORGANIZATION_TO_CREATE_A_ROLE",
                "You must be in an organization to create a role",
            )
        })?;
        let name = text(body, "role")?.to_lowercase();
        super::create::check_predefined(&self.access_control, &name)?;
        let mut tx = self.store.begin(false).await?;
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
        let role = text(&member, "role")?.to_owned();
        let slot = self.access_control.cache().reserve(&org)?;
        if !allowed(
            &self.access_control,
            &mut tx,
            &org,
            &role,
            Some(&json!({"ac":["create"]})),
        )
        .await?
        {
            return Err(super::create::denied("create"));
        }
        tx.commit().await?;
        let limit = hook
            .prepare(org.clone())
            .map_err(|_| failure())?
            .await
            .map_err(|_| failure())?;
        self.insert_after_limit(&org, &role, &name, &body["permission"], limit, now, &slot)
            .await
    }
    #[allow(clippy::too_many_arguments)] // Explicit admitted identity plus trusted cache guard.
    async fn insert_after_limit(
        &self,
        org: &str,
        role: &str,
        name: &str,
        permission: &Value,
        limit: f64,
        now: DateTime<Utc>,
        slot: &super::super::permission::cache::Slot,
    ) -> BlueprintResult<Value> {
        let mut tx = self.store.begin(true).await?;
        let count = tx
            .find_window(Model::OrganizationRole, "organizationId", org, 0, 1)
            .await?
            .total;
        // Upstream compares its Number count with the callback's Number. A native
        // usize count is rounded to binary64 in the same way, without a u32 cap.
        #[allow(clippy::cast_precision_loss)]
        let count = count as f64;
        if count >= limit {
            return Err(BlueprintError::new(
                "TOO_MANY_ROLES",
                "This organization has too many roles",
            ));
        }
        check_cached_permissions(&self.access_control, slot, role, permission)?;
        let permission = super::super::permission::admitted_record(permission);
        if crate::store::find_all(&mut tx, Model::OrganizationRole, "organizationId", org)
            .await?
            .iter()
            .any(|row| row["organizationId"] == org && row["role"] == name)
        {
            return Err(super::create::taken());
        }
        let now = self.time_source.sample(now);
        let mut row = json!({"id":uuid::Uuid::new_v4().to_string(),"organizationId":org,"role":name,"permission":crate::ecmascript::stringify(&permission),"createdAt":super::super::timestamp(now)});
        tx.insert(Model::OrganizationRole, row.clone()).await?;
        tx.commit().await?;
        row["permission"] = permission.clone();
        Ok(json!({"success":true,"roleData":row,"statements":permission}))
    }
}

fn check_cached_permissions(
    config: &crate::organizations::Authorization,
    slot: &super::super::permission::cache::Slot,
    role: &str,
    permission: &Value,
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
            if !super::super::permission::cached(slot, role, Some(&json!({resource:[action]})))? {
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
        Err(super::create::denied("create").with_detail("missingPermissions", json!(missing)))
    }
}
