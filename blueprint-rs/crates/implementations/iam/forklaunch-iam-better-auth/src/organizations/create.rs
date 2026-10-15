//! Atomic organization, owner, default team and session creation.
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Engine, Model,
    OrganizationService, URL_SAFE_NO_PAD, Utc, Value, find_slug, internal, json, text, timestamp,
    unauthorized, update_active,
};

impl<S: AuthStore> OrganizationService<S> {
    /// Creates an organization and its default owner/team memberships atomically.
    /// The HTTP adapter validates the configured input schema. Caller-supplied
    /// user IDs never replace the authenticated creator.
    /// # Errors
    /// Returns session/duplicate-slug or sanitized storage errors.
    pub async fn create(
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
        if find_slug(&mut tx, text(body, "slug")?).await?.is_some() {
            return Err(BlueprintError::new(
                "ORGANIZATION_ALREADY_EXISTS",
                "Organization already exists",
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let created = timestamp(now);
        let mut organization = json!({"id":id,"name":text(body,"name")?,"slug":text(body,"slug")?,"createdAt":created,"subscription":"free","status":"active"});
        for field in ["logo", "domain", "subscription", "status"] {
            if let Some(value) = body.get(field) {
                organization[field] = value.clone();
            }
        }
        if let Some(metadata) = body.get("metadata") {
            organization["metadata"] = json!(crate::ecmascript::stringify(metadata));
        }
        tx.insert(Model::Organization, organization.clone()).await?;
        let user = text(&view.user, "id")?;
        let owner = json!({"id":uuid::Uuid::new_v4().to_string(),"organizationId":id,"userId":user,"role":"owner","createdAt":created});
        tx.insert(Model::Member, owner.clone()).await?;
        let team_id = uuid::Uuid::new_v4().to_string();
        tx.insert(Model::Team,json!({"id":team_id,"organizationId":id,"name":organization["name"],"memberCount":1,"createdAt":created})).await?;
        let pair = serde_json::to_vec(&json!([team_id, user])).map_err(|_| internal())?;
        let membership =
            URL_SAFE_NO_PAD.encode(ring::digest::digest(&ring::digest::SHA256, &pair).as_ref());
        tx.insert(Model::TeamMember,json!({"id":uuid::Uuid::new_v4().to_string(),"teamId":team_id,"userId":user,"membershipKey":membership,"createdAt":created})).await?;
        if body["keepCurrentActiveOrganization"] != true {
            update_active(&mut tx, &view, Some(&id), Some(&team_id), now).await?;
        }
        tx.commit().await?;
        if let Some(metadata) = body.get("metadata") {
            organization["metadata"] = metadata.clone();
        }
        organization["members"] = json!([owner]);
        Ok(organization)
    }
}
