//! Trusted challenge and device lifetimes, independent of session/OTP expiry.
use super::{BlueprintResult, TwoFactorService, error};
use std::time::Duration;

/// Whole-second server policy, bounded to one year per lifetime. Zero is admitted:
/// a challenge remains valid at its exact expiry instant; trust requires future expiry.
/// Existing records retain their persisted expiry when policy changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TwoFactorLifetimeOptions {
    challenge_seconds: u32,
    trust_seconds: u32,
}
impl Default for TwoFactorLifetimeOptions {
    fn default() -> Self {
        Self {
            challenge_seconds: 600,
            trust_seconds: 2_592_000,
        }
    }
}
impl TwoFactorLifetimeOptions {
    /// # Errors
    /// Rejects fractional seconds or durations over 365 days. JavaScript negative,
    /// fractional and nonfinite option values are outside this typed subset.
    pub fn new(challenge: Duration, trust: Duration) -> BlueprintResult<Self> {
        fn seconds(value: Duration) -> BlueprintResult<u32> {
            if value.subsec_nanos() != 0 || value.as_secs() > 31_536_000 {
                return Err(error(
                    "better_auth_invalid_config",
                    "Invalid two-factor lifetime",
                ));
            }
            u32::try_from(value.as_secs())
                .map_err(|_| error("better_auth_invalid_config", "Invalid two-factor lifetime"))
        }
        Ok(Self {
            challenge_seconds: seconds(challenge)?,
            trust_seconds: seconds(trust)?,
        })
    }
    #[must_use]
    pub fn challenge(self) -> Duration {
        Duration::from_secs(u64::from(self.challenge_seconds))
    }
    #[must_use]
    pub fn trusted_device(self) -> Duration {
        Duration::from_secs(u64::from(self.trust_seconds))
    }
    pub(super) fn challenge_seconds(self) -> i64 {
        i64::from(self.challenge_seconds)
    }
    pub(super) fn trust_seconds(self) -> i64 {
        i64::from(self.trust_seconds)
    }
}
impl<S> TwoFactorService<S> {
    /// Sets lifetimes for newly issued challenges and newly issued/renewed trust.
    #[must_use]
    pub fn with_lifetime_options(mut self, options: TwoFactorLifetimeOptions) -> Self {
        self.lifetime_options = options;
        self
    }
    #[must_use]
    pub fn lifetime_options(&self) -> TwoFactorLifetimeOptions {
        self.lifetime_options
    }
}
