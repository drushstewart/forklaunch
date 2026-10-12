//! Full organization graph, scoped membership and configured joins.
use super::{
    AuthStore, AuthTransaction, BlueprintResult, DateTime, Model, OrganizationService, Utc, Value,
    find_slug, internal, json, load, member, missing, not_member, text, unauthorized,
    update_active,
};
enum Graph {
    Metadata,
    Full(Option<i64>),
}
impl<S: AuthStore> OrganizationService<S> {
    /// Returns an organization's configured member/invitation/team graph.
    /// Slug takes precedence over ID, then the session's active organization.
    /// # Errors
    /// Returns session, missing-organization, membership or storage errors.
    pub async fn full(
        &self,
        token: &str,
        transient: bool,
        id: Option<&str>,
        slug: Option<&str>,
        members_limit: Option<i64>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.read_organization(token, transient, id, slug, Graph::Full(members_limit), now)
            .await
    }
    /// Reads organization metadata without loading its member graph.
    /// # Errors
    /// Returns authoritative session, missing-organization or membership errors.
    pub async fn metadata(
        &self,
        token: &str,
        transient: bool,
        id: Option<&str>,
        slug: Option<&str>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.read_organization(token, transient, id, slug, Graph::Metadata, now)
            .await
    }
    async fn read_organization(
        &self,
        token: &str,
        transient: bool,
        id: Option<&str>,
        slug: Option<&str>,
        graph: Graph,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.session(token, transient, now).await?;
        let mut tx = self.store.begin(true).await?;
        let view = load(&mut tx, token, true, now)
            .await?
            .ok_or_else(unauthorized)?;
        let slug = slug.filter(|slug| !slug.is_empty());
        let id = id.filter(|id| !id.is_empty()).or_else(|| {
            view.session["activeOrganizationId"]
                .as_str()
                .filter(|id| !id.is_empty())
        });
        let organization = if let Some(slug) = slug {
            find_slug(&mut tx, slug).await?
        } else if let Some(id) = id {
            tx.get(Model::Organization, id).await?
        } else {
            tx.commit().await?;
            return Ok(Value::Null);
        };
        let organization = organization.ok_or_else(missing)?;
        let id = text(&organization, "id")?.to_owned();
        let body = if let Graph::Full(limit) = graph {
            full_graph(&mut tx, organization, limit).await?
        } else {
            organization
        };
        if !member(&mut tx, text(&view.user, "id")?, &id).await? {
            update_active(&mut tx, &view, None, None, now).await?;
            tx.commit().await?;
            return Err(not_member());
        }
        tx.commit().await?;
        Ok(body)
    }
}
async fn full_graph<T: AuthTransaction>(
    tx: &mut T,
    mut organization: Value,
    limit: Option<i64>,
) -> BlueprintResult<Value> {
    let id = text(&organization, "id")?.to_owned();
    let mut members: Vec<_> = tx
        .list(Model::Member)
        .await?
        .into_iter()
        .filter(|row| row["organizationId"] == id)
        .collect();
    let count = match limit {
        None | Some(0) => 100,
        Some(limit) if limit < 0 => members
            .len()
            .saturating_sub(usize::try_from(limit.unsigned_abs()).unwrap_or(usize::MAX)),
        Some(limit) => usize::try_from(limit).unwrap_or(usize::MAX),
    };
    members.truncate(count);
    let user_ids = members
        .iter()
        .map(|member| text(member, "userId").map(str::to_owned))
        .collect::<BlueprintResult<Vec<_>>>()?;
    let users = tx.get_many(Model::User, &user_ids, 100).await?;
    for member in &mut members {
        let user = users
            .iter()
            .find(|user| user["id"] == member["userId"])
            .ok_or_else(internal)?;
        let mut public = json!({});
        for field in ["id", "name", "email", "image"] {
            if let Some(value) = user.get(field) {
                public[field] = value.clone();
            }
        }
        member["user"] = public;
    }
    organization["members"] = json!(members);
    organization["invitations"] = json!(
        tx.list(Model::Invitation)
            .await?
            .into_iter()
            .filter(|row| row["organizationId"] == id)
            .take(100)
            .collect::<Vec<_>>()
    );
    organization["teams"] = json!(
        tx.list(Model::Team)
            .await?
            .into_iter()
            .filter(|row| row["organizationId"] == id)
            .take(100)
            .collect::<Vec<_>>()
    );
    Ok(organization)
}
