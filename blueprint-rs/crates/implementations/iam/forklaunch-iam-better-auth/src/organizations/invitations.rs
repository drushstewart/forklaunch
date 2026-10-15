//! Configured invitation visibility and adapter joins.
mod accept;
mod create;
mod mutation;
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, SessionService, Utc, Value, json, member, missing, text,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Lists the first 100 organization invitations, retaining every status.
    /// # Errors
    /// Requires a session, active/explicit organization and joined membership.
    pub async fn list_invitations(
        &self,
        token: &str,
        transient: bool,
        id: Option<&str>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let view = self.session(token, transient, now).await?;
        let id = id
            .filter(|id| !id.is_empty())
            .or_else(|| {
                view.session["activeOrganizationId"]
                    .as_str()
                    .filter(|id| !id.is_empty())
            })
            .ok_or_else(|| message("Organization ID is required"))?;
        let mut tx = self.store.begin(false).await?;
        if !member(&mut tx, text(&view.user, "id")?, id).await? {
            return Err(BlueprintError::new(
                "HTTP_FORBIDDEN_MESSAGE_ONLY",
                "You are not a member of this organization",
            ));
        }
        let invitations = crate::store::find_all(&mut tx, Model::Invitation, "organizationId", id)
            .await?
            .into_iter()
            .filter(|row| row["organizationId"] == id)
            .take(100)
            .collect::<Vec<_>>();
        tx.commit().await?;
        Ok(json!(invitations))
    }
    /// Reads a pending unexpired invitation addressed to the session email.
    /// # Errors
    /// Requires matching recipient, organization and a joined inviter membership.
    pub async fn get_invitation(
        &self,
        token: &str,
        transient: bool,
        id: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let view = self.session(token, transient, now).await?;
        let mut tx = self.store.begin(false).await?;
        let mut invitation = tx
            .get(Model::Invitation, id)
            .await?
            .filter(|row| {
                row["status"] == "pending"
                    && row["expiresAt"]
                        .as_str()
                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        .is_some_and(|expiry| expiry >= now)
            })
            .ok_or_else(|| message("Invitation not found!"))?;
        if text(&invitation, "email")?.to_lowercase() != text(&view.user, "email")?.to_lowercase() {
            return Err(BlueprintError::new(
                "YOU_ARE_NOT_THE_RECIPIENT_OF_THE_INVITATION",
                "You are not the recipient of the invitation",
            ));
        }
        if view.user["emailVerified"] != true {
            return Err(BlueprintError::new(
                "EMAIL_VERIFICATION_REQUIRED_FOR_INVITATION",
                "Email verification required to view or list invitations for the session email",
            ));
        }
        let org = text(&invitation, "organizationId")?;
        let organization = tx
            .get(Model::Organization, org)
            .await?
            .ok_or_else(missing)?;
        let inviter = text(&invitation, "inviterId")?;
        if !member(&mut tx, inviter, org).await? {
            return Err(BlueprintError::new(
                "INVITER_IS_NO_LONGER_A_MEMBER_OF_THE_ORGANIZATION",
                "Inviter is no longer a member of the organization",
            ));
        }
        let user = tx
            .get(Model::User, inviter)
            .await?
            .ok_or_else(|| BlueprintError::new("HTTP_INTERNAL_EMPTY", "Missing inviter"))?;
        invitation["organizationName"] = organization["name"].clone();
        invitation["organizationSlug"] = organization["slug"].clone();
        invitation["inviterEmail"] = user["email"].clone();
        tx.commit().await?;
        Ok(invitation)
    }
    /// Lists received pending invitations after the adapter's 100-row limit.
    /// Explicit email is for trusted server callers; HTTP rejects it.
    /// # Errors
    /// Rejects unverified sessions or absent session/email and storage failures.
    pub async fn list_user_invitations(
        &self,
        token: &str,
        transient: bool,
        email: Option<&str>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let view = SessionService::new(self.store.clone())
            .with_time_source(self.time_source.clone())
            .with_session_policy(self.session_policy)
            .get(token, transient, false, now)
            .await?;
        if view
            .as_ref()
            .is_some_and(|view| view.user["emailVerified"] != true)
        {
            return Err(BlueprintError::new(
                "EMAIL_VERIFICATION_REQUIRED_FOR_INVITATION",
                "Email verification required to view or list invitations for the session email",
            ));
        }
        let email = view
            .as_ref()
            .and_then(|view| view.user["email"].as_str())
            .filter(|s| !s.is_empty())
            .or(email)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| message("Missing session headers, or email query parameter."))?
            .to_lowercase();
        let mut tx = self.store.begin(false).await?;
        let mut invitations = Vec::new();
        for mut row in crate::store::find_all(&mut tx, Model::Invitation, "email", &email)
            .await?
            .into_iter()
            .filter(|row| row["email"] == email)
            .take(100)
        {
            if row["status"] != "pending" {
                continue;
            }
            if let Some(org) = tx
                .get(Model::Organization, text(&row, "organizationId")?)
                .await?
                && let Some(name) = org.get("name")
            {
                row["organizationName"] = name.clone();
            }
            invitations.push(row);
        }
        tx.commit().await?;
        Ok(json!(invitations))
    }
}
fn message(text: &str) -> BlueprintError {
    BlueprintError::new("HTTP_MESSAGE_ONLY", text)
}
