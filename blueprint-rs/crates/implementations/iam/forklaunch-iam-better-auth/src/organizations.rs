//! Configured organization lifecycle over tenant-bound authoritative records.
use crate::{
    sessions::{SessionService, SessionView, load},
    store::{AuthStore, AuthTransaction, Model},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
#[cfg(feature = "postgres")]
pub(crate) use members::query_number;
use serde_json::{Value, json};
use std::sync::Arc;

mod create;
mod full;
mod invitations;
mod members;
mod mutation;
mod permission;
mod roles;
mod selection;
mod teams;
pub use members::{AddMemberInput, MemberRoles};
#[cfg(test)]
pub(crate) mod tests;

pub struct OrganizationService<S> {
    time_source: crate::time_source::TimeSource,
    session_policy: crate::sessions::SessionPolicy,
    store: Arc<S>,
}
/// The selection endpoint emits a session cookie only after an actual selection
/// or clearing an existing selection; failed membership checks do not emit it.
pub struct OrganizationSelection {
    pub body: Value,
    pub cookie: bool,
}
impl<S: AuthStore> OrganizationService<S> {
    #[cfg(feature = "http")]
    pub(crate) fn with_time_source(mut self, source: crate::time_source::TimeSource) -> Self {
        self.time_source = source;
        self
    }

    /// Installs trusted session lifetime, refresh and freshness options.
    #[must_use]
    pub fn with_session_policy(mut self, policy: crate::sessions::SessionPolicy) -> Self {
        self.session_policy = policy;
        self
    }

    #[must_use]
    pub fn new(store: Arc<S>) -> Self {
        Self {
            time_source: crate::time_source::TimeSource::default(),
            store,
            session_policy: crate::sessions::SessionPolicy::default(),
        }
    }

    /// Lists organizations in membership insertion order.
    /// # Errors
    /// Returns authoritative session or sanitized storage errors.
    pub async fn list(
        &self,
        token: &str,
        transient: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let view = self.session(token, transient, now).await?;
        let mut tx = self.store.begin(false).await?;
        let mut organizations = Vec::new();
        for member in
            crate::store::find_all(&mut tx, Model::Member, "userId", text(&view.user, "id")?)
                .await?
        {
            if member["userId"] == view.user["id"] {
                organizations.push(
                    tx.get(Model::Organization, text(&member, "organizationId")?)
                        .await?
                        .unwrap_or(Value::Null),
                );
            }
        }
        tx.commit().await?;
        Ok(json!(organizations))
    }

    /// Authenticated availability check; slugs compare exactly, including case.
    /// # Errors
    /// Returns unauthorized, taken-slug or sanitized storage failures.
    pub async fn check_slug(
        &self,
        token: &str,
        transient: bool,
        slug: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.session(token, transient, now).await?;
        let mut tx = self.store.begin(false).await?;
        let exists = find_slug(&mut tx, slug).await?.is_some();
        tx.commit().await?;
        if exists {
            return Err(BlueprintError::new(
                "ORGANIZATION_SLUG_ALREADY_TAKEN",
                "Organization slug already taken",
            ));
        }
        Ok(json!({"status":true}))
    }

    async fn session(
        &self,
        token: &str,
        transient: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<SessionView> {
        SessionService::new(self.store.clone())
            .with_time_source(self.time_source.clone())
            .with_session_policy(self.session_policy)
            .get(token, transient, false, now)
            .await?
            .ok_or_else(unauthorized)
    }
}
async fn find_slug<T: AuthTransaction>(tx: &mut T, slug: &str) -> BlueprintResult<Option<Value>> {
    tx.find_unique(Model::Organization, "slug", slug).await
}
async fn member<T: AuthTransaction>(
    tx: &mut T,
    user: &str,
    organization: &str,
) -> BlueprintResult<bool> {
    Ok(crate::store::find_all(tx, Model::Member, "userId", user)
        .await?
        .iter()
        .any(|row| row["userId"] == user && row["organizationId"] == organization))
}
async fn update_active<T: AuthTransaction>(
    tx: &mut T,
    view: &SessionView,
    organization: Option<&str>,
    team: Option<&str>,
    now: DateTime<Utc>,
) -> BlueprintResult<()> {
    let mut session = tx
        .get(Model::Session, text(&view.session, "id")?)
        .await?
        .ok_or_else(unauthorized)?;
    session["activeOrganizationId"] = json!(organization);
    if let Some(team) = team {
        session["activeTeamId"] = json!(team);
    }
    session["updatedAt"] = json!(timestamp(now));
    tx.replace(Model::Session, session).await
}
fn text<'a>(row: &'a Value, field: &str) -> BlueprintResult<&'a str> {
    row[field].as_str().ok_or_else(internal)
}
fn timestamp(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn unauthorized() -> BlueprintError {
    BlueprintError::new("UNAUTHORIZED", "Unauthorized")
}
fn internal() -> BlueprintError {
    BlueprintError::new(
        "better_auth_storage_error",
        "Unable to access authentication storage",
    )
}
fn missing() -> BlueprintError {
    BlueprintError::new("ORGANIZATION_NOT_FOUND", "Organization not found")
}
fn not_member() -> BlueprintError {
    BlueprintError::new(
        "USER_IS_NOT_A_MEMBER_OF_THE_ORGANIZATION",
        "User is not a member of the organization",
    )
}
