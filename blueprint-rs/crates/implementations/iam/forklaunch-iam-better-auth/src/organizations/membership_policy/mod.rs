//! Awaited membership quota; JavaScript numeric comparison over a captured count.
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, Model, OrganizationService, Value,
};
use std::{future::Future, pin::Pin};
mod accept;
mod add;
/// Owned configured public user and organization. No credentials, raw request,
/// Debug or Serde. Selected strings are bounded to 4096 UTF-8 bytes before cloning.
pub struct MembershipLimitContext {
    user: Value,
    organization: Value,
}
impl MembershipLimitContext {
    #[must_use]
    pub fn user(&self) -> &Value {
        &self.user
    }
    #[must_use]
    pub fn organization(&self) -> &Value {
        &self.organization
    }
}
/// Full IEEE numeric callback result: fractions, signed zero, NaN and infinities
/// retain JavaScript `count >= result` semantics. No JSON number conversion occurs.
pub type MembershipLimitFuture =
    Pin<Box<dyn Future<Output = BlueprintResult<f64>> + Send + 'static>>;
/// Trusted awaited quota. Preparation/future failures are sanitized errors;
/// blocking work, JavaScript receiver mutation and Rust panics are not modeled.
pub trait MembershipLimitPolicy: Send + Sync {
    /// # Errors
    /// Failure is fatal; an HTTP consumer returns an empty 500 response.
    fn prepare(&self, context: MembershipLimitContext) -> BlueprintResult<MembershipLimitFuture>;
}
fn failure() -> BlueprintError {
    BlueprintError::new("HTTP_INTERNAL_EMPTY", "Membership limit policy failed")
}
fn limited() -> BlueprintError {
    BlueprintError::new(
        "ORGANIZATION_MEMBERSHIP_LIMIT_REACHED",
        "Organization membership limit reached",
    )
}
async fn invoke(
    hook: &dyn MembershipLimitPolicy,
    user: &Value,
    organization: &Value,
    count: usize,
) -> BlueprintResult<()> {
    let user = crate::accounts::reset_request::projection(user).map_err(|_| failure())?;
    let organization =
        super::delivery::organization_projection(organization).map_err(|_| failure())?;
    let limit = hook
        .prepare(MembershipLimitContext { user, organization })
        .map_err(|_| failure())?
        .await
        .map_err(|_| failure())?;
    // Counts come from the existing bounded record scan, and fit exactly in u32/f64.
    let count = f64::from(u32::try_from(count).map_err(|_| super::internal())?);
    if count >= limit {
        return Err(limited());
    }
    Ok(())
}
async fn count<T: AuthTransaction>(tx: &mut T, id: &str) -> BlueprintResult<usize> {
    Ok(
        crate::store::find_all(tx, Model::Member, "organizationId", id)
            .await?
            .iter()
            .filter(|row| row["organizationId"] == id)
            .count(),
    )
}
impl<S: AuthStore> OrganizationService<S> {
    pub(super) fn hydration_member_limit(&self) -> usize {
        if self.membership_limit.is_some() {
            100
        } else {
            self.policy.member_limit()
        }
    }
}
