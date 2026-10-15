use super::super::{DateTime, OrganizationSelection, Utc, json, missing, text, timestamp};
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, MembershipLimitPolicy, Model,
    OrganizationService, count, failure, invoke,
};
impl<S: AuthStore> OrganizationService<S> {
    pub(in crate::organizations) async fn accept_with_policy(
        &self,
        token: &str,
        transient: bool,
        id: &str,
        now: DateTime<Utc>,
        hook: &dyn MembershipLimitPolicy,
    ) -> BlueprintResult<OrganizationSelection> {
        let view = self.session(token, transient, now).await?;
        let mut tx = self.store.begin(false).await?;
        let invitation = tx
            .get(Model::Invitation, id)
            .await?
            .filter(|row| {
                row["status"] == "pending"
                    && row["expiresAt"]
                        .as_str()
                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        .is_some_and(|expiry| expiry >= now)
            })
            .ok_or_else(not_found)?;
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
        let org = text(&invitation, "organizationId")?;
        let count = count(&mut tx, org).await?;
        let organization = tx
            .get(Model::Organization, org)
            .await?
            .ok_or_else(missing)?;
        tx.commit().await?;
        invoke(hook, &view.user, &organization, count).await?;
        let mut tx = self.store.begin(true).await?;
        // Upstream CAS only checks current pending status and returns the current row.
        let mut invitation = tx
            .get(Model::Invitation, id)
            .await?
            .filter(|row| row["status"] == "pending")
            .ok_or_else(not_found)?;
        invitation["status"] = json!("accepted");
        tx.replace(Model::Invitation, invitation.clone()).await?;
        let org = text(&invitation, "organizationId")?;
        let teams = invitation["teamId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map_or_else(Vec::new, |s| s.split(',').collect::<Vec<_>>());
        let now = self.time_source.sample(now);
        for id in &teams {
            self.enroll_accepted_team(&mut tx, org.as_ref(), id, &view, now)
                .await?;
        }
        let now = if self.team_member_limit.is_some() {
            self.time_source.sample(now)
        } else {
            now
        };
        let cookie = teams.len() == 1;
        if let Some(mut session) = tx.find_unique(Model::Session, "token", token).await? {
            session["activeOrganizationId"] = json!(org);
            if cookie {
                session["activeTeamId"] = json!(teams[0]);
            }
            session["updatedAt"] = json!(timestamp(now));
            tx.replace(Model::Session, session).await?;
        } else if cookie {
            return Err(failure());
        }
        let member = json!({"id":uuid::Uuid::new_v4().to_string(),"organizationId":org,"userId":view.user["id"],"role":invitation["role"],"createdAt":timestamp(now)});
        tx.insert(Model::Member, member.clone()).await?;
        tx.commit().await?;
        Ok(OrganizationSelection {
            body: json!({"invitation":invitation,"member":member}),
            cookie,
        })
    }
}
fn not_found() -> BlueprintError {
    BlueprintError::new("INVITATION_NOT_FOUND", "Invitation not found")
}
