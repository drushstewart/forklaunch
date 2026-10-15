use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, allowed, joined_member, json, missing, no_active, public_team,
    text, timestamp, unauthorized,
};
use crate::sessions::SessionView;
mod removal;

#[derive(Clone, Copy)]
enum Mutation {
    Create,
    Update,
    Delete,
}
impl Mutation {
    fn action(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
    fn denied(self, member: bool) -> BlueprintError {
        let (code, message) = match (self, member) {
            (Self::Create, true) => (
                "YOU_ARE_NOT_ALLOWED_TO_INVITE_USERS_TO_THIS_ORGANIZATION",
                "You are not allowed to invite users to this organization",
            ),
            (Self::Create, false) => (
                "YOU_ARE_NOT_ALLOWED_TO_CREATE_TEAMS_IN_THIS_ORGANIZATION",
                "You are not allowed to create teams in this organization",
            ),
            (Self::Update, _) => (
                "YOU_ARE_NOT_ALLOWED_TO_UPDATE_THIS_TEAM",
                "You are not allowed to update this team",
            ),
            (Self::Delete, true) => (
                "YOU_ARE_NOT_ALLOWED_TO_DELETE_THIS_TEAM",
                "You are not allowed to delete this team",
            ),
            (Self::Delete, false) => (
                "YOU_ARE_NOT_ALLOWED_TO_DELETE_TEAMS_IN_THIS_ORGANIZATION",
                "You are not allowed to delete teams in this organization",
            ),
        };
        BlueprintError::new(code, message)
    }
}
async fn authorize<T: AuthTransaction>(
    config: &crate::organizations::Authorization,
    tx: &mut T,
    view: &SessionView,
    id: &str,
    action: Mutation,
    target: Option<&str>,
) -> BlueprintResult<()> {
    let actor = joined_member(tx, text(&view.user, "id")?, id)
        .await?
        .ok_or_else(|| action.denied(true))?;
    if matches!(action, Mutation::Delete)
        && target.is_some_and(|target| view.session["activeTeamId"] == target)
    {
        return Err(action.denied(true));
    }
    if !allowed(
        config,
        tx,
        id,
        text(&actor, "role")?,
        Some(&json!({"team":[action.action()]})),
    )
    .await?
    {
        return Err(action.denied(false));
    }
    Ok(())
}
fn scope<'a>(body: &'a Value, view: &'a SessionView) -> BlueprintResult<&'a str> {
    body["organizationId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .or_else(|| {
            view.session["activeOrganizationId"]
                .as_str()
                .filter(|id| !id.is_empty())
        })
        .ok_or_else(no_active)
}
async fn organization<T: AuthTransaction>(tx: &mut T, id: &str) -> BlueprintResult<()> {
    if tx.get(Model::Organization, id).await?.is_none() {
        return Err(super::super::missing());
    }
    Ok(())
}
impl<S: AuthStore> OrganizationService<S> {
    /// Creates a scoped team after configured team:create permission checks.
    /// Creation does not automatically enroll the actor or select the team.
    /// # Errors
    /// Returns authoritative session/scope/permission or storage errors.
    pub async fn create_team(
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
        let id = scope(body, &view)?;
        authorize(
            &self.access_control,
            &mut tx,
            &view,
            id,
            Mutation::Create,
            None,
        )
        .await?;
        if let Some(hook) = &self.team_count_limit {
            let id = id.to_owned();
            let count = crate::store::find_all(&mut tx, Model::Team, "organizationId", &id)
                .await?
                .iter()
                .filter(|row| row["organizationId"] == id)
                .take(100)
                .count();
            tx.commit().await?;
            let mut adapter = super::super::team_callbacks::AutocommitAdapter(self.store.as_ref());
            let limit =
                super::super::team_callbacks::count(hook.as_ref(), &id, Some(&view), &mut adapter)
                    .await?;
            if limit != 0.0
                && !limit.is_nan()
                && f64::from(u32::try_from(count).map_err(|_| super::super::internal())?) >= limit
            {
                return Err(BlueprintError::new(
                    "YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_TEAMS",
                    "You have reached the maximum number of teams",
                ));
            }
            let mut tx = self.store.begin(true).await?;
            organization(&mut tx, &id).await?;
            let now = self.time_source.sample(now);
            let team = json!({"id":uuid::Uuid::new_v4().to_string(),"organizationId":id,"name":text(body,"name")?,"memberCount":0,"createdAt":timestamp(now),"updatedAt":timestamp(now)});
            tx.insert(Model::Team, team.clone()).await?;
            tx.commit().await?;
            return Ok(public_team(team));
        }
        if let Some(limit) = self.team_policy.maximum_teams.filter(|limit| *limit != 0) {
            let count = crate::store::find_all(&mut tx, Model::Team, "organizationId", id)
                .await?
                .iter()
                .filter(|row| row["organizationId"] == id)
                .take(100)
                .count();
            if u64::try_from(count).unwrap_or(u64::MAX) >= u64::from(limit) {
                return Err(BlueprintError::new(
                    "YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_TEAMS",
                    "You have reached the maximum number of teams",
                ));
            }
        }
        organization(&mut tx, id).await?;
        let team = json!({"id":uuid::Uuid::new_v4().to_string(),"organizationId":id,"name":text(body,"name")?,"memberCount":0,"createdAt":timestamp(now),"updatedAt":timestamp(now)});
        tx.insert(Model::Team, team.clone()).await?;
        tx.commit().await?;
        Ok(public_team(team))
    }
    /// Updates a team's public name without moving it to another organization.
    /// # Errors
    /// Returns authoritative session/scope/permission, missing team or store errors.
    pub async fn update_team(
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
        let id = scope(&body["data"], &view)?;
        authorize(
            &self.access_control,
            &mut tx,
            &view,
            id,
            Mutation::Update,
            None,
        )
        .await?;
        let mut team = tx
            .get(Model::Team, text(body, "teamId")?)
            .await?
            .filter(|row| row["organizationId"] == id)
            .ok_or_else(missing)?;
        organization(&mut tx, id).await?;
        if let Some(name) = body["data"].get("name") {
            team["name"] = name.clone();
        }
        team["updatedAt"] = json!(timestamp(now));
        tx.replace(Model::Team, team.clone()).await?;
        tx.commit().await?;
        Ok(public_team(team))
    }
}
