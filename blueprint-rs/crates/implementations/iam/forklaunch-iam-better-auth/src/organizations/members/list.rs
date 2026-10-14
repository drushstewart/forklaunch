//! Scoped membership listing with configured adapter filtering and pagination.
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, find_member, find_slug, joined, json, missing, no_active,
    nonmember, text,
};
#[cfg(feature = "postgres")]
pub(crate) use values::number as query_number;
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
        let window = if let Some((offset, limit)) = indexed_window(query) {
            Some(
                tx.find_window(Model::Member, "organizationId", &id, offset, limit)
                    .await?,
            )
        } else {
            tx.query_members(&id, query).await?
        };
        if let Some(window) = window {
            let ids = window
                .records
                .iter()
                .map(|member| text(member, "userId").map(str::to_owned))
                .collect::<BlueprintResult<Vec<_>>>()?;
            let users = tx.get_many(Model::User, &ids, ids.len()).await?;
            let users = users
                .iter()
                .map(|user| text(user, "id").map(|id| (id, user)))
                .collect::<BlueprintResult<std::collections::BTreeMap<_, _>>>()?;
            let mut members = Vec::with_capacity(window.records.len());
            for member in window.records {
                let user = users
                    .get(text(&member, "userId")?)
                    .ok_or_else(adapter_error)?;
                members.push(super::joined_user(member, user));
            }
            tx.commit().await?;
            return Ok(json!({"members":members,"total":window.total}));
        }
        let filter = filter::Filter::new(query)?;
        let mut members = Vec::new();
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

// Preserve the established JS coercion/slicing path for negative, fractional,
// nonfinite, oversized or configured filtering/sorting inputs.
fn indexed_window(query: &Value) -> Option<(usize, usize)> {
    if ["filterField", "sortBy"].iter().any(|field| {
        query[*field]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    }) {
        return None;
    }
    let offset = query
        .get("offset")
        .map_or(0.0, |value| values::number(Some(value)));
    let limit = query
        .get("limit")
        .map_or(100.0, |value| values::number(Some(value)));
    let limit = if limit == 0.0 { 100.0 } else { limit };
    if !offset.is_finite()
        || offset < 0.0
        || offset.fract() != 0.0
        || !limit.is_finite()
        || !(1.0..=4096.0).contains(&limit)
        || limit.fract() != 0.0
    {
        return None;
    }
    let offset: usize = offset.to_string().parse().ok()?;
    i64::try_from(offset).ok()?;
    Some((offset, limit.to_string().parse().ok()?))
}
