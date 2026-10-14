//! Typed counterpart of a trusted JavaScript Date expiration option.
use super::{AuthHttp, AuthStore};
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
pub(super) enum JwtExpiration {
    Absolute(i64),
    Lifetime(u64),
}
impl JwtExpiration {
    pub(super) fn option(&self) -> serde_json::Value {
        match self {
            Self::Absolute(seconds) => serde_json::json!(seconds),
            Self::Lifetime(seconds) => serde_json::json!(format!("{seconds}s")),
        }
    }
}
impl<S: AuthStore + 'static> AuthHttp<S> {
    /// Sets a trusted relative JWT lifetime as whole nonnegative seconds.
    /// Zero issues immediately expired tokens. Payload `exp` still takes precedence.
    /// This and the absolute-date setter replace one another; the last call wins.
    /// Cookie and persisted session expiry are unchanged.
    /// # Errors
    /// Rejects fractional seconds and seconds outside JavaScript's safe integer range.
    pub fn with_jwt_lifetime(mut self, lifetime: std::time::Duration) -> BlueprintResult<Self> {
        if lifetime.subsec_nanos() != 0 || lifetime.as_secs() > 9_007_199_254_740_991 {
            return Err(BlueprintError::new(
                "better_auth_invalid_config",
                "JWT lifetime must contain whole JavaScript-safe seconds",
            ));
        }
        self.jwt_expiration = Some(JwtExpiration::Lifetime(lifetime.as_secs()));
        Ok(self)
    }
    /// Sets an absolute JWT expiry, matching `jwt.expirationTime: Date`.
    /// Milliseconds are floored to seconds, including before the Unix epoch.
    /// Explicit payload `exp` takes precedence; cookie/session expiry is unchanged.
    /// Past dates are allowed and produce expired tokens, as upstream does.
    /// # Errors
    /// Rejects sub-millisecond instants and dates outside the JavaScript Date range.
    pub fn with_jwt_expiration_at(mut self, expiration: DateTime<Utc>) -> BlueprintResult<Self> {
        if !expiration
            .timestamp_subsec_nanos()
            .is_multiple_of(1_000_000)
            || expiration.timestamp_millis().unsigned_abs() > 8_640_000_000_000_000
        {
            return Err(BlueprintError::new(
                "better_auth_invalid_config",
                "JWT expiration must be a representable JavaScript Date",
            ));
        }
        self.jwt_expiration = Some(JwtExpiration::Absolute(expiration.timestamp()));
        Ok(self)
    }
}
#[cfg(test)]
mod tests;
