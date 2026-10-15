use super::{
    AuthStore, AuthTransaction, BlueprintResult, DateTime, OrganizationService, Utc, Value,
    delete_member, find_member, joined, member_missing, only_owner, owner_count, owns, text,
    unauthorized, update_active,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Leaves an organization without removing its last owner. Team memberships
    /// and counters update atomically; only this session's organization is cleared.
    /// # Errors
    /// Returns session, missing-member, only-owner or storage errors.
    pub async fn leave(
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
        let member = find_member(&mut tx, text(&view.user, "id")?, id)
            .await?
            .ok_or_else(member_missing)?;
        let member = joined(&mut tx, member).await?.ok_or_else(member_missing)?;
        if owns(&member, self.creator_role.as_str())
            && owner_count(&mut tx, id, self.creator_role.as_str()).await? <= 1
        {
            return Err(only_owner());
        }
        delete_member(&mut tx, &member).await?;
        if view.session["activeOrganizationId"] == id {
            update_active(&mut tx, &view, None, None, now).await?;
        }
        tx.commit().await?;
        Ok(member)
    }
}
