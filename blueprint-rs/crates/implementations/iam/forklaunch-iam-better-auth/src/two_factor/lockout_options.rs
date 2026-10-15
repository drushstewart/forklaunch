//! Server-owned account failure budget, independent of challenge/OTP attempt limits.
use super::{
    BlueprintResult, DateTime, TwoFactorService, Utc, Value, corrupt, error, json, time, timestamp,
};
use std::time::Duration;

/// Pending sign-in lockout for accounts with a factor record. Authenticated
/// verification and OTP-only users without that record do not use this budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountLockoutOptions {
    enabled: bool,
    threshold: u8,
    duration_ms: i64,
}
impl Default for AccountLockoutOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: 10,
            duration_ms: 900_000,
        }
    }
}
impl AccountLockoutOptions {
    /// # Errors
    /// Rejects thresholds outside 1..=100, sub-millisecond durations or durations
    /// over 365 days. Zero duration expires at the current instant. Negative and
    /// nonfinite JavaScript values are outside this trusted typed subset.
    pub fn new(enabled: bool, threshold: u8, duration: Duration) -> BlueprintResult<Self> {
        if !(1..=100).contains(&threshold)
            || duration.as_millis() > 31_536_000_000
            || !duration.subsec_nanos().is_multiple_of(1_000_000)
        {
            return Err(error(
                "better_auth_invalid_config",
                "Invalid account lockout policy",
            ));
        }
        let duration_ms = i64::try_from(duration.as_millis()).map_err(|_| {
            error(
                "better_auth_invalid_config",
                "Invalid account lockout policy",
            )
        })?;
        Ok(Self {
            enabled,
            threshold,
            duration_ms,
        })
    }
    #[must_use]
    pub fn enabled(self) -> bool {
        self.enabled
    }
    #[must_use]
    pub fn max_failed_attempts(self) -> u8 {
        self.threshold
    }
    #[must_use]
    pub fn duration(self) -> Duration {
        Duration::from_millis(self.duration_ms.unsigned_abs())
    }
    // JS: new Date(Date.now() + durationSeconds * 1000), including TimeClip's
    // truncation toward zero. Both inputs are bounded before float/integer casts.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    fn deadline(self, now: DateTime<Utc>) -> BlueprintResult<DateTime<Utc>> {
        const DATE_LIMIT: f64 = 8_640_000_000_000_000.0;
        let now_ms = now.timestamp_millis();
        if !(-8_640_000_000_000_000..=8_640_000_000_000_000).contains(&now_ms) {
            return Err(corrupt());
        }
        let seconds = self.duration_ms as f64 / 1000.0;
        let deadline = now_ms as f64 + seconds * 1000.0;
        if !deadline.is_finite() || !(-DATE_LIMIT..=DATE_LIMIT).contains(&deadline) {
            return Err(corrupt());
        }
        DateTime::<Utc>::from_timestamp_millis(deadline.trunc() as i64).ok_or_else(corrupt)
    }
}
impl<S> TwoFactorService<S> {
    /// Changes current account policy without rewriting existing lock deadlines.
    /// Disabled policy neither clears nor increments historical lock state.
    #[must_use]
    pub fn with_account_lockout_options(mut self, options: AccountLockoutOptions) -> Self {
        self.account_lockout_options = options;
        self
    }
    pub(super) fn check_account_lock(
        &self,
        row: &mut Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<bool> {
        if !self.account_lockout_options.enabled || row["lockedUntil"].is_null() {
            return Ok(false);
        }
        if time(row, "lockedUntil")? > now {
            return Err(error(
                "ACCOUNT_TEMPORARILY_LOCKED",
                "Account temporarily locked. Please try again later.",
            ));
        }
        self.reset_account_failures(row);
        Ok(true)
    }
    pub(super) fn record_account_failure(
        &self,
        row: &mut Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<bool> {
        if !self.account_lockout_options.enabled {
            return Ok(false);
        }
        let count = row
            .get("failedVerificationCount")
            .filter(|v| !v.is_null())
            .map_or(Ok(0), |v| v.as_u64().ok_or_else(corrupt))?
            .checked_add(1)
            .ok_or_else(corrupt)?;
        let deadline = if count >= u64::from(self.account_lockout_options.threshold) {
            Some(self.account_lockout_options.deadline(now)?)
        } else {
            None
        };
        row["failedVerificationCount"] = json!(count);
        if let Some(deadline) = deadline {
            row["lockedUntil"] = json!(timestamp(deadline));
        }
        Ok(true)
    }
    pub(super) fn reset_account_failures(&self, row: &mut Value) {
        if self.account_lockout_options.enabled {
            row["failedVerificationCount"] = json!(0);
            row["lockedUntil"] = Value::Null;
        }
    }
}
