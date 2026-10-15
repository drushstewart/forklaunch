use super::super::permission::allowed;
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, delete_member, find_member, joined, json, member_missing,
    missing, no_active, only_owner, owner_count, owns, text, trim, unauthorized, update_active,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Removes a scoped member and team memberships with the configured owner
    /// safeguards, permission ordering and email-versus-ID response projections.
    /// # Errors
    /// Returns authoritative session, member, creator or permission errors.
    pub async fn remove_member(
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
        let id = body["organizationId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .or_else(|| {
                view.session["activeOrganizationId"]
                    .as_str()
                    .filter(|id| !id.is_empty())
            })
            .ok_or_else(no_active)?;
        let actor = find_member(&mut tx, text(&view.user, "id")?, id)
            .await?
            .ok_or_else(member_missing)?;
        let target = removal_target(&mut tx, text(body, "memberIdOrEmail")?, id)
            .await?
            .ok_or_else(member_missing)?;
        if owns(&target, self.creator_role.as_str())
            && (!text(&actor, "role")?
                .split(',')
                .map(trim)
                .any(|role| role == self.creator_role.as_str())
                || owner_count(&mut tx, id, self.creator_role.as_str()).await? <= 1)
        {
            return Err(only_owner());
        }
        if !allowed(
            &self.access_control,
            &mut tx,
            id,
            text(&actor, "role")?,
            Some(&json!({"member":["delete"]})),
        )
        .await?
        {
            return Err(BlueprintError::new(
                "YOU_ARE_NOT_ALLOWED_TO_DELETE_THIS_MEMBER",
                "You are not allowed to delete this member",
            ));
        }
        if target["organizationId"] != id {
            return Err(member_missing());
        }
        if tx.get(Model::Organization, id).await?.is_none() {
            return Err(missing());
        }
        if tx
            .get(Model::User, text(&target, "userId")?)
            .await?
            .is_none()
        {
            return Err(BlueprintError::new("HTTP_MESSAGE_ONLY", "User not found"));
        }
        delete_member(&mut tx, &target).await?;
        if target["userId"] == view.user["id"] && view.session["activeOrganizationId"] == id {
            update_active(&mut tx, &view, None, None, now).await?;
        }
        tx.commit().await?;
        Ok(json!({"member":target}))
    }
}
async fn removal_target<T: AuthTransaction>(
    tx: &mut T,
    target: &str,
    organization: &str,
) -> BlueprintResult<Option<Value>> {
    if target.contains('@') {
        let Some(user) = tx
            .find_unique(Model::User, "email", &target.to_lowercase())
            .await?
        else {
            return Ok(None);
        };
        let Some(member) = find_member(tx, text(&user, "id")?, organization).await? else {
            return Ok(None);
        };
        joined(tx, member).await
    } else {
        let Some(member) = tx.get(Model::Member, target).await? else {
            return Ok(None);
        };
        if tx
            .get(Model::User, text(&member, "userId")?)
            .await?
            .is_none()
        {
            return Err(BlueprintError::new(
                "HTTP_INTERNAL_EMPTY",
                "Member user not found",
            ));
        }
        Ok(Some(member))
    }
}
