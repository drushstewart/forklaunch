//! Configured organization lifecycle over tenant-bound authoritative records.
use crate::{
    sessions::{SessionService, SessionView},
    store::{AuthStore, AuthTransaction, Model},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
#[cfg(feature = "postgres")]
pub(crate) use members::query_number;
use serde_json::{Value, json};
use std::sync::Arc;

mod access_control;
mod authorizers;
pub(crate) use authorizers::Authorization;
pub use authorizers::{
    OrganizationAuthorizationRequest, OrganizationRoleAuthorizer, OrganizationRoleAuthorizers,
};
mod create;
mod creation_policy;
pub use access_control::{OrganizationAccessControl, OrganizationRoles, OrganizationStatements};
pub use creation_policy::{
    OrganizationCreationContext, OrganizationCreationFuture, OrganizationCreationPolicy,
};
mod delivery;
pub use delivery::{InvitationDeliveryContext, InvitationEmailDelivery};
mod controls;
mod creator_role;
pub use creator_role::CreatorRole;
mod full;
mod invitation_policy;
mod team_callbacks;
mod team_policy;
pub use controls::OrganizationControls;
pub use team_callbacks::{
    OrganizationAdapterFuture, OrganizationCallbackAdapter, TeamCountLimitContext,
    TeamCountLimitPolicy, TeamLimitFuture, TeamMemberLimitContext, TeamMemberLimitPolicy,
    TeamPolicySession,
};
pub use team_policy::{TeamPolicy, TeamSeatQueryProfile};
mod invitations;
pub use invitation_policy::InvitationPolicy;
pub use invitations::create::limit::{
    InvitationLimitContext, InvitationLimitFuture, InvitationLimitPolicy,
};
mod members;
mod membership_policy;
pub use membership_policy::{MembershipLimitContext, MembershipLimitFuture, MembershipLimitPolicy};
mod mutation;
mod permission;
pub use permission::cache::OrganizationPermissionCache;
mod policy;
pub use policy::OrganizationPolicy;
mod roles;
pub use roles::limit::{RoleLimitFuture, RoleLimitPolicy};
mod selection;
mod teams;
pub use members::{AddMemberInput, MemberRoles};
#[cfg(test)]
pub(crate) mod tests;

pub struct OrganizationService<S> {
    policy: OrganizationPolicy,
    allow_creation: Option<Arc<dyn OrganizationCreationPolicy>>,
    creation_limit: Option<Arc<dyn OrganizationCreationPolicy>>,
    membership_limit: Option<Arc<dyn MembershipLimitPolicy>>,
    role_limit: Option<Arc<dyn RoleLimitPolicy>>,
    invitation_policy: InvitationPolicy,
    invitation_limit: Option<Arc<dyn InvitationLimitPolicy>>,
    team_count_limit: Option<Arc<dyn TeamCountLimitPolicy>>,
    team_member_limit: Option<Arc<dyn TeamMemberLimitPolicy>>,
    team_policy: TeamPolicy,
    team_seat_query_profile: TeamSeatQueryProfile,
    controls: OrganizationControls,
    creator_role: CreatorRole,
    access_control: Authorization,
    invitation_delivery: Option<Arc<dyn InvitationEmailDelivery>>,
    background_dispatcher: Option<Arc<dyn crate::background::AuthBackgroundDispatcher>>,
    cancel_pending_invitations_on_reinvite: bool,
    time_source: crate::time_source::TimeSource,
    session_policy: crate::sessions::SessionPolicy,
    store: Arc<S>,
}
/// The selection endpoint emits a session cookie only after an actual selection
/// or clearing an existing selection; failed membership checks do not emit it.
pub struct OrganizationSelection {
    pub body: Value,
    pub cookie: bool,
}
impl<S: AuthStore> OrganizationService<S> {
    #[cfg(feature = "http")]
    pub(crate) fn with_time_source(mut self, source: crate::time_source::TimeSource) -> Self {
        self.time_source = source;
        self
    }

    /// Installs the trusted `sendInvitationEmail` hook; no delivery occurs by default.
    #[must_use]
    pub fn with_invitation_delivery(mut self, hook: Arc<dyn InvitationEmailDelivery>) -> Self {
        self.invitation_delivery = Some(hook);
        self
    }
    /// Installs application-owned delivery scheduling; omission awaits delivery.
    #[must_use]
    pub fn with_background_dispatcher(
        mut self,
        dispatcher: Arc<dyn crate::background::AuthBackgroundDispatcher>,
    ) -> Self {
        self.background_dispatcher = Some(dispatcher);
        self
    }

    /// Cancels an existing pending invitation before admitting its replacement.
    /// Cancellation is committed independently and survives subsequent admission failure.
    #[must_use]
    pub fn with_cancel_pending_invitations_on_reinvite(mut self, enabled: bool) -> Self {
        self.cancel_pending_invitations_on_reinvite = enabled;
        self
    }

    /// Replaces the trusted resource catalog/static roles and clears custom authorizers.
    #[must_use]
    pub fn with_access_control(mut self, policy: OrganizationAccessControl) -> Self {
        self.access_control =
            Authorization::from(policy).with_cache(self.access_control.cache().clone());
        self
    }
    /// Installs bound static roles and synchronous overrides as one policy snapshot.
    /// A later `with_access_control` replaces the snapshot and clears the overrides.
    #[must_use]
    pub fn with_role_authorizers(mut self, registry: OrganizationRoleAuthorizers) -> Self {
        self.access_control =
            Authorization::from(registry).with_cache(self.access_control.cache().clone());
        self
    }
    #[cfg(feature = "http")]
    pub(crate) fn with_authorization(mut self, policy: Authorization) -> Self {
        self.access_control = policy;
        self
    }
    /// Shares permission-cache effects with other services in the same trusted tenant.
    /// Custom stores without a scope must explicitly share this handle for reentrant calls.
    #[must_use]
    pub fn with_organization_permission_cache(
        mut self,
        cache: OrganizationPermissionCache,
    ) -> Self {
        self.access_control = self.access_control.with_cache(cache);
        self
    }
    /// Installs the trusted creator identity across creation and member safeguards.
    #[must_use]
    pub fn with_creator_role(mut self, role: CreatorRole) -> Self {
        self.creator_role = role;
        self
    }
    /// Installs trusted deletion and dynamic-role quota controls.
    #[must_use]
    pub fn with_organization_controls(mut self, controls: OrganizationControls) -> Self {
        self.controls = controls;
        self.role_limit = None;
        self
    }
    /// Replaces the dynamic-role scalar quota with a trusted awaited numeric policy.
    /// A later `with_organization_controls` clears this callback.
    #[must_use]
    pub fn with_role_limit_policy(mut self, hook: Arc<dyn RoleLimitPolicy>) -> Self {
        self.role_limit = Some(hook);
        self
    }
    /// Installs trusted scalar team options without changing other policies.
    #[must_use]
    pub fn with_team_policy(mut self, policy: TeamPolicy) -> Self {
        self.team_policy = policy;
        self.team_count_limit = None;
        self.team_member_limit = None;
        self
    }
    /// Selects the trusted upstream seat-query profile, independently of quotas.
    #[must_use]
    pub fn with_team_seat_query_profile(mut self, profile: TeamSeatQueryProfile) -> Self {
        self.team_seat_query_profile = profile;
        self
    }

    /// Installs the trusted awaited maximum-team callback, preserving other options.
    #[must_use]
    pub fn with_team_count_limit_policy(mut self, hook: Arc<dyn TeamCountLimitPolicy>) -> Self {
        self.team_count_limit = Some(hook);
        self
    }
    /// Installs the trusted awaited per-team seat callback, preserving other options.
    #[must_use]
    pub fn with_team_member_limit_policy(mut self, hook: Arc<dyn TeamMemberLimitPolicy>) -> Self {
        self.team_member_limit = Some(hook);
        self
    }

    /// Installs trusted invitation expiry, quota and recipient verification options.
    #[must_use]
    pub fn with_invitation_policy(mut self, policy: InvitationPolicy) -> Self {
        self.invitation_policy = policy;
        self.invitation_limit = None;
        self
    }

    /// Installs trusted awaited invitation quota; resends bypass this callback.
    /// A later `with_invitation_policy` restores its scalar quota.
    #[must_use]
    pub fn with_invitation_limit_policy(mut self, hook: Arc<dyn InvitationLimitPolicy>) -> Self {
        self.invitation_limit = Some(hook);
        self
    }

    /// Replaces allow-create with trusted awaited policy; true permits creation.
    #[must_use]
    pub fn with_allow_user_to_create_organization(
        mut self,
        hook: Arc<dyn OrganizationCreationPolicy>,
    ) -> Self {
        self.allow_creation = Some(hook);
        self
    }
    /// Replaces the scalar organization quota; true means the limit is reached.
    #[must_use]
    pub fn with_organization_limit_policy(
        mut self,
        hook: Arc<dyn OrganizationCreationPolicy>,
    ) -> Self {
        self.creation_limit = Some(hook);
        self
    }

    /// Replaces scalar membership quota with an awaited numeric policy.
    #[must_use]
    pub fn with_membership_limit_policy(mut self, hook: Arc<dyn MembershipLimitPolicy>) -> Self {
        self.membership_limit = Some(hook);
        self
    }
    /// Replaces scalar creation/membership policy and clears creation/membership callbacks.
    #[must_use]
    pub fn with_organization_policy(mut self, policy: OrganizationPolicy) -> Self {
        self.policy = policy;
        self.allow_creation = None;
        self.creation_limit = None;
        self.membership_limit = None;
        self
    }

    /// Installs trusted session lifetime, refresh and freshness options.
    #[must_use]
    pub fn with_session_policy(mut self, policy: crate::sessions::SessionPolicy) -> Self {
        self.session_policy = policy;
        self
    }

    #[must_use]
    pub fn new(store: Arc<S>) -> Self {
        Self {
            policy: OrganizationPolicy::default(),
            allow_creation: None,
            creation_limit: None,
            membership_limit: None,
            role_limit: None,
            invitation_policy: InvitationPolicy::default(),
            invitation_limit: None,
            team_count_limit: None,
            team_member_limit: None,
            team_policy: TeamPolicy::default(),
            team_seat_query_profile: TeamSeatQueryProfile::default(),
            controls: OrganizationControls::default(),
            creator_role: CreatorRole::default(),
            access_control: Authorization::from(OrganizationAccessControl::default())
                .with_cache(store.organization_permission_cache().unwrap_or_default()),
            invitation_delivery: None,
            background_dispatcher: None,
            cancel_pending_invitations_on_reinvite: false,
            time_source: crate::time_source::TimeSource::default(),
            store,
            session_policy: crate::sessions::SessionPolicy::default(),
        }
    }

    /// Lists organizations in membership insertion order.
    /// # Errors
    /// Returns authoritative session or sanitized storage errors.
    pub async fn list(
        &self,
        token: &str,
        transient: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let view = self.session(token, transient, now).await?;
        let mut tx = self.store.begin(false).await?;
        let mut organizations = Vec::new();
        for member in
            crate::store::find_all(&mut tx, Model::Member, "userId", text(&view.user, "id")?)
                .await?
        {
            if member["userId"] == view.user["id"] {
                organizations.push(
                    tx.get(Model::Organization, text(&member, "organizationId")?)
                        .await?
                        .unwrap_or(Value::Null),
                );
            }
        }
        tx.commit().await?;
        Ok(json!(organizations))
    }

    /// Authenticated availability check; slugs compare exactly, including case.
    /// # Errors
    /// Returns unauthorized, taken-slug or sanitized storage failures.
    pub async fn check_slug(
        &self,
        token: &str,
        transient: bool,
        slug: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.session(token, transient, now).await?;
        let mut tx = self.store.begin(false).await?;
        let exists = find_slug(&mut tx, slug).await?.is_some();
        tx.commit().await?;
        if exists {
            return Err(BlueprintError::new(
                "ORGANIZATION_SLUG_ALREADY_TAKEN",
                "Organization slug already taken",
            ));
        }
        Ok(json!({"status":true}))
    }

    // Recheck the session inside the operation's own transaction. HTTP callers
    // resample after awaited session/user reads; direct callers retain explicit now.
    async fn recheck<T: AuthTransaction>(
        &self,
        tx: &mut T,
        token: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Option<SessionView>> {
        crate::sessions::load_with_time(
            tx,
            token,
            true,
            now,
            self.session_policy,
            &self.time_source,
        )
        .await
    }

    async fn session(
        &self,
        token: &str,
        transient: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<SessionView> {
        SessionService::new(self.store.clone())
            .with_time_source(self.time_source.clone())
            .with_session_policy(self.session_policy)
            .get(token, transient, false, now)
            .await?
            .ok_or_else(unauthorized)
    }
}
async fn find_slug<T: AuthTransaction>(tx: &mut T, slug: &str) -> BlueprintResult<Option<Value>> {
    tx.find_unique(Model::Organization, "slug", slug).await
}
async fn member<T: AuthTransaction>(
    tx: &mut T,
    user: &str,
    organization: &str,
) -> BlueprintResult<bool> {
    Ok(crate::store::find_all(tx, Model::Member, "userId", user)
        .await?
        .iter()
        .any(|row| row["userId"] == user && row["organizationId"] == organization))
}
async fn update_active<T: AuthTransaction>(
    tx: &mut T,
    view: &SessionView,
    organization: Option<&str>,
    team: Option<&str>,
    now: DateTime<Utc>,
) -> BlueprintResult<()> {
    let mut session = tx
        .get(Model::Session, text(&view.session, "id")?)
        .await?
        .ok_or_else(unauthorized)?;
    session["activeOrganizationId"] = json!(organization);
    if let Some(team) = team {
        session["activeTeamId"] = json!(team);
    }
    session["updatedAt"] = json!(timestamp(now));
    tx.replace(Model::Session, session).await
}
fn text<'a>(row: &'a Value, field: &str) -> BlueprintResult<&'a str> {
    row[field].as_str().ok_or_else(internal)
}
fn timestamp(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn unauthorized() -> BlueprintError {
    BlueprintError::new("UNAUTHORIZED", "Unauthorized")
}
fn internal() -> BlueprintError {
    BlueprintError::new(
        "better_auth_storage_error",
        "Unable to access authentication storage",
    )
}
fn missing() -> BlueprintError {
    BlueprintError::new("ORGANIZATION_NOT_FOUND", "Organization not found")
}
fn not_member() -> BlueprintError {
    BlueprintError::new(
        "USER_IS_NOT_A_MEMBER_OF_THE_ORGANIZATION",
        "User is not a member of the organization",
    )
}
