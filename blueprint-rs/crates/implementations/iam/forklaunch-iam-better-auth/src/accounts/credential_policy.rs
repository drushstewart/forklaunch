//! Bounded trusted email/password options. Request bodies never configure policy.
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};

/// Password lengths count UTF-16 code units, matching JavaScript string length.
/// Zero selects the upstream fallback (8/128); effective limits are 1..=4096.
/// Inverted limits are permitted and evaluated minimum first for new passwords.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CredentialPolicy {
    disable_signup: bool,
    minimum: u16,
    maximum: u16,
}
impl Default for CredentialPolicy {
    fn default() -> Self {
        Self {
            disable_signup: false,
            minimum: 8,
            maximum: 128,
        }
    }
}
impl CredentialPolicy {
    /// # Errors
    /// Rejects effective limits above the bounded 4096-code-unit profile.
    pub fn new(disable_signup: bool, minimum: u16, maximum: u16) -> BlueprintResult<Self> {
        let minimum = if minimum == 0 { 8 } else { minimum };
        let maximum = if maximum == 0 { 128 } else { maximum };
        if minimum > 4096 || maximum > 4096 {
            return Err(invalid());
        }
        Ok(Self {
            disable_signup,
            minimum,
            maximum,
        })
    }
    #[must_use]
    pub fn signup_disabled(self) -> bool {
        self.disable_signup
    }
    #[must_use]
    pub fn minimum_password_length(self) -> u16 {
        self.minimum
    }
    #[must_use]
    pub fn maximum_password_length(self) -> u16 {
        self.maximum
    }
    pub(crate) fn check_signup(self) -> BlueprintResult<()> {
        if self.disable_signup {
            return Err(BlueprintError::new(
                "EMAIL_PASSWORD_SIGN_UP_DISABLED",
                "Email and password sign up is not enabled",
            ));
        }
        Ok(())
    }
    pub(crate) fn validate(self, password: &str, minimum: bool) -> BlueprintResult<()> {
        let length = password.encode_utf16().count();
        if minimum && length < usize::from(self.minimum) {
            return Err(BlueprintError::new(
                "PASSWORD_TOO_SHORT",
                "Password too short",
            ));
        }
        if length > usize::from(self.maximum) {
            return Err(BlueprintError::new(
                "PASSWORD_TOO_LONG",
                "Password too long",
            ));
        }
        Ok(())
    }
}
fn invalid() -> BlueprintError {
    BlueprintError::new("better_auth_invalid_config", "Invalid credential policy")
}

/// Whole-second token lifetimes. Reset zero selects 3600 seconds; verification
/// zero is preserved. Fractional/negative JavaScript configuration is not admitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthTokenLifetimes {
    reset: u32,
    verification: u32,
}
impl Default for AuthTokenLifetimes {
    fn default() -> Self {
        Self {
            reset: 3600,
            verification: 3600,
        }
    }
}
impl AuthTokenLifetimes {
    #[must_use]
    pub fn new(reset: u32, verification: u32) -> Self {
        Self {
            reset: if reset == 0 { 3600 } else { reset },
            verification,
        }
    }
    #[must_use]
    pub fn password_reset_seconds(self) -> u32 {
        self.reset
    }
    #[must_use]
    pub fn email_verification_seconds(self) -> u32 {
        self.verification
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        accounts::{CredentialService, SignupInput},
        store::{AuthStore, memory::MemoryAuthTransaction},
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct DenyBegin(AtomicUsize);
    impl AuthStore for DenyBegin {
        type Transaction<'a> = MemoryAuthTransaction;
        async fn begin(&self, _: bool) -> BlueprintResult<Self::Transaction<'_>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(BlueprintError::new(
                "unexpected_store_access",
                "Unexpected store access",
            ))
        }
    }
    #[tokio::test]
    async fn credential_policy_disabled_signup_precedes_store_and_legacy_session_policy() {
        let store = Arc::new(DenyBegin(AtomicUsize::new(0)));
        let service = CredentialService::new(store.clone())
            .with_credential_policy(CredentialPolicy::new(true, 8, 128).unwrap())
            .with_signup_auto_sign_in(false);
        let input = || {
            serde_json::from_value::<SignupInput>(serde_json::json!({"name":"Owned","firstName":"Owned","lastName":"User","email":"invalid","password":"x"})).unwrap()
        };
        for result in [
            service
                .sign_up(input(), "", "", chrono::Utc::now())
                .await
                .map(|_| ()),
            service
                .sign_up_outcome(input(), "", "", chrono::Utc::now())
                .await
                .map(|_| ()),
        ] {
            assert_eq!(result.unwrap_err().code, "EMAIL_PASSWORD_SIGN_UP_DISABLED");
        }
        assert_eq!(store.0.load(Ordering::SeqCst), 0);
    }
}
