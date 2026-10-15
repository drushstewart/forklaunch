//! Trusted TOTP parameters, matching the exported six/eight-digit SHA-1 profile.
use super::{BlueprintResult, TwoFactorService, error};
use std::time::Duration;

/// Digits exposed by the upstream two-factor plugin (not arbitrary HOTP widths).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TotpDigits {
    #[default]
    Six,
    Eight,
}
impl TotpDigits {
    #[must_use]
    pub fn count(self) -> u32 {
        match self {
            Self::Six => 6,
            Self::Eight => 8,
        }
    }
}
/// Trusted parameters shared by URI enrollment and verification. SHA-1 and the
/// one-step clock-skew window remain fixed. No parameters are read from HTTP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TotpOptions {
    digits: TotpDigits,
    period_ms: u32,
}
impl Default for TotpOptions {
    fn default() -> Self {
        Self {
            digits: TotpDigits::Six,
            period_ms: 30_000,
        }
    }
}
impl TotpOptions {
    /// Maps the duration to upstream numeric seconds (`milliseconds / 1000`).
    /// # Errors
    /// Rejects periods outside 1ms..=24h or sub-millisecond precision. Negative,
    /// zero-fallback and other JavaScript-only option values are unsupported.
    pub fn new(digits: TotpDigits, period: Duration) -> BlueprintResult<Self> {
        if !(1..=86_400_000).contains(&period.as_millis())
            || !period.subsec_nanos().is_multiple_of(1_000_000)
        {
            return Err(error("better_auth_invalid_config", "Invalid TOTP period"));
        }
        let period_ms = u32::try_from(period.as_millis())
            .map_err(|_| error("better_auth_invalid_config", "Invalid TOTP period"))?;
        Ok(Self { digits, period_ms })
    }
    #[must_use]
    pub fn digits(self) -> TotpDigits {
        self.digits
    }
    #[must_use]
    pub fn period(self) -> Duration {
        Duration::from_millis(u64::from(self.period_ms))
    }
    pub(super) fn period_seconds(self) -> f64 {
        f64::from(self.period_ms) / 1000.0
    }
    // Every admitted timestamp is an exactly representable JS Date integer.
    // Bounds precede conversion; no saturating conversion accepts invalid input.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub(super) fn counter(self, timestamp_ms: i64) -> Option<u64> {
        if !(0..=8_640_000_000_000_000).contains(&timestamp_ms) {
            return None;
        }
        let milliseconds = self.period_seconds() * 1000.0;
        let counter = ((timestamp_ms as f64) / milliseconds).floor();
        if !counter.is_finite() || !(0.0..=8_640_000_000_000_000.0).contains(&counter) {
            return None;
        }
        Some(counter as u64)
    }
}
impl<S> TwoFactorService<S> {
    /// Changes current TOTP URI/verification parameters without rewriting secrets.
    /// Changing policy invalidates codes generated with incompatible old parameters.
    #[must_use]
    pub fn with_totp_options(mut self, options: TotpOptions) -> Self {
        self.totp_options = options;
        self
    }
    #[cfg(feature = "http")]
    pub(crate) fn copy_configuration(&self) -> Self {
        Self {
            store: self.store.clone(),
            secret: self.secret.clone(),
            issuer: self.issuer.clone(),
            otp_delivery: self.otp_delivery.clone(),
            otp_options: self.otp_options,
            otp_storage: self.otp_storage,
            totp_options: self.totp_options,
            lifecycle_options: self.lifecycle_options.clone(),
            lifetime_options: self.lifetime_options,
            account_lockout_options: self.account_lockout_options,
            backup_code_options: self.backup_code_options,
            backup_code_generator: self.backup_code_generator.clone(),
        }
    }
}
