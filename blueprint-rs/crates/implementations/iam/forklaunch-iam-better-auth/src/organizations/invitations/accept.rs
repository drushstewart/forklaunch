//! Atomic invitation acceptance, team enrollment and active scope.
use super::super::teams::add_team_member;
use super::super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationSelection, OrganizationService, Utc, json, missing, text, timestamp, unauthorized,
    update_active,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Accepts a pending unexpired invitation for a verified recipient.
    /// A single team emits a session cookie; no/multiple teams retain selection.
    /// # Errors
    /// Returns recipient, expiry, membership limit, scope or storage failures.
    /// Failed enrollment rolls back invitation, team, member and session writes.
    pub async fn accept_invitation(
        &self,
        token: &str,
        transient: bool,
        id: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<OrganizationSelection> {
        self.session(token, transient, now).await?;
        let mut tx = self.store.begin(true).await?;
        let view = self
            .recheck(&mut tx, token, now)
            .await?
            .ok_or_else(unauthorized)?;
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
            .ok_or_else(|| BlueprintError::new("INVITATION_NOT_FOUND", "Invitation not found"))?;
        if text(&invitation, "email")?.to_lowercase() != text(&view.user, "email")?.to_lowercase() {
            return Err(BlueprintError::new(
                "YOU_ARE_NOT_THE_RECIPIENT_OF_THE_INVITATION",
                "You are not the recipient of the invitation",
            ));
        }
        if view.user["emailVerified"] != true {
            return Err(BlueprintError::new(
                "EMAIL_VERIFICATION_REQUIRED_BEFORE_ACCEPTING_OR_REJECTING_INVITATION",
                "Email verification required before accepting or rejecting invitation",
            ));
        }
        let org = text(&invitation, "organizationId")?.to_owned();
        let count = crate::store::find_all(&mut tx, Model::Member, "organizationId", &org)
            .await?
            .iter()
            .filter(|row| row["organizationId"] == org)
            .count();
        tx.get(Model::Organization, &org)
            .await?
            .ok_or_else(missing)?;
        if count >= 100 {
            return Err(BlueprintError::new(
                "ORGANIZATION_MEMBERSHIP_LIMIT_REACHED",
                "Organization membership limit reached",
            ));
        }
        invitation["status"] = json!("accepted");
        tx.replace(Model::Invitation, invitation.clone()).await?;
        let teams = invitation["teamId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map_or_else(Vec::new, |s| s.split(',').collect::<Vec<_>>());
        for id in &teams {
            let team = tx
                .get(Model::Team, id)
                .await?
                .filter(|row| row["organizationId"] == org)
                .ok_or_else(|| BlueprintError::new("TEAM_NOT_FOUND", "Team not found"))?;
            add_team_member(&mut tx, team, text(&view.user, "id")?, now).await?;
        }
        let member = json!({"id":uuid::Uuid::new_v4().to_string(),"organizationId":org,"userId":view.user["id"],"role":invitation["role"],"createdAt":timestamp(now)});
        tx.insert(Model::Member, member.clone()).await?;
        let cookie = teams.len() == 1;
        update_active(
            &mut tx,
            &view,
            Some(&org),
            if cookie { teams.first().copied() } else { None },
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(OrganizationSelection {
            body: json!({"invitation":invitation,"member":member}),
            cookie,
        })
    }
}
