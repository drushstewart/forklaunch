//! Awaited invitation quota after committed cancellation and before current count.
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Step, Utc, Value, admission, complete, delivery, error, input, json,
    missing, timestamp, unexpired,
};
use std::{future::Future, pin::Pin};
/// Bounded owned configured projections; deliberately no Debug/Serde or credentials.
/// Trusted implementations may capture their own store/services to perform work
/// outside the invitation transaction. Raw JavaScript `AuthContext` mutation and
/// arbitrary additional fields are not represented by this typed context.
pub struct InvitationLimitContext {
    user: Value,
    organization: Value,
    member: Value,
}
impl InvitationLimitContext {
    #[must_use]
    pub fn user(&self) -> &Value {
        &self.user
    }
    #[must_use]
    pub fn organization(&self) -> &Value {
        &self.organization
    }
    #[must_use]
    pub fn member(&self) -> &Value {
        &self.member
    }
}
/// IEEE numeric result, including signed zero, fractions, NaN and infinities.
pub type InvitationLimitFuture =
    Pin<Box<dyn Future<Output = BlueprintResult<f64>> + Send + 'static>>;
/// Trusted awaited `invitationLimit`. Preparation/future failures are fatal and
/// sanitized; a preceding cancellation remains committed. No transaction spans
/// the owned future and no automatic background scheduling occurs.
pub trait InvitationLimitPolicy: Send + Sync {
    /// # Errors
    /// Preparation failures produce an empty HTTP 500 response.
    fn prepare(&self, context: InvitationLimitContext) -> BlueprintResult<InvitationLimitFuture>;
}
fn failure() -> BlueprintError {
    BlueprintError::new("HTTP_INTERNAL_EMPTY", "Invitation limit policy failed")
}
fn context(mut inviter: Value, organization: &Value) -> BlueprintResult<InvitationLimitContext> {
    let user = inviter
        .as_object_mut()
        .and_then(|row| row.remove("user"))
        .ok_or_else(failure)?;
    let organization = delivery::organization_projection(organization).map_err(|_| failure())?;
    Ok(InvitationLimitContext {
        user,
        organization,
        member: inviter,
    })
}
impl<S: AuthStore> OrganizationService<S> {
    pub(super) async fn invitation_with_limit(
        &self,
        token: &str,
        body: &Value,
        now: DateTime<Utc>,
        hook: Option<&dyn InvitationLimitPolicy>,
    ) -> BlueprintResult<Step> {
        let mut tx = self.store.begin(true).await?;
        let mut admitted = self.invitation_admission(&mut tx, token, body, now).await?;
        let rows =
            crate::store::find_all(&mut tx, Model::Invitation, "organizationId", &admitted.org)
                .await?;
        let pending = rows
            .into_iter()
            .filter(|row| {
                row["organizationId"] == admitted.org
                    && row["email"] == admitted.email
                    && row["status"] == "pending"
            })
            .take(100)
            .find(|row| unexpired(row, now));
        let resend = body["resend"] == true;
        if pending.is_some() && !resend && !self.cancel_pending_invitations_on_reinvite {
            return Err(error(
                "USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION",
                "User is already invited to this organization",
            ));
        }
        let organization = tx
            .get(Model::Organization, &admitted.org)
            .await?
            .ok_or_else(missing)?;
        if let Some(mut row) = pending {
            if resend {
                row["expiresAt"] = json!(timestamp(self.invitation_policy.expiry(now)?));
                tx.replace(Model::Invitation, row.clone()).await?;
                let completed = complete(row, admitted.inviter, &organization);
                tx.commit().await?;
                return Ok(completed);
            }
            row["status"] = json!("canceled");
            tx.replace(Model::Invitation, row).await?;
        }
        tx.commit().await?;
        // Projection/preparation failures occur after the cancellation commit.
        let limit = if let Some(hook) = hook {
            let inviter = admitted
                .quota_inviter
                .take()
                .ok_or_else(failure)?
                .map_err(|_| failure())?;
            hook.prepare(context(inviter, &organization)?)
                .map_err(|_| failure())?
                .await
                .map_err(|_| failure())?
        } else {
            f64::from(self.invitation_policy.limit)
        };
        self.insert_after_invitation_limit(admitted, &organization, body, now, limit)
            .await
    }
    async fn insert_after_invitation_limit(
        &self,
        admitted: admission::Admission,
        organization: &Value,
        body: &Value,
        now: DateTime<Utc>,
        limit: f64,
    ) -> BlueprintResult<Step> {
        let mut tx = self.store.begin(true).await?;
        let rows =
            crate::store::find_all(&mut tx, Model::Invitation, "organizationId", &admitted.org)
                .await?;
        let count_now = self.time_source.sample(now);
        let count = rows
            .iter()
            .filter(|row| row["organizationId"] == admitted.org && row["status"] == "pending")
            .take(100)
            .filter(|row| unexpired(row, count_now))
            .count();
        if f64::from(u32::try_from(count).map_err(|_| failure())?) >= limit {
            return Err(error(
                "INVITATION_LIMIT_REACHED",
                "Invitation limit reached",
            ));
        }
        if let Some(hook) = &self.team_member_limit {
            let teams = input::teams(&mut tx, &admitted.org, body.get("teamId"), None).await?;
            tx.commit().await?;
            if body
                .get("teamId")
                .is_some_and(|v| v.is_array() || v.as_str().is_some_and(|s| !s.is_empty()))
            {
                for id in &teams {
                    let mut read = self.store.begin(false).await?;
                    read.get(Model::Team, id)
                        .await?
                        .filter(|row| row["organizationId"] == admitted.org)
                        .ok_or_else(|| error("TEAM_NOT_FOUND", "Team not found"))?;
                    let count = crate::store::find_all(&mut read, Model::TeamMember, "teamId", id)
                        .await?
                        .iter()
                        .filter(|row| row["teamId"] == *id)
                        .take(100)
                        .count();
                    read.commit().await?;
                    let mut adapter = crate::organizations::team_callbacks::AutocommitAdapter(
                        self.store.as_ref(),
                    );
                    let limit = crate::organizations::team_callbacks::member(
                        hook.as_ref(),
                        &admitted.org,
                        id,
                        admitted.team_view.as_ref().ok_or_else(failure)?,
                        &mut adapter,
                    )
                    .await?;
                    if f64::from(u32::try_from(count).map_err(|_| failure())?) >= limit {
                        return Err(crate::organizations::team_policy::member_limit());
                    }
                }
            }
            let mut tx = self.store.begin(true).await?;
            let now = self.time_source.sample(now);
            let row = json!({"id":uuid::Uuid::new_v4().to_string(),"email":admitted.email,"role":admitted.role,"organizationId":admitted.org,"inviterId":admitted.actor,"status":"pending","expiresAt":timestamp(self.invitation_policy.expiry(now)?),"createdAt":timestamp(now),"teamId":if teams.is_empty(){Value::Null}else{json!(teams.join(","))}});
            tx.insert(Model::Invitation, row.clone()).await?;
            let completed = complete(row, admitted.inviter, organization);
            tx.commit().await?;
            return Ok(completed);
        }
        let teams = input::teams(
            &mut tx,
            &admitted.org,
            body.get("teamId"),
            self.team_policy.maximum_members,
        )
        .await?;
        let now = self.time_source.sample(now);
        let expiry = timestamp(self.invitation_policy.expiry(now)?);
        let row = json!({"id":uuid::Uuid::new_v4().to_string(),"email":admitted.email,"role":admitted.role,"organizationId":admitted.org,"inviterId":admitted.actor,"status":"pending","expiresAt":expiry,"createdAt":timestamp(now),"teamId":if teams.is_empty(){Value::Null}else{json!(teams.join(","))}});
        tx.insert(Model::Invitation, row.clone()).await?;
        let completed = complete(row, admitted.inviter, organization);
        tx.commit().await?;
        Ok(completed)
    }
}
