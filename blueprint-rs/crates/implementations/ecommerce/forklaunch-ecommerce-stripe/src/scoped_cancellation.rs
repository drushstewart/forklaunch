//! Direct-only operator repair for an explicitly cancelled local order.
use crate::{StripePaymentService, context::mismatch, reconciliation::verify_cancellation};
use forklaunch_blueprint_core::{AuditContext, BlueprintResult};
use forklaunch_ecommerce_base::postgres::{
    PaymentProviderScope, PostgresEcommerceStore, ScopedCancellationOutcome,
};
use serde_json::Value;
use uuid::Uuid;
impl StripePaymentService<PostgresEcommerceStore> {
    /// Confirms terminal canceled/no-charge evidence under fresh direct account/mode
    /// scope, then atomically marks the exact local payment failed and records receipts.
    /// No provider POST, reference clearing or order transition occurs. Non-null/absent
    /// latest charge (including failed/refunded histories) is outside this narrow profile.
    /// Caller must retain `receipt_id` before invocation and inspect it after ambiguous
    /// COMMIT errors/cancellation; success after deadline is explicitly recorded-expired.
    /// # Errors
    /// Rejects Connect, invalid ownership/snapshots, nonterminal/charged/malformed
    /// evidence, stale context, contradictory local capture history and failed audit.
    pub async fn reconcile_bound_cancelled_order_payment(
        &self,
        scope: &PaymentProviderScope,
        payment_id: &str,
        actor: AuditContext,
        receipt_id: Uuid,
    ) -> BlueprintResult<ScopedCancellationOutcome> {
        if self.connect.is_some()
            || scope.provider() != "stripe"
            || receipt_id.is_nil()
            || actor
                .actor_id
                .as_deref()
                .is_none_or(|s| s.trim().is_empty())
            || actor.correlation_id.trim().is_empty()
        {
            return Err(mismatch());
        }
        let context = self
            .client
            .verify_direct_context(scope.merchant_id(), scope.livemode())
            .await?;
        let binding = self
            .base
            .store()
            .payment_provider_binding(scope, payment_id)
            .await?;
        context.ensure_current(&self.client)?;
        let snapshot = self
            .base
            .store()
            .cancelled_order_payment_snapshot(&binding)
            .await?;
        context.ensure_current(&self.client)?;
        let intent = self
            .client
            .retrieve_intent_in_context(&context, binding.provider_ref())
            .await?;
        if intent.get("latest_charge") != Some(&Value::Null)
            || intent["currency"].as_str().is_none_or(|value| {
                value.len() != 3 || !value.bytes().all(|b| b.is_ascii_lowercase())
            })
        {
            return Err(mismatch());
        }
        let evidence = verify_cancellation(snapshot.payment(), &intent)?;
        self.base
            .store()
            .record_scoped_cancelled_order_cancellation(
                snapshot,
                evidence,
                actor,
                receipt_id,
                || context.ensure_current(&self.client),
            )
            .await
    }
}
