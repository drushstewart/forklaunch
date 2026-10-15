use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, add_team_member, allowed, joined_member, json, missing,
    no_active, no_team_member, public_member, text, unauthorized,
};
use crate::sessions::SessionView;

impl<S: AuthStore> OrganizationService<S> {
    /// Adds an organization's member to a scoped team. Duplicate requests reuse
    /// the existing membership; counters and keyed creation update atomically.
    /// # Errors
    /// Returns session/scope/membership/permission or storage failures.
    pub async fn add_team_member(
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
        let team = authorized_team(&self.access_control, &mut tx, &view, body, false).await?;
        if let Some(hook) = &self.team_member_limit {
            let org = text(&team, "organizationId")?.to_owned();
            let id = text(&team, "id")?.to_owned();
            tx.commit().await?;
            let mut adapter = super::super::team_callbacks::AutocommitAdapter(self.store.as_ref());
            let limit =
                super::super::team_callbacks::member(hook.as_ref(), &org, &id, &view, &mut adapter)
                    .await?;
            let mut tx = self.store.begin(true).await?;
            let row = super::super::team_callbacks::seat(
                &mut tx,
                &id,
                text(body, "userId")?,
                self.time_source.sample(now),
                limit,
            )
            .await?;
            tx.commit().await?;
            return Ok(public_member(row));
        }
        let row = add_team_member(
            &mut tx,
            team,
            text(body, "userId")?,
            now,
            self.team_policy.maximum_members,
        )
        .await?;
        tx.commit().await?;
        Ok(public_member(row))
    }
    /// Removes all matching team memberships, releasing only available stored
    /// seats. Active-team references in sessions are preserved by this API.
    /// # Errors
    /// Returns session/scope/membership/permission or storage failures.
    pub async fn remove_team_member(
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
        let mut team = authorized_team(&self.access_control, &mut tx, &view, body, true).await?;
        let rows: Vec<_> =
            crate::store::find_all(&mut tx, Model::TeamMember, "teamId", text(body, "teamId")?)
                .await?
                .into_iter()
                .filter(|row| row["teamId"] == body["teamId"] && row["userId"] == body["userId"])
                .collect();
        if rows.is_empty() {
            return Err(no_team_member());
        }
        let removed = i64::try_from(rows.len()).map_err(|_| super::super::internal())?;
        for row in rows {
            tx.delete(Model::TeamMember, text(&row, "id")?).await?;
        }
        if let Some(count) = team["memberCount"]
            .as_i64()
            .filter(|count| *count >= removed)
        {
            team["memberCount"] = json!(count - removed);
            tx.replace(Model::Team, team).await?;
        }
        tx.commit().await?;
        Ok(json!({"message":"Team member removed successfully."}))
    }
}
async fn authorized_team<T: AuthTransaction>(
    config: &crate::organizations::Authorization,
    tx: &mut T,
    view: &SessionView,
    body: &Value,
    remove: bool,
) -> BlueprintResult<Value> {
    let id = body["organizationId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .or_else(|| {
            view.session["activeOrganizationId"]
                .as_str()
                .filter(|id| !id.is_empty())
        })
        .ok_or_else(no_active)?;
    let actor = joined_member(tx, text(&view.user, "id")?, id)
        .await?
        .ok_or_else(nonmember)?;
    let action = if remove { "delete" } else { "update" };
    if !allowed(
        config,
        tx,
        id,
        text(&actor, "role")?,
        Some(&json!({"member":[action]})),
    )
    .await?
    {
        let (code, message) = if remove {
            (
                "YOU_ARE_NOT_ALLOWED_TO_REMOVE_A_TEAM_MEMBER",
                "You are not allowed to remove a team member",
            )
        } else {
            (
                "YOU_ARE_NOT_ALLOWED_TO_CREATE_A_NEW_TEAM_MEMBER",
                "You are not allowed to create a new member",
            )
        };
        return Err(BlueprintError::new(code, message));
    }
    if joined_member(tx, text(body, "userId")?, id)
        .await?
        .is_none()
    {
        return Err(nonmember());
    }
    let team = tx
        .get(Model::Team, text(body, "teamId")?)
        .await?
        .filter(|row| row["organizationId"] == id)
        .ok_or_else(missing)?;
    if tx.get(Model::Organization, id).await?.is_none() {
        return Err(super::super::missing());
    }
    Ok(team)
}
fn nonmember() -> BlueprintError {
    BlueprintError::new(
        "USER_IS_NOT_A_MEMBER_OF_THE_ORGANIZATION",
        "User is not a member of the organization",
    )
}
