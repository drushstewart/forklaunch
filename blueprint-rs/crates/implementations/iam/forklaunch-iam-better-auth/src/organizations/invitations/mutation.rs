use super::super::permission::allowed;
use super::super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, json, missing, text, unauthorized,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Rejects a pending invitation; expiry does not prevent rejection.
    /// # Errors
    /// Requires its verified recipient and existing organization.
    pub async fn reject_invitation(
        &self,
        token: &str,
        transient: bool,
        id: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.session(token, transient, now).await?;
        let mut tx = self.store.begin(true).await?;
        let view = self
            .recheck(&mut tx, token, now)
            .await?
            .ok_or_else(unauthorized)?;
        let mut invitation = tx
            .get(Model::Invitation, id)
            .await?
            .filter(|row| row["status"] == "pending")
            .ok_or_else(|| BlueprintError::new("INVITATION_NOT_FOUND", "Invitation not found!"))?;
        if text(&invitation, "email")?.to_lowercase() != text(&view.user, "email")?.to_lowercase() {
            return Err(BlueprintError::new(
                "YOU_ARE_NOT_THE_RECIPIENT_OF_THE_INVITATION",
                "You are not the recipient of the invitation",
            ));
        }
        if self.invitation_policy.require_verified_email && view.user["emailVerified"] != true {
            return Err(BlueprintError::new(
                "EMAIL_VERIFICATION_REQUIRED_BEFORE_ACCEPTING_OR_REJECTING_INVITATION",
                "Email verification required before accepting or rejecting invitation",
            ));
        }
        tx.get(Model::Organization, text(&invitation, "organizationId")?)
            .await?
            .ok_or_else(missing)?;
        invitation["status"] = json!("rejected");
        tx.replace(Model::Invitation, invitation.clone()).await?;
        tx.commit().await?;
        self.organization_features.invitation(&mut invitation);
        Ok(json!({"invitation":invitation,"member":null}))
    }
    /// Cancels an invitation regardless of status/expiry, with a scoped grant.
    /// # Errors
    /// Requires joined membership, invitation:cancel and existing organization.
    pub async fn cancel_invitation(
        &self,
        token: &str,
        transient: bool,
        id: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.session(token, transient, now).await?;
        let mut tx = self.store.begin(true).await?;
        let view = self
            .recheck(&mut tx, token, now)
            .await?
            .ok_or_else(unauthorized)?;
        let mut invitation = tx
            .get(Model::Invitation, id)
            .await?
            .ok_or_else(|| BlueprintError::new("INVITATION_NOT_FOUND", "Invitation not found"))?;
        let org = text(&invitation, "organizationId")?;
        let member = crate::store::find_all(&mut tx, Model::Member, "organizationId", org)
            .await?
            .into_iter()
            .find(|row| row["organizationId"] == org && row["userId"] == view.user["id"])
            .ok_or_else(|| BlueprintError::new("MEMBER_NOT_FOUND", "Member not found"))?;
        if !allowed(
            &self.access_control,
            &mut tx,
            org,
            text(&member, "role")?,
            Some(&json!({"invitation":["cancel"]})),
        )
        .await?
        {
            return Err(BlueprintError::new(
                "YOU_ARE_NOT_ALLOWED_TO_CANCEL_THIS_INVITATION",
                "You are not allowed to cancel this invitation",
            ));
        }
        tx.get(Model::Organization, org)
            .await?
            .ok_or_else(missing)?;
        invitation["status"] = json!("canceled");
        tx.replace(Model::Invitation, invitation.clone()).await?;
        tx.commit().await?;
        self.organization_features.invitation(&mut invitation);
        Ok(invitation)
    }
}
