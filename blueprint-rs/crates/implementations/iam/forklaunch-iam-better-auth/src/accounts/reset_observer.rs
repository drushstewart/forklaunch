//! Trusted post-password-commit callback; never dispatched in the background.
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use zeroize::Zeroizing;

/// Owned bounded public-user snapshot, deliberately without Debug or Serde.
/// No password, reset token, session or raw HTTP request is included.
/// Copies parsed or retained by the callback remain the callback owner's data.
pub struct PasswordResetObserverContext {
    user: Zeroizing<String>,
}
impl PasswordResetObserverContext {
    /// JSON object containing the configured native public-user projection.
    #[must_use]
    pub fn user_json(&self) -> &str {
        &self.user
    }
}
/// Trusted equivalent of the supported `onPasswordReset` callback subset.
/// Preparation and the owned future run after password commit and before optional
/// session revocation. No database transaction is held while either executes.
/// The future is always awaited; cancellation may leave revocation unperformed.
/// Blocking code and panics are not intercepted.
pub trait PasswordResetObserver: Send + Sync {
    /// # Errors
    /// Either preparation or future failure becomes a sanitized empty HTTP 500.
    fn prepare(
        &self,
        context: PasswordResetObserverContext,
    ) -> BlueprintResult<crate::background::DeliveryFuture>;
}
pub(super) fn context(user: &Value) -> BlueprintResult<PasswordResetObserverContext> {
    let public = super::reset_request::projection(user).map_err(|_| failure())?;
    let user = serde_json::to_string(&public).map_err(|_| failure())?;
    Ok(PasswordResetObserverContext {
        user: Zeroizing::new(user),
    })
}
pub(super) fn failure() -> BlueprintError {
    BlueprintError::new(
        "PASSWORD_RESET_OBSERVER_FAILED",
        "Unable to complete password reset callback",
    )
}

#[cfg(all(test, feature = "http"))]
#[path = "reset_observer_tests.rs"]
pub(crate) mod tests;
