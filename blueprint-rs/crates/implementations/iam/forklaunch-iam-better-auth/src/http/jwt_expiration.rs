//! Typed counterpart of a trusted JavaScript Date expiration option.
use super::{AuthHttp, AuthStore};
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
impl<S: AuthStore + 'static> AuthHttp<S> {
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
        self.jwt_expiration_at = Some(expiration.timestamp());
        Ok(self)
    }
}
#[cfg(test)]
mod tests;
