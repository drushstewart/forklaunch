//! Organization membership checks and active session selection.
use super::{
    AuthStore, AuthTransaction, BlueprintResult, DateTime, Model, OrganizationSelection,
    OrganizationService, Utc, Value, find_slug, member, missing, not_member, text, unauthorized,
    update_active,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Selects/unsets an organization and preserves the active team exactly as
    /// the configured handler does. Rejection clears the prior organization.
    /// # Errors
    /// Returns authoritative session, organization, membership or storage errors.
    pub async fn set_active(
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
        if body.get("organizationId") == Some(&Value::Null) {
            let cookie = view.session["activeOrganizationId"]
                .as_str()
                .is_some_and(|id| !id.is_empty());
            if cookie {
                update_active(&mut tx, &view, None, None, now).await?;
            }
            tx.commit().await?;
            return Ok(OrganizationSelection {
                body: Value::Null,
                cookie,
            });
        }
        let mut id = body["organizationId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(str::to_owned);
        let slug = body["organizationSlug"]
            .as_str()
            .filter(|slug| !slug.is_empty());
        if id.is_none() {
            id = if let Some(slug) = slug {
                Some(text(&find_slug(&mut tx, slug).await?.ok_or_else(missing)?, "id")?.to_owned())
            } else {
                view.session["activeOrganizationId"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
            };
        }
        let Some(id) = id else {
            tx.commit().await?;
            return Ok(OrganizationSelection {
                body: Value::Null,
                cookie: false,
            });
        };
        if !member(&mut tx, text(&view.user, "id")?, &id).await? {
            update_active(&mut tx, &view, None, None, now).await?;
            tx.commit().await?;
            return Err(not_member());
        }
        let organization = tx
            .get(Model::Organization, &id)
            .await?
            .ok_or_else(missing)?;
        update_active(&mut tx, &view, Some(&id), None, now).await?;
        tx.commit().await?;
        Ok(OrganizationSelection {
            body: organization,
            cookie: true,
        })
    }
}
