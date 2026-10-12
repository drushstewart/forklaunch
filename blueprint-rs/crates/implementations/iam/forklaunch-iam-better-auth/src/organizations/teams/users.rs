use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, OrganizationService,
    Utc, Value, allowed, joined_member, json, member, no_active, nonmember, text, user_teams,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Lists self teams across member organizations, or another member's teams
    /// with the configured scoped member:update permission.
    /// # Errors
    /// Returns session/scope/membership/permission or missing joined-team errors.
    pub async fn list_user_teams(
        &self,
        token: &str,
        transient: bool,
        query: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let view = self.session(token, transient, now).await?;
        let user = text(&view.user, "id")?;
        let target = query["userId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .unwrap_or(user);
        let explicit = query["organizationId"].as_str().filter(|id| !id.is_empty());
        let org = explicit.or_else(|| {
            view.session["activeOrganizationId"]
                .as_str()
                .filter(|id| !id.is_empty())
        });
        let mut tx = self.store.begin(false).await?;
        if target != user {
            let org = org.ok_or_else(no_active)?;
            let actor = joined_member(&mut tx, user, org)
                .await?
                .ok_or_else(nonmember)?;
            if !allowed(
                &mut tx,
                org,
                text(&actor, "role")?,
                Some(&json!({"member":["update"]})),
            )
            .await?
            {
                return Err(BlueprintError::new(
                    "YOU_ARE_NOT_ALLOWED_TO_UPDATE_THIS_MEMBER",
                    "You are not allowed to update this member",
                ));
            }
            if joined_member(&mut tx, target, org).await?.is_none() {
                return Err(BlueprintError::new(
                    "USER_IS_NOT_A_MEMBER_OF_THE_ORGANIZATION",
                    "User is not a member of the organization",
                ));
            }
        } else if let Some(org) = explicit
            && joined_member(&mut tx, user, org).await?.is_none()
        {
            return Err(nonmember());
        }
        let teams = user_teams(&mut tx, target).await?;
        let mut result = Vec::new();
        for team in teams {
            let include = if target != user || explicit.is_some() {
                Some(text(&team, "organizationId")?) == org
            } else {
                member(&mut tx, user, text(&team, "organizationId")?).await?
            };
            if include {
                result.push(team);
            }
        }
        tx.commit().await?;
        Ok(json!(result))
    }
}
