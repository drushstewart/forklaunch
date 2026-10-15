//! Trusted persistent-session policy; independent of JWT and MFA proof lifetimes.
use chrono::{DateTime, Duration, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};

/// Whole-second session options. Default preserves the `ForkLaunch` preset (one
/// day expiry/freshness, one hour refresh), not upstream's seven-day expiry.
/// Zero freshness disables age checks; zero update age refreshes on every read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionPolicy {
    pub(super) expires_in: u32,
    pub(super) update_age: u32,
    pub(super) fresh_age: u32,
    pub(super) disable_refresh: bool,
}
impl Default for SessionPolicy {
    fn default() -> Self {
        Self {
            expires_in: 86400,
            update_age: 3600,
            fresh_age: 86400,
            disable_refresh: false,
        }
    }
}
impl SessionPolicy {
    /// Validates positive expiry and nonnegative whole-second ages. Update age
    /// may exceed expiry. Fractional/negative JS options are outside this profile;
    /// upstream zero expiry's fallback is represented by an explicit positive TTL.
    /// # Errors
    /// Rejects zero expiry before any storage or cookie side effects.
    pub fn new(
        expires_in: u32,
        update_age: u32,
        fresh_age: u32,
        disable_refresh: bool,
    ) -> BlueprintResult<Self> {
        if expires_in == 0 {
            return Err(BlueprintError::new(
                "better_auth_invalid_config",
                "Session expiry must be positive",
            ));
        }
        Ok(Self {
            expires_in,
            update_age,
            fresh_age,
            disable_refresh,
        })
    }
    #[must_use]
    pub const fn expires_in(self) -> u32 {
        self.expires_in
    }
    #[must_use]
    pub const fn update_age(self) -> u32 {
        self.update_age
    }
    #[must_use]
    pub const fn fresh_age(self) -> u32 {
        self.fresh_age
    }
    #[must_use]
    pub const fn disable_refresh(self) -> bool {
        self.disable_refresh
    }
    pub(crate) fn fresh(self, created: DateTime<Utc>, now: DateTime<Utc>) -> bool {
        self.fresh_age == 0
            || now.signed_duration_since(created) < Duration::seconds(i64::from(self.fresh_age))
    }
}
