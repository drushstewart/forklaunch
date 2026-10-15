//! Trusted awaited verification hooks, without request or credential exposure.
use forklaunch_blueprint_core::BlueprintResult;
use zeroize::Zeroizing;

/// Owned bounded native public-user JSON. No Debug/Serde, credentials or request.
/// Application-retained copies are outside this context's zeroizing ownership.
pub struct EmailVerificationObserverContext {
    user: Zeroizing<String>,
}
impl EmailVerificationObserverContext {
    #[must_use]
    pub fn user_json(&self) -> &str {
        &self.user
    }
}
/// Server-owned before/after verification hook, always awaited outside a database
/// transaction. Blocking code and panics are not intercepted.
pub trait EmailVerificationObserver: Send + Sync {
    /// # Errors
    /// Preparation/future errors become sanitized empty HTTP500 responses.
    fn prepare(
        &self,
        context: EmailVerificationObserverContext,
    ) -> BlueprintResult<crate::background::DeliveryFuture>;
}
#[cfg(feature = "http")]
pub(crate) fn context(
    user: &serde_json::Value,
) -> BlueprintResult<EmailVerificationObserverContext> {
    let public = super::reset_request::projection(user).map_err(|_| failure())?;
    let user = serde_json::to_string(&public).map_err(|_| failure())?;
    Ok(EmailVerificationObserverContext {
        user: Zeroizing::new(user),
    })
}
#[cfg(feature = "http")]
pub(crate) fn failure() -> forklaunch_blueprint_core::BlueprintError {
    forklaunch_blueprint_core::BlueprintError::new(
        "EMAIL_VERIFICATION_OBSERVER_FAILED",
        "Unable to complete email verification callback",
    )
}
