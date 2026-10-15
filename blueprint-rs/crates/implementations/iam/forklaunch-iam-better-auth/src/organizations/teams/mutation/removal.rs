use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model, Mutation,
    OrganizationService, Utc, Value, authorize, json, missing, organization, scope, text,
    unauthorized,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Deletes a non-active team, preserving the last team and removing team
    /// memberships and references from live pending invitations atomically.
    /// # Errors
    /// Returns session/scope/permission, missing/last team or storage failures.
    pub async fn remove_team(
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
        let org = scope(body, &view)?;
        let id = text(body, "teamId")?;
        authorize(&mut tx, &view, org, Mutation::Delete, Some(id)).await?;
        if tx
            .get(Model::Team, id)
            .await?
            .filter(|team| team["organizationId"] == org)
            .is_none()
        {
            return Err(missing());
        }
        if crate::store::find_all(&mut tx, Model::Team, "organizationId", org)
            .await?
            .iter()
            .filter(|row| row["organizationId"] == org)
            .count()
            <= 1
        {
            return Err(BlueprintError::new(
                "UNABLE_TO_REMOVE_LAST_TEAM",
                "Unable to remove last team",
            ));
        }
        organization(&mut tx, org).await?;
        for row in crate::store::find_all(&mut tx, Model::TeamMember, "teamId", id).await? {
            if row["teamId"] == id {
                tx.delete(Model::TeamMember, text(&row, "id")?).await?;
            }
        }
        tx.delete(Model::Team, id).await?;
        clean_invitations(&mut tx, org, id, now).await?;
        tx.commit().await?;
        Ok(json!({"message":"Team removed successfully."}))
    }
}
async fn clean_invitations<T: AuthTransaction>(
    tx: &mut T,
    org: &str,
    id: &str,
    now: DateTime<Utc>,
) -> BlueprintResult<()> {
    for mut row in crate::store::find_all(tx, Model::Invitation, "organizationId", org)
        .await?
        .into_iter()
        .filter(|row| row["organizationId"] == org && row["status"] == "pending")
        .take(100)
    {
        if row["expiresAt"]
            .as_str()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .is_none_or(|expires| expires <= now)
        {
            continue;
        }
        let Some(teams) = row["teamId"].as_str().filter(|value| !value.is_empty()) else {
            continue;
        };
        let teams: Vec<_> = teams.split(',').collect();
        if !teams.contains(&id) {
            continue;
        }
        let remaining: Vec<_> = teams.into_iter().filter(|team| *team != id).collect();
        row["teamId"] = if remaining.is_empty() {
            Value::Null
        } else {
            json!(remaining.join(","))
        };
        tx.replace(Model::Invitation, row).await?;
    }
    Ok(())
}
