//! Explicit local historical access; no Stripe read, URL refresh or remote validity proof.
use crate::{bound_portal_creation::BoundPortalCreationProfile, services::StripeBillingService};
use forklaunch_billing_base::services::{BillingKind, postgres::PostgresBillingStore};
use forklaunch_billing_interface::services::BillingPortalDto;
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use uuid::Uuid;

/// Original stored DTO returned inside its original, unextended lifetime.
/// The URL may already be invalid remotely. Never a new session or paid-state proof.
pub struct StoredBoundPortalSession {
    pub record: BillingPortalDto,
    /// Final completion-clock TTL check, not the time at which all data is current.
    /// The data snapshot linearizes at the last matching SELECT before audit COMMIT.
    pub observed_at: chrono::DateTime<chrono::Utc>,
}
impl StripeBillingService<PostgresBillingStore> {
    /// No provider GET/POST. Requires current local provenance and same server policy.
    /// # Errors
    /// Rejects managed mode, missing/changed authority, actor/scope mismatch and expiry.
    pub async fn get_stored_bound_portal_session(
        &self,
        profile: &BoundPortalCreationProfile,
        public_id: Uuid,
    ) -> BlueprintResult<StoredBoundPortalSession> {
        let version = self.client().direct_creation_version()?;
        let missing = || {
            BlueprintError::new(
                "billing_provider_binding_conflict",
                "Stored portal evidence does not match",
            )
        };
        let binding = self
            .base()
            .store()
            .stripe_binding_by_public_id(&profile.scope, BillingKind::BillingPortal, public_id)
            .await?
            .ok_or_else(missing)?;
        let (record, observed_at) = self
            .base()
            .store()
            .read_stored_portal_creation(
                &binding,
                version,
                &profile.configuration_id,
                &profile.return_url,
            )
            .await?;
        Ok(StoredBoundPortalSession {
            record: serde_json::from_value(record).map_err(|_| missing())?,
            observed_at,
        })
    }
}
