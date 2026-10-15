//! Configured invitation admission and resend ordering.
use super::super::delivery::{self, InvitationDeliveryContext};
use super::super::permission::allowed;
use super::super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, json, missing, text, timestamp, unauthorized,
};
mod admission;
mod input;
pub(in crate::organizations) mod limit;
struct Canceled {
    row: Value,
    actor: String,
    pending: Vec<Value>,
}
enum Step {
    Complete(
        Value,
        Option<Box<BlueprintResult<InvitationDeliveryContext>>>,
    ),
    Canceled(Canceled),
}
impl<S: AuthStore> OrganizationService<S> {
    /// Creates or resends an invitation using configured grants and limits.
    /// # Errors
    /// Returns session, scope, role, duplicate, team or admission failures.
    pub async fn create_invitation(
        &self,
        token: &str,
        transient: bool,
        body: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.session(token, transient, now).await?;
        let first = if self.invitation_limit.is_some() || self.team_member_limit.is_some() {
            self.invitation_with_limit(token, body, now, self.invitation_limit.as_deref())
                .await?
        } else {
            self.invitation_step(token, body, now, None).await?
        };
        let completed = match first {
            Step::Complete(row, context) => (row, context),
            Step::Canceled(canceled) => match self
                .invitation_step(token, body, now, Some(&canceled))
                .await?
            {
                Step::Complete(row, context) => (row, context),
                Step::Canceled(_) => return Err(state_changed()),
            },
        };
        if let Some(hook) = &self.invitation_delivery {
            let context = (*completed.1.ok_or_else(delivery::failure)?)?;
            let future = hook.prepare(context).map_err(|_| delivery::failure())?;
            crate::background::run(
                future,
                self.background_dispatcher.as_deref(),
                "/organization/invite-member",
                "INVITATION_DELIVERY_FAILED",
                "INVITATION_DELIVERY_DISPATCH_FAILED",
                "Unable to deliver invitation email",
            )
            .await;
        }
        Ok(completed.0)
    }
    async fn invitation_step(
        &self,
        token: &str,
        body: &Value,
        now: DateTime<Utc>,
        canceled: Option<&Canceled>,
    ) -> BlueprintResult<Step> {
        let mut tx = self.store.begin(true).await?;
        let admission::Admission {
            org,
            email,
            role,
            actor,
            inviter,
            quota_inviter: _,
            team_view: _,
        } = self.invitation_admission(&mut tx, token, body, now).await?;
        let org = org.as_str();
        let matching_rows =
            crate::store::find_all(&mut tx, Model::Invitation, "organizationId", org)
                .await?
                .into_iter()
                .filter(|row| {
                    row["organizationId"] == org
                        && row["email"] == email
                        && row["status"] == "pending"
                })
                .collect::<Vec<_>>();
        let pending = matching_rows
            .iter()
            .take(100)
            .find(|row| unexpired(row, now))
            .cloned();
        let pending_rows = matching_rows
            .into_iter()
            .filter(|row| unexpired(row, now))
            .collect::<Vec<_>>();
        let resend = body["resend"] == true;
        let cancel = self.cancel_pending_invitations_on_reinvite && canceled.is_none() && !resend;
        let unchanged_pending =
            canceled.is_some_and(|expected| same_pending(&expected.pending, &pending_rows));
        if pending.is_some() && !resend && !cancel && !unchanged_pending {
            return Err(error(
                "USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION",
                "User is already invited to this organization",
            ));
        }
        let organization = tx
            .get(Model::Organization, org)
            .await?
            .ok_or_else(missing)?;
        if let Some(expected) = canceled
            && (!unchanged_pending
                || expected.actor != actor
                || expected.row["organizationId"] != org
                || tx
                    .get(Model::Invitation, text(&expected.row, "id")?)
                    .await?
                    .as_ref()
                    != Some(&expected.row))
        {
            return Err(state_changed());
        }
        if cancel && let Some(mut pending) = pending.clone() {
            pending["status"] = json!("canceled");
            tx.replace(Model::Invitation, pending.clone()).await?;
            tx.commit().await?;
            let others = pending_rows
                .into_iter()
                .filter(|row| row["id"] != pending["id"])
                .collect();
            return Ok(Step::Canceled(Canceled {
                row: pending,
                actor,
                pending: others,
            }));
        }
        let expiry = timestamp(self.invitation_policy.expiry(now)?);
        if canceled.is_none()
            && let Some(mut pending) = pending
        {
            pending["expiresAt"] = json!(expiry);
            tx.replace(Model::Invitation, pending.clone()).await?;
            let completed = complete(pending, inviter, &organization);
            tx.commit().await?;
            return Ok(completed);
        }
        ensure_invitation_quota(&mut tx, org, now, self.invitation_policy.limit).await?;
        let teams = input::teams(
            &mut tx,
            org,
            body.get("teamId"),
            self.team_policy.maximum_members,
        )
        .await?;
        let invitation = json!({"id":uuid::Uuid::new_v4().to_string(),"email":email,"role":role,"organizationId":org,"inviterId":actor,"status":"pending","expiresAt":expiry,"createdAt":timestamp(now),"teamId":if teams.is_empty(){Value::Null}else{json!(teams.join(","))}});
        tx.insert(Model::Invitation, invitation.clone()).await?;
        let completed = complete(invitation, inviter, &organization);
        tx.commit().await?;
        Ok(completed)
    }
}
fn error(code: &str, message: &str) -> BlueprintError {
    BlueprintError::new(code, message)
}
fn unexpired(row: &Value, now: DateTime<Utc>) -> bool {
    row["expiresAt"]
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .is_some_and(|expiry| expiry > now)
}

fn state_changed() -> BlueprintError {
    error("INVITATION_STATE_CHANGED", "Invitation state changed")
}

fn same_pending(expected: &[Value], current: &[Value]) -> bool {
    fn index(rows: &[Value]) -> Option<std::collections::BTreeMap<&str, &Value>> {
        rows.iter()
            .map(|row| Some((row["id"].as_str()?, row)))
            .collect()
    }
    if expected.len() != current.len() {
        return false;
    }
    match (index(expected), index(current)) {
        (Some(a), Some(b)) => a.len() == expected.len() && a == b,
        _ => false,
    }
}

fn complete(row: Value, inviter: Option<BlueprintResult<Value>>, organization: &Value) -> Step {
    let context = inviter
        .map(|inviter| inviter.and_then(|inviter| delivery::context(inviter, organization, &row)));
    Step::Complete(row, context.map(Box::new))
}

async fn ensure_invitation_quota<T: AuthTransaction>(
    tx: &mut T,
    org: &str,
    now: DateTime<Utc>,
    limit: u32,
) -> BlueprintResult<()> {
    if crate::store::find_all(tx, Model::Invitation, "organizationId", org)
        .await?
        .iter()
        .filter(|row| row["organizationId"] == org && row["status"] == "pending")
        .take(100)
        .filter(|row| unexpired(row, now))
        .count()
        >= usize::try_from(limit).unwrap_or(usize::MAX)
    {
        return Err(error(
            "INVITATION_LIMIT_REACHED",
            "Invitation limit reached",
        ));
    }
    Ok(())
}
