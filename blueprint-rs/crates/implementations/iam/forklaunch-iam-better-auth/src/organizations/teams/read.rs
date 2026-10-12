use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, joined_member, json, member, missing, no_active,
    no_team_member, public_member, public_team, team_member, text,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Lists an organization's first 100 teams for an authenticated member.
    /// # Errors
    /// Returns session, scope, membership or store errors.
    pub async fn list_teams(
        &self,
        token: &str,
        transient: bool,
        query: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let view = self.session(token, transient, now).await?;
        let id = query["organizationId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .or_else(|| {
                view.session["activeOrganizationId"]
                    .as_str()
                    .filter(|id| !id.is_empty())
            })
            .ok_or_else(no_active)?;
        let mut tx = self.store.begin(false).await?;
        if joined_member(&mut tx, text(&view.user, "id")?, id)
            .await?
            .is_none()
        {
            return Err(BlueprintError::new(
                "YOU_ARE_NOT_ALLOWED_TO_ACCESS_THIS_ORGANIZATION",
                "You are not allowed to access this organization as an owner",
            ));
        }
        let teams: Vec<_> = tx
            .list(Model::Team)
            .await?
            .into_iter()
            .filter(|row| row["organizationId"] == id)
            .take(100)
            .map(public_team)
            .collect();
        tx.commit().await?;
        Ok(json!(teams))
    }
    /// Lists the first 100 team memberships after both organization and team checks.
    /// Membership keys remain internal; user records are not joined by this API.
    /// # Errors
    /// Returns session, missing team, membership or storage failures.
    pub async fn list_team_members(
        &self,
        token: &str,
        transient: bool,
        query: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let view = self.session(token, transient, now).await?;
        let id = query["teamId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .or_else(|| {
                view.session["activeTeamId"]
                    .as_str()
                    .filter(|id| !id.is_empty())
            })
            .ok_or_else(|| {
                BlueprintError::new(
                    "YOU_DO_NOT_HAVE_AN_ACTIVE_TEAM",
                    "You do not have an active team",
                )
            })?;
        let mut tx = self.store.begin(false).await?;
        let team = tx.get(Model::Team, id).await?.ok_or_else(missing)?;
        let user = text(&view.user, "id")?;
        if !member(&mut tx, user, text(&team, "organizationId")?).await?
            || !team_member(&mut tx, id, user).await?
        {
            return Err(no_team_member());
        }
        let rows: Vec<_> = tx
            .list(Model::TeamMember)
            .await?
            .into_iter()
            .filter(|row| row["teamId"] == id)
            .take(100)
            .map(public_member)
            .collect();
        tx.commit().await?;
        Ok(json!(rows))
    }
}
