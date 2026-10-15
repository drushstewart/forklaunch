//! Actor-owned direct completed-creation enrichment, without mutation authority.
use crate::{bound_creation::BoundPaymentLinkCreationProfile, services::StripeBillingService};
use forklaunch_billing_base::services::{BillingKind, postgres::PostgresBillingStore};
use forklaunch_billing_interface::services::PaymentLinkDto;
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::json;
use uuid::Uuid;
fn mismatch() -> BlueprintError {
    BlueprintError::new(
        "billing_bound_payment_link_read_mismatch",
        "Payment link creation evidence does not match",
    )
}
impl StripeBillingService<PostgresBillingStore> {
    /// Reads actor-owned completed direct creation provenance, never a mutable
    /// cache reference or administrative alias fallback. The original trusted
    /// price allowlist and current exact local receipt/binding must still match.
    /// Only providerFields is refreshed from an authenticated exact id/kind/mode
    /// read; active, line items, amount and payment state imply no local lifecycle
    /// update or entitlement. No POST, cache update, claims or publisher is used.
    /// Evidence JSON is bounded (32 depth/8192 nodes/256 KiB worst-case encoding).
    /// Data linearizes at the last matching SELECT, with freshness checked after
    /// audit commit; later local/provider changes remain possible.
    /// # Errors
    /// Denies managed transport/provenance, foreign actor/scope, changed policy,
    /// journal/body/binding mismatch, oversized evidence and stale provider context.
    pub async fn get_bound_created_payment_link(
        &self,
        profile: &BoundPaymentLinkCreationProfile,
        public_id: Uuid,
    ) -> BlueprintResult<PaymentLinkDto> {
        let version = self.client().direct_creation_version()?;
        let store = self.base().store();
        let binding = store
            .stripe_binding_by_public_id(&profile.scope, BillingKind::PaymentLink, public_id)
            .await?
            .ok_or_else(mismatch)?;
        let snapshot = store
            .payment_link_creation_read_snapshot(&binding, version)
            .await?;
        let payload = snapshot.payload();
        let object = payload.as_object().ok_or_else(mismatch)?;
        if object.len() != 3
            || !["request", "providerFields", "trustedPrices"]
                .iter()
                .all(|key| object.contains_key(*key))
            || payload["trustedPrices"] != json!(profile.prices)
        {
            return Err(mismatch());
        }
        let context = self
            .client()
            .verify_context(profile.scope.account_id(), profile.scope.livemode())
            .await?;
        let provider = self
            .client()
            .retrieve_payment_link_in_context(&context, binding.provider_id())
            .await?;
        let record = store.read_payment_link_creation_snapshot(&snapshot).await?;
        context.ensure_current(self.client())?;
        let mut record: PaymentLinkDto = serde_json::from_value(record).map_err(|_| mismatch())?;
        record.provider_fields = Some(provider);
        Ok(record)
    }
}
