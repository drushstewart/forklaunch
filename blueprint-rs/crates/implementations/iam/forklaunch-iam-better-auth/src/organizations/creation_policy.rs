//! Trusted awaited creation policies; callbacks never hold an authentication transaction.
use super::{BlueprintError, BlueprintResult, OrganizationService, Value};
use crate::{
    sessions::SessionView,
    store::{AuthStore, AuthTransaction, Model},
};
use std::{future::Future, pin::Pin};

/// Owned configured public-user projection. No credentials, raw request, Debug or Serde.
/// Selected scalar strings are limited to 4096 UTF-8 bytes before cloning.
pub struct OrganizationCreationContext {
    user: Value,
}
impl OrganizationCreationContext {
    #[must_use]
    pub fn user(&self) -> &Value {
        &self.user
    }
}
/// Owned lazy policy work. Always awaited; the delivery scheduler does not apply.
pub type OrganizationCreationFuture =
    Pin<Box<dyn Future<Output = BlueprintResult<bool>> + Send + 'static>>;
/// Trusted boolean policy. For allow-create, true permits; for organization-limit,
/// true means the limit has been reached. Preparation and future failures are fatal.
/// Blocking work and panics are outside this explicit Result contract.
pub trait OrganizationCreationPolicy: Send + Sync {
    /// # Errors
    /// Both preparation and future errors become a sanitized empty HTTP 500.
    fn prepare(
        &self,
        context: OrganizationCreationContext,
    ) -> BlueprintResult<OrganizationCreationFuture>;
}
fn failure() -> BlueprintError {
    BlueprintError::new("HTTP_INTERNAL_EMPTY", "Organization creation policy failed")
}
async fn invoke(hook: &dyn OrganizationCreationPolicy, user: &Value) -> BlueprintResult<bool> {
    let user = crate::accounts::reset_request::projection(user).map_err(|_| failure())?;
    hook.prepare(OrganizationCreationContext { user })
        .map_err(|_| failure())?
        .await
        .map_err(|_| failure())
}
pub(super) fn denied() -> BlueprintError {
    BlueprintError::new(
        "YOU_ARE_NOT_ALLOWED_TO_CREATE_A_NEW_ORGANIZATION",
        "You are not allowed to create a new organization",
    )
}
pub(super) fn limited() -> BlueprintError {
    BlueprintError::new(
        "YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_ORGANIZATIONS",
        "You have reached the maximum number of organizations",
    )
}
impl<S: AuthStore> OrganizationService<S> {
    pub(super) async fn creation_scalar<T: AuthTransaction>(
        &self,
        tx: &mut T,
        view: &SessionView,
    ) -> BlueprintResult<()> {
        if !self.policy.allow_create {
            return Err(denied());
        }
        if let Some(limit) = self.policy.organization_limit {
            let user = super::text(&view.user, "id")?;
            let count = tx
                .find_page(Model::Member, "userId", user, None, 100)
                .await?
                .records
                .into_iter()
                .filter(|row| row["userId"] == user)
                .count();
            if u32::try_from(count).map_err(|_| super::internal())? >= limit {
                return Err(limited());
            }
        }
        Ok(())
    }
    pub(super) fn has_creation_callbacks(&self) -> bool {
        self.allow_creation.is_some() || self.creation_limit.is_some()
    }
    pub(super) async fn creation_callbacks(&self, view: &SessionView) -> BlueprintResult<()> {
        let allowed = if let Some(hook) = &self.allow_creation {
            invoke(hook.as_ref(), &view.user).await?
        } else {
            self.policy.allow_create
        };
        if !allowed {
            return Err(denied());
        }
        // Upstream performs its default adapter list/join even for a boolean hook.
        // Commit the read before calling application work, allowing store reentry.
        let user = super::text(&view.user, "id")?;
        let mut tx = self.store.begin(false).await?;
        let page = tx
            .find_page(Model::Member, "userId", user, None, 100)
            .await?;
        let mut count = 0u32;
        for row in page.records {
            if row["userId"] != user {
                continue;
            }
            count += 1;
            if let Some(id) = row["organizationId"].as_str() {
                let _ = tx.get(Model::Organization, id).await?;
            }
        }
        tx.commit().await?;
        let reached = if let Some(hook) = &self.creation_limit {
            invoke(hook.as_ref(), &view.user).await?
        } else {
            self.policy
                .organization_limit
                .is_some_and(|limit| count >= limit)
        };
        if reached {
            return Err(limited());
        }
        Ok(())
    }
}

// The upstream adapter updates by the captured token, without re-admitting its
// current user or ID. Missing rows are a no-op; never recreate a removed session.
pub(super) async fn update_captured_token<T: AuthTransaction>(
    tx: &mut T,
    token: &str,
    organization: &str,
    team: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> BlueprintResult<()> {
    if let Some(mut session) = tx.find_unique(Model::Session, "token", token).await? {
        session["activeOrganizationId"] = serde_json::json!(organization);
        if let Some(team) = team {
            session["activeTeamId"] = serde_json::json!(team);
        }
        session["updatedAt"] = serde_json::json!(super::timestamp(now));
        tx.replace(Model::Session, session).await?;
    }
    Ok(())
}
