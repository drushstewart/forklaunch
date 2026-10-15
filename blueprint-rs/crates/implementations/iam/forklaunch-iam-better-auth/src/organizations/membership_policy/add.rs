use super::super::{AddMemberInput, DateTime, Utc, json, missing, text};
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, MembershipLimitPolicy, Model,
    OrganizationService, Value, count, invoke,
};
use base64::Engine;
impl<S: AuthStore> OrganizationService<S> {
    pub(in crate::organizations) async fn add_member_with_policy(
        &self,
        token: Option<&str>,
        transient: bool,
        input: AddMemberInput,
        now: DateTime<Utc>,
        hook: &dyn MembershipLimitPolicy,
    ) -> BlueprintResult<Value> {
        let session = if input.user_id.is_empty() {
            None
        } else if let Some(token) = token {
            self.session(token, transient, now).await.ok()
        } else {
            None
        };
        let id = input
            .organization_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .or_else(|| {
                session.as_ref().and_then(|v| {
                    v.session["activeOrganizationId"]
                        .as_str()
                        .filter(|id| !id.is_empty())
                })
            })
            .ok_or_else(no_active)?;
        let mut tx = self.store.begin(false).await?;
        let user = tx
            .get(Model::User, &input.user_id)
            .await?
            .ok_or_else(|| BlueprintError::new("USER_NOT_FOUND", "User not found"))?;
        if super::super::members::add::duplicate(&mut tx, &user, id).await? {
            return Err(BlueprintError::new(
                "USER_IS_ALREADY_A_MEMBER_OF_THIS_ORGANIZATION",
                "User is already a member of this organization",
            ));
        }
        let team_id = input.team_id.as_deref().filter(|id| !id.is_empty());
        if let Some(team) = team_id {
            tx.get(Model::Team, team)
                .await?
                .filter(|row| row["organizationId"] == id)
                .ok_or_else(team_missing)?;
        }
        let count = count(&mut tx, id).await?;
        let organization = tx.get(Model::Organization, id).await?.ok_or_else(missing)?;
        tx.commit().await?;
        invoke(hook, &user, &organization, count).await?;
        let mut tx = self.store.begin(true).await?;
        let now = self.time_source.sample(now);
        let member = json!({"id":uuid::Uuid::new_v4().to_string(),"organizationId":id,"userId":user["id"],"role":input.role.joined(),"createdAt":super::super::timestamp(now)});
        tx.insert(Model::Member, member.clone()).await?;
        if let (Some(hook), Some(team)) = (&self.team_member_limit, team_id) {
            tx.commit().await?;
            return self
                .complete_member_with_team_callback(
                    member,
                    team,
                    session.as_ref(),
                    now,
                    hook.as_ref(),
                )
                .await;
        }

        if let Some(team) = team_id {
            enroll(
                &mut tx,
                team,
                text(&user, "id")?,
                now,
                self.team_policy.maximum_members,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(member)
    }
}
fn team_missing() -> BlueprintError {
    BlueprintError::new("TEAM_NOT_FOUND", "Team not found")
}

fn no_active() -> BlueprintError {
    BlueprintError::new("NO_ACTIVE_ORGANIZATION", "No active organization")
}

async fn enroll<T: AuthTransaction>(
    tx: &mut T,
    id: &str,
    user: &str,
    now: DateTime<Utc>,
    limit: Option<u32>,
) -> BlueprintResult<()> {
    if let Some(team) = tx.get(Model::Team, id).await? {
        super::super::teams::add_team_member(tx, team, user, now, limit).await?;
        return Ok(());
    }
    // A team deleted during the callback is not re-created. The unbounded
    // upstream membership path may still insert its pair; bounded seats deny.
    if crate::store::find_all(tx, Model::TeamMember, "teamId", id)
        .await?
        .iter()
        .any(|row| row["userId"] == user)
    {
        return Ok(());
    }
    if limit.is_some() {
        return Err(super::super::team_policy::member_limit());
    }
    let pair = serde_json::to_vec(&json!([id, user])).map_err(|_| super::super::internal())?;
    let key = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(ring::digest::digest(&ring::digest::SHA256, &pair).as_ref());
    tx.insert(Model::TeamMember,json!({"id":uuid::Uuid::new_v4().to_string(),"teamId":id,"userId":user,"membershipKey":key,"createdAt":super::super::timestamp(now)})).await
}
