//! Configured invitation admission and resend ordering.
use super::super::permission::allowed;
use super::super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, json, missing, text, timestamp, unauthorized,
};
mod input;
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
        let mut tx = self.store.begin(true).await?;
        let view = self
            .recheck(&mut tx, token, now)
            .await?
            .ok_or_else(unauthorized)?;
        let org = body["organizationId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                view.session["activeOrganizationId"]
                    .as_str()
                    .filter(|s| !s.is_empty())
            })
            .ok_or_else(missing)?;
        let email = text(body, "email")?.to_lowercase();
        crate::accounts::validate_email(&email)?;
        let member = crate::store::find_all(&mut tx, Model::Member, "organizationId", org)
            .await?
            .into_iter()
            .find(|row| row["organizationId"] == org && row["userId"] == view.user["id"])
            .ok_or_else(|| error("MEMBER_NOT_FOUND", "Member not found"))?;
        if !allowed(
            &mut tx,
            org,
            text(&member, "role")?,
            Some(&json!({"invitation":["create"]})),
        )
        .await?
        {
            return Err(error(
                "YOU_ARE_NOT_ALLOWED_TO_INVITE_USERS_TO_THIS_ORGANIZATION",
                "You are not allowed to invite users to this organization",
            ));
        }
        let role = input::roles(&body["role"]);
        input::validate_roles(&mut tx, org, &role).await?;
        if !text(&member, "role")?
            .split(',')
            .any(|role| input::trim(role) == "owner")
            && role.split(',').any(|role| role == "owner")
        {
            return Err(error(
                "YOU_ARE_NOT_ALLOWED_TO_INVITE_USER_WITH_THIS_ROLE",
                "You are not allowed to invite a user with this role",
            ));
        }
        input::ensure_not_member(&mut tx, org, &email).await?;
        let pending = crate::store::find_all(&mut tx, Model::Invitation, "organizationId", org)
            .await?
            .into_iter()
            .filter(|row| {
                row["organizationId"] == org && row["email"] == email && row["status"] == "pending"
            })
            .take(100)
            .find(|row| unexpired(row, now));
        let resend = body["resend"] == true;
        if pending.is_some() && !resend {
            return Err(error(
                "USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION",
                "User is already invited to this organization",
            ));
        }
        tx.get(Model::Organization, org)
            .await?
            .ok_or_else(missing)?;
        let expiry = timestamp(now + chrono::Duration::hours(48));
        if let Some(mut pending) = pending {
            pending["expiresAt"] = json!(expiry);
            tx.replace(Model::Invitation, pending.clone()).await?;
            tx.commit().await?;
            return Ok(pending);
        }
        if crate::store::find_all(&mut tx, Model::Invitation, "organizationId", org)
            .await?
            .iter()
            .filter(|row| row["organizationId"] == org && row["status"] == "pending")
            .take(100)
            .filter(|row| unexpired(row, now))
            .count()
            >= 100
        {
            return Err(error(
                "INVITATION_LIMIT_REACHED",
                "Invitation limit reached",
            ));
        }
        let teams = input::teams(&mut tx, org, body.get("teamId")).await?;
        let invitation = json!({"id":uuid::Uuid::new_v4().to_string(),"email":email,"role":role,"organizationId":org,"inviterId":view.user["id"],"status":"pending","expiresAt":expiry,"createdAt":timestamp(now),"teamId":if teams.is_empty(){Value::Null}else{json!(teams.join(","))}});
        tx.insert(Model::Invitation, invitation.clone()).await?;
        tx.commit().await?;
        Ok(invitation)
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
