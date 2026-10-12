use super::{
    AuthStore, AuthTransaction, BlueprintResult, DateTime, Model, OrganizationService, Utc, Value,
    find_member, find_slug, joined, member_missing, missing, no_active, nonmember, text,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Reads the current user's membership and public user projection.
    /// # Errors
    /// Returns session, active-organization or missing-membership errors.
    pub async fn active_member(
        &self,
        token: &str,
        transient: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let view = self.session(token, transient, now).await?;
        let id = view.session["activeOrganizationId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(no_active)?;
        let mut tx = self.store.begin(false).await?;
        let member = find_member(&mut tx, text(&view.user, "id")?, id)
            .await?
            .ok_or_else(member_missing)?;
        let member = joined(&mut tx, member).await?.ok_or_else(member_missing)?;
        tx.commit().await?;
        Ok(member)
    }
    /// Reads a member's role after verifying the actor belongs to that organization.
    /// # Errors
    /// Returns session, organization selection or membership errors.
    pub async fn member_role(
        &self,
        token: &str,
        transient: bool,
        query: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let view = self.session(token, transient, now).await?;
        let mut tx = self.store.begin(false).await?;
        let id = if let Some(slug) = query["organizationSlug"]
            .as_str()
            .filter(|slug| !slug.is_empty())
        {
            text(&find_slug(&mut tx, slug).await?.ok_or_else(missing)?, "id")?.to_owned()
        } else {
            query["organizationId"]
                .as_str()
                .filter(|id| !id.is_empty())
                .or_else(|| {
                    view.session["activeOrganizationId"]
                        .as_str()
                        .filter(|id| !id.is_empty())
                })
                .ok_or_else(no_active)?
                .to_owned()
        };
        let actor = find_member(&mut tx, text(&view.user, "id")?, &id)
            .await?
            .ok_or_else(nonmember)?;
        if tx
            .get(Model::User, text(&actor, "userId")?)
            .await?
            .is_none()
        {
            return Err(nonmember());
        }
        let role = if let Some(user) = query["userId"].as_str().filter(|user| !user.is_empty()) {
            let target = find_member(&mut tx, user, &id)
                .await?
                .ok_or_else(nonmember)?;
            if tx
                .get(Model::User, text(&target, "userId")?)
                .await?
                .is_none()
            {
                return Err(nonmember());
            }
            target["role"].clone()
        } else {
            actor["role"].clone()
        };
        tx.commit().await?;
        Ok(serde_json::json!({"role":role}))
    }
}
