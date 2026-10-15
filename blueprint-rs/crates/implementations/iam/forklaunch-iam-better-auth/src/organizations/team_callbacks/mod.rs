//! Trusted awaited team policy callbacks and current-adapter capabilities.
mod adapter;
mod member_completion;
use crate::sessions::SessionView;
pub(super) use adapter::{AutocommitAdapter, TransactionAdapter};
pub use adapter::{OrganizationAdapterFuture, OrganizationCallbackAdapter};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use std::{future::Future, pin::Pin};
/// Owned bounded identity captured before the callback, including the real
/// session token supplied by upstream. Treat all fields as trusted secrets.
/// No Debug/Serde implementations expose this context accidentally.
pub struct TeamPolicySession {
    user: Value,
    session: Value,
}
impl TeamPolicySession {
    #[must_use]
    pub fn user(&self) -> &Value {
        &self.user
    }
    #[must_use]
    pub fn session(&self) -> &Value {
        &self.session
    }
}
/// Awaited team-count callback data and the current trusted tenant adapter.
pub struct TeamCountLimitContext<'a> {
    organization_id: String,
    session: Option<TeamPolicySession>,
    adapter: &'a mut dyn OrganizationCallbackAdapter,
}
impl TeamCountLimitContext<'_> {
    #[must_use]
    pub fn organization_id(&self) -> &str {
        &self.organization_id
    }
    #[must_use]
    pub fn session(&self) -> Option<&TeamPolicySession> {
        self.session.as_ref()
    }
    pub fn adapter(&mut self) -> &mut dyn OrganizationCallbackAdapter {
        self.adapter
    }
}
/// Awaited team-seat callback data and the current trusted tenant adapter.
pub struct TeamMemberLimitContext<'a> {
    organization_id: String,
    team_id: String,
    session: TeamPolicySession,
    adapter: &'a mut dyn OrganizationCallbackAdapter,
}
impl TeamMemberLimitContext<'_> {
    #[must_use]
    pub fn organization_id(&self) -> &str {
        &self.organization_id
    }
    #[must_use]
    pub fn team_id(&self) -> &str {
        &self.team_id
    }
    #[must_use]
    pub fn session(&self) -> &TeamPolicySession {
        &self.session
    }
    pub fn adapter(&mut self) -> &mut dyn OrganizationCallbackAdapter {
        self.adapter
    }
}
/// Full IEEE quota result, borrowed for the caller's adapter lifetime.
pub type TeamLimitFuture<'a> = Pin<Box<dyn Future<Output = BlueprintResult<f64>> + Send + 'a>>;
pub trait TeamCountLimitPolicy: Send + Sync {
    /// # Errors
    /// Callback errors are sanitized fatal empty HTTP 500 responses.
    fn evaluate<'a>(&'a self, context: TeamCountLimitContext<'a>) -> TeamLimitFuture<'a>;
}
pub trait TeamMemberLimitPolicy: Send + Sync {
    /// # Errors
    /// Callback errors are sanitized fatal empty HTTP 500 responses.
    fn evaluate<'a>(&'a self, context: TeamMemberLimitContext<'a>) -> TeamLimitFuture<'a>;
}
pub(super) fn failure() -> BlueprintError {
    BlueprintError::new("HTTP_INTERNAL_EMPTY", "Team limit policy failed")
}
fn session(view: &SessionView) -> BlueprintResult<TeamPolicySession> {
    let projection = super::delivery::projection;
    Ok(TeamPolicySession {
        user: crate::accounts::reset_request::projection(&view.user).map_err(|_| failure())?,
        session: projection(
            &view.session,
            &[
                "id",
                "expiresAt",
                "token",
                "createdAt",
                "updatedAt",
                "ipAddress",
                "userAgent",
                "userId",
                "activeOrganizationId",
                "activeTeamId",
            ],
        )
        .map_err(|_| failure())?,
    })
}
fn id(value: &str) -> BlueprintResult<String> {
    if value.len() > 4096 {
        return Err(failure());
    }
    Ok(value.to_owned())
}
pub(super) async fn count(
    hook: &dyn TeamCountLimitPolicy,
    org: &str,
    view: Option<&SessionView>,
    adapter: &mut dyn OrganizationCallbackAdapter,
) -> BlueprintResult<f64> {
    hook.evaluate(TeamCountLimitContext {
        organization_id: id(org)?,
        session: view.map(session).transpose()?,
        adapter,
    })
    .await
    .map_err(|_| failure())
}
pub(super) async fn member(
    hook: &dyn TeamMemberLimitPolicy,
    org: &str,
    team: &str,
    view: &SessionView,
    adapter: &mut dyn OrganizationCallbackAdapter,
) -> BlueprintResult<f64> {
    hook.evaluate(TeamMemberLimitContext {
        organization_id: id(org)?,
        team_id: id(team)?,
        session: session(view)?,
        adapter,
    })
    .await
    .map_err(|_| failure())
}

pub(super) async fn seat<T: crate::store::AuthTransaction>(
    tx: &mut T,
    team: &str,
    user: &str,
    now: chrono::DateTime<chrono::Utc>,
    limit: f64,
    profile: super::TeamSeatQueryProfile,
) -> BlueprintResult<Value> {
    use crate::store::Model;
    if let Some(row) = tx.get(Model::Team, team).await? {
        return super::teams::add_team_member_numeric(tx, row, user, now, Some(limit), profile)
            .await;
    }
    if let Some(row) = crate::store::find_all(tx, Model::TeamMember, "teamId", team)
        .await?
        .into_iter()
        .find(|row| row["userId"] == user)
    {
        return Ok(row);
    }
    profile.validate(limit)?;
    Err(super::team_policy::member_limit())
}
impl<S: crate::store::AuthStore> super::OrganizationService<S> {
    /// Creates a team from trusted server code without session admission.
    /// No HTTP route exposes this capability. Quota callbacks receive no session.
    /// # Errors
    /// Returns missing organization, quota, callback or storage failures.
    pub async fn create_team_for_organization(
        &self,
        organization_id: &str,
        name: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> BlueprintResult<Value> {
        use crate::store::{AuthTransaction, Model};
        self.organization_features.require_teams()?;
        if organization_id.is_empty() {
            return Err(BlueprintError::new(
                "NO_ACTIVE_ORGANIZATION",
                "No active organization",
            ));
        }
        let mut tx = self.store.begin(true).await?;
        let count = crate::store::find_all(&mut tx, Model::Team, "organizationId", organization_id)
            .await?
            .iter()
            .filter(|row| row["organizationId"] == organization_id)
            .take(100)
            .count();
        let maximum = if let Some(hook) = &self.team_count_limit {
            tx.commit().await?;
            let mut adapter = AutocommitAdapter(self.store.as_ref());
            let maximum = crate::organizations::team_callbacks::count(
                hook.as_ref(),
                organization_id,
                None,
                &mut adapter,
            )
            .await?;
            tx = self.store.begin(true).await?;
            Some(maximum)
        } else {
            self.team_policy.maximum_teams.map(f64::from)
        };
        if maximum.is_some_and(|m| {
            m != 0.0 && !m.is_nan() && f64::from(u32::try_from(count).unwrap_or(u32::MAX)) >= m
        }) {
            return Err(BlueprintError::new(
                "YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_TEAMS",
                "You have reached the maximum number of teams",
            ));
        }
        tx.get(Model::Organization, organization_id)
            .await?
            .ok_or_else(super::missing)?;
        let now = self.time_source.sample(now);
        let row = serde_json::json!({"id":uuid::Uuid::new_v4().to_string(),"organizationId":organization_id,"name":name,"memberCount":0,"createdAt":super::timestamp(now),"updatedAt":super::timestamp(now)});
        tx.insert(Model::Team, row.clone()).await?;
        tx.commit().await?;
        let mut public = row;
        public
            .as_object_mut()
            .ok_or_else(failure)?
            .remove("memberCount");
        Ok(public)
    }
}

impl<S: crate::store::AuthStore> super::OrganizationService<S> {
    pub(in crate::organizations) async fn enroll_accepted_team<T: crate::store::AuthTransaction>(
        &self,
        tx: &mut T,
        org: &str,
        id: &str,
        view: &SessionView,
        now: chrono::DateTime<chrono::Utc>,
    ) -> BlueprintResult<()> {
        use crate::store::Model;
        let team = tx
            .get(Model::Team, id)
            .await?
            .filter(|row| row["organizationId"] == org)
            .ok_or_else(|| BlueprintError::new("TEAM_NOT_FOUND", "Team not found"))?;
        if let Some(hook) = &self.team_member_limit {
            let mut adapter = TransactionAdapter(tx);
            let limit = member(hook.as_ref(), org, id, view, &mut adapter).await?;
            seat(
                tx,
                id,
                super::text(&view.user, "id")?,
                self.time_source.sample(now),
                limit,
                self.team_seat_query_profile,
            )
            .await?;
        } else {
            super::teams::add_team_member(
                tx,
                team,
                super::text(&view.user, "id")?,
                now,
                self.team_policy.maximum_members,
            )
            .await?;
        }
        Ok(())
    }
}
