use super::{
    AuthStore, AuthTransaction, BlueprintResult, DateTime, Model, OrganizationSelection,
    OrganizationService, Utc, Value, json, missing, no_active, no_team_member, public_team,
    team_member, text, timestamp, unauthorized,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Selects/clears a team for this session, preserving organization selection.
    /// A successful selection emits the session cookie, including repeat selection.
    /// # Errors
    /// Returns authoritative session, organization, team or membership errors.
    pub async fn set_active_team(
        &self,
        token: &str,
        transient: bool,
        body: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<OrganizationSelection> {
        self.session(token, transient, now).await?;
        let mut tx = self.store.begin(true).await?;
        let view = self
            .recheck(&mut tx, token, now)
            .await?
            .ok_or_else(unauthorized)?;
        let current = view.session["activeTeamId"]
            .as_str()
            .filter(|id| !id.is_empty());
        let clear = body.get("teamId") == Some(&Value::Null);
        let id = if clear {
            None
        } else {
            body["teamId"]
                .as_str()
                .filter(|id| !id.is_empty())
                .or(current)
        };
        let (result, cookie) = if let Some(id) = id {
            let org = view.session["activeOrganizationId"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(no_active)?;
            let team = tx
                .get(Model::Team, id)
                .await?
                .filter(|row| row["organizationId"] == org)
                .ok_or_else(missing)?;
            if !team_member(&mut tx, id, text(&view.user, "id")?).await? {
                return Err(no_team_member());
            }
            (public_team(team), true)
        } else {
            (Value::Null, clear && current.is_some())
        };
        if cookie {
            let mut session = view.session.clone();
            session["activeTeamId"] = json!(id);
            session["updatedAt"] = json!(timestamp(now));
            tx.replace(Model::Session, session).await?;
        }
        tx.commit().await?;
        Ok(OrganizationSelection {
            body: result,
            cookie,
        })
    }
}
