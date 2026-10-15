//! Scoped organization mutation and dynamically granted permissions.
use super::permission::allowed;
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, find_slug, internal, json, missing, not_member, text,
    unauthorized, update_active,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Checks the configured permission set, loading dynamic grants each time.
    /// # Errors
    /// Returns session, membership, missing-selection or malformed-grant errors.
    pub async fn has_permission(
        &self,
        token: &str,
        transient: bool,
        body: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let view = self.session(token, transient, now).await?;
        let id = body["organizationId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .or_else(|| {
                view.session["activeOrganizationId"]
                    .as_str()
                    .filter(|id| !id.is_empty())
            })
            .ok_or_else(|| {
                BlueprintError::new("NO_ACTIVE_ORGANIZATION", "No active organization")
            })?;
        let mut tx = self.store.begin(false).await?;
        let member = find_member(&mut tx, text(&view.user, "id")?, id)
            .await?
            .ok_or_else(not_member)?;
        let success = allowed(
            &self.access_control,
            &mut tx,
            id,
            text(&member, "role")?,
            body.get("permissions"),
        )
        .await?;
        tx.commit().await?;
        Ok(json!({"error":null,"success":success}))
    }
    /// Updates only configured fields after an authoritative dynamic role check.
    /// # Errors
    /// Returns session, organization, membership, permission or storage errors.
    pub async fn update(
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
            .ok_or_else(missing)?;
        let member = find_member(&mut tx, text(&view.user, "id")?, id)
            .await?
            .ok_or_else(not_member)?;
        if !allowed(
            &self.access_control,
            &mut tx,
            id,
            text(&member, "role")?,
            Some(&json!({"organization":["update"]})),
        )
        .await?
        {
            return Err(BlueprintError::new(
                "YOU_ARE_NOT_ALLOWED_TO_UPDATE_THIS_ORGANIZATION",
                "You are not allowed to update this organization",
            ));
        }
        let data = &body["data"];
        if let Some(slug) = data["slug"].as_str()
            && find_slug(&mut tx, slug)
                .await?
                .is_some_and(|row| row["id"] != id)
        {
            return Err(BlueprintError::new(
                "ORGANIZATION_SLUG_ALREADY_TAKEN",
                "Organization slug already taken",
            ));
        }
        let Some(mut organization) = tx.get(Model::Organization, id).await? else {
            tx.commit().await?;
            return Ok(Value::Null);
        };
        for field in ["name", "slug", "logo", "domain", "subscription", "status"] {
            if let Some(value) = data.get(field) {
                organization[field] = value.clone();
            }
        }
        if let Some(metadata) = data.get("metadata") {
            organization["metadata"] = json!(crate::ecmascript::stringify(metadata));
        }
        tx.replace(Model::Organization, organization.clone())
            .await?;
        tx.commit().await?;
        if let Some(metadata) = organization.get("metadata").and_then(Value::as_str) {
            let mut parsed: Value = crate::ecmascript::parse(metadata).map_err(|_| internal())?;
            if crate::ecmascript::unsafe_metadata(&parsed) {
                return Err(BlueprintError::new(
                    "HTTP_INTERNAL_EMPTY",
                    "Invalid organization metadata",
                ));
            }
            crate::ecmascript::revive_dates(&mut parsed);
            organization["metadata"] = parsed;
        }
        Ok(organization)
    }
    /// Deletes the organization and configured member/invitation records.
    /// The configured adapter retains team/role rows and other sessions' active
    /// selections; this operation only clears its own matching selection.
    /// # Errors
    /// Returns session, membership, permission or sanitized persistence errors.
    pub async fn delete(
        &self,
        token: &str,
        transient: bool,
        id: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.controls.check_deletion()?;
        self.session(token, transient, now).await?;
        if id.is_empty() {
            return Err(missing());
        }
        let mut tx = self.store.begin(true).await?;
        let view = self
            .recheck(&mut tx, token, now)
            .await?
            .ok_or_else(unauthorized)?;
        let member = find_member(&mut tx, text(&view.user, "id")?, id)
            .await?
            .ok_or_else(not_member)?;
        if !allowed(
            &self.access_control,
            &mut tx,
            id,
            text(&member, "role")?,
            Some(&json!({"organization":["delete"]})),
        )
        .await?
        {
            return Err(BlueprintError::new(
                "YOU_ARE_NOT_ALLOWED_TO_DELETE_THIS_ORGANIZATION",
                "You are not allowed to delete this organization",
            ));
        }
        if view.session["activeOrganizationId"] == id {
            update_active(&mut tx, &view, None, None, now).await?;
        }
        let Some(organization) = tx.get(Model::Organization, id).await? else {
            tx.commit().await?;
            return Err(BlueprintError::new("HTTP_BAD_REQUEST_EMPTY", "Bad Request"));
        };
        for model in [Model::Member, Model::Invitation] {
            for row in crate::store::find_all(&mut tx, model, "organizationId", id).await? {
                if row["organizationId"] == id {
                    tx.delete(model, text(&row, "id")?).await?;
                }
            }
        }
        tx.delete(Model::Organization, id).await?;
        tx.commit().await?;
        Ok(organization)
    }
}
pub(super) async fn find_member<T: AuthTransaction>(
    tx: &mut T,
    user: &str,
    organization: &str,
) -> BlueprintResult<Option<Value>> {
    Ok(crate::store::find_all(tx, Model::Member, "userId", user)
        .await?
        .into_iter()
        .find(|row| row["userId"] == user && row["organizationId"] == organization))
}
