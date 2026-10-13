//! Scoped membership listing with configured adapter filtering and pagination.
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, find_member, find_slug, joined, json, missing, no_active,
    nonmember, text,
};
mod filter;
mod sorting;
mod values;

impl<S: AuthStore> OrganizationService<S> {
    /// Lists scoped members with public users, filtered total and stable sorting.
    /// # Errors
    /// Returns session/scope/membership errors or invalid adapter field failures.
    pub async fn list_members(
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
            .filter(|value| !value.is_empty())
        {
            text(&find_slug(&mut tx, slug).await?.ok_or_else(missing)?, "id")?.to_owned()
        } else {
            query["organizationId"]
                .as_str()
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    view.session["activeOrganizationId"]
                        .as_str()
                        .filter(|value| !value.is_empty())
                })
                .ok_or_else(no_active)?
                .to_owned()
        };
        let actor = find_member(&mut tx, text(&view.user, "id")?, &id)
            .await?
            .ok_or_else(nonmember)?;
        if joined(&mut tx, actor).await?.is_none() {
            return Err(nonmember());
        }
        let mut members = Vec::new();
        let filter = filter::Filter::new(query)?;
        for member in crate::store::find_all(&mut tx, Model::Member, "organizationId", &id).await? {
            if member["organizationId"] == id && filter.matches(&member)? {
                members.push(member);
            }
        }
        let total = members.len();
        sorting::sort(&mut members, query)?;
        let offset = values::number(query.get("offset"));
        let offset = values::index(offset, members.len());
        let limit = values::number(query.get("limit"));
        let limit = if limit.is_nan() || limit == 0.0 {
            100.0
        } else {
            limit
        };
        let limit = values::index(limit, members.len() - offset);
        let mut result = Vec::new();
        for member in members.into_iter().skip(offset).take(limit) {
            result.push(joined(&mut tx, member).await?.ok_or_else(adapter_error)?);
        }
        tx.commit().await?;
        Ok(json!({"members":result,"total":total}))
    }
}
fn field(value: &str) -> BlueprintResult<()> {
    if matches!(
        value,
        "id" | "organizationId" | "userId" | "role" | "createdAt"
    ) {
        Ok(())
    } else {
        Err(adapter_error())
    }
}
fn adapter_error() -> BlueprintError {
    BlueprintError::new(
        "HTTP_INTERNAL_EMPTY",
        "Unable to query organization members",
    )
}
