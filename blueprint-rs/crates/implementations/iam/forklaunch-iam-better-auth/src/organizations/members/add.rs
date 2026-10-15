//! Trusted server-only addition; deliberately has no public HTTP route.
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, find_member, json, missing, no_active, text,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MemberRoles {
    Role(String),
    Roles(Vec<String>),
}
impl MemberRoles {
    pub(in crate::organizations) fn joined(&self) -> String {
        match self {
            Self::Role(role) => role.clone(),
            Self::Roles(roles) => roles.join(","),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddMemberInput {
    pub user_id: String,
    pub role: MemberRoles,
    pub organization_id: Option<String>,
    pub team_id: Option<String>,
}
impl<S: AuthStore> OrganizationService<S> {
    /// Adds a member from trusted server code, without public-route authorization.
    /// An explicit organization needs no session. A supplied session provides only
    /// the active-organization fallback and its usual refresh/expiry side effects.
    /// Roles are stored exactly, joining arrays without trimming or validation.
    /// # Errors
    /// Returns missing scope/user/team, duplicate membership, limit or store errors.
    pub async fn add_member(
        &self,
        token: Option<&str>,
        transient: bool,
        input: AddMemberInput,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        if let Some(hook) = &self.membership_limit {
            return self
                .add_member_with_policy(token, transient, input, now, hook.as_ref())
                .await;
        }
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
                session.as_ref().and_then(|view| {
                    view.session["activeOrganizationId"]
                        .as_str()
                        .filter(|id| !id.is_empty())
                })
            })
            .ok_or_else(no_active)?;
        let mut tx = self.store.begin(true).await?;
        let user = tx
            .get(Model::User, &input.user_id)
            .await?
            .ok_or_else(|| BlueprintError::new("USER_NOT_FOUND", "User not found"))?;
        if duplicate(&mut tx, &user, id).await? {
            return Err(BlueprintError::new(
                "USER_IS_ALREADY_A_MEMBER_OF_THIS_ORGANIZATION",
                "User is already a member of this organization",
            ));
        }
        let team = if let Some(team_id) = input.team_id.as_deref().filter(|id| !id.is_empty()) {
            Some(
                tx.get(Model::Team, team_id)
                    .await?
                    .filter(|team| team["organizationId"] == id)
                    .ok_or_else(|| BlueprintError::new("TEAM_NOT_FOUND", "Team not found"))?,
            )
        } else {
            None
        };
        let count = crate::store::find_all(&mut tx, Model::Member, "organizationId", id)
            .await?
            .iter()
            .filter(|row| row["organizationId"] == id)
            .count();
        if tx.get(Model::Organization, id).await?.is_none() {
            return Err(missing());
        }
        if count >= self.policy.member_limit() {
            return Err(BlueprintError::new(
                "ORGANIZATION_MEMBERSHIP_LIMIT_REACHED",
                "Organization membership limit reached",
            ));
        }
        let member = json!({"id":uuid::Uuid::new_v4().to_string(),"organizationId":id,"userId":user["id"],"role":input.role.joined(),"createdAt":super::super::timestamp(now)});
        tx.insert(Model::Member, member.clone()).await?;
        if let Some(team) = team {
            super::super::teams::add_team_member(
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
pub(in crate::organizations) async fn duplicate<T: AuthTransaction>(
    tx: &mut T,
    user: &Value,
    id: &str,
) -> BlueprintResult<bool> {
    let email = text(user, "email")?.to_lowercase();
    let Some(user) = tx.find_unique(Model::User, "email", &email).await? else {
        return Ok(false);
    };
    Ok(find_member(tx, text(&user, "id")?, id).await?.is_some())
}
