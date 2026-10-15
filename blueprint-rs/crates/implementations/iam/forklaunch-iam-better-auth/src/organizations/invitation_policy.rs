//! Trusted scalar invitation options, independent of organization admission.
use super::{BlueprintError, BlueprintResult, DateTime, Utc};

/// Bounded scalar invitation policy, configured by the trusted application.
///
/// Defaults preserve the configured native profile: 48 hours, 100 pending
/// invitations and verified recipients for get/accept/reject by ID. Listing
/// invitations for the session email always requires verification independently.
/// Whole seconds and counts are unsigned 32-bit integers. Zero expiration uses
/// 48 hours; zero count denies new invitations but does not deny resends.
/// Pending counts use the first 100 matching records before expiry filtering.
/// Custom ID generators, policy callbacks, cancellation-on-reinvite and delivery
/// hooks are separate capabilities; request JSON cannot set this policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvitationPolicy {
    expires_in_seconds: u32,
    pub(super) limit: u32,
    pub(super) require_verified_email: bool,
}
impl Default for InvitationPolicy {
    fn default() -> Self {
        Self::new(172_800, 100, true)
    }
}
impl InvitationPolicy {
    /// Sets trusted whole-second expiration, pending count and recipient policy.
    #[must_use]
    pub const fn new(expires_in_seconds: u32, limit: u32, require_verified_email: bool) -> Self {
        Self {
            expires_in_seconds: if expires_in_seconds == 0 {
                172_800
            } else {
                expires_in_seconds
            },
            limit,
            require_verified_email,
        }
    }
    pub(super) fn expiry(self, now: DateTime<Utc>) -> BlueprintResult<DateTime<Utc>> {
        now.checked_add_signed(chrono::Duration::seconds(i64::from(
            self.expires_in_seconds,
        )))
        .ok_or_else(|| {
            BlueprintError::new("better_auth_storage_error", "Invalid invitation expiration")
        })
    }
}
