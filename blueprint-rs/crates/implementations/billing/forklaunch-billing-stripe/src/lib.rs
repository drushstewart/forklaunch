#![forbid(unsafe_code)]

use forklaunch_billing_interface::{BillingService, CheckoutSession, Customer, Subscription};
use forklaunch_blueprint_core::{AuditContext, BlueprintResult, Page, PageRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripeBillingConfig {
    pub webhook_secret_env: String,
    pub publishable_key_env: String,
    pub secret_key_env: String,
    pub checkout_success_url: String,
    pub checkout_cancel_url: String,
}

impl StripeBillingConfig {
    #[must_use]
    pub fn new(webhook_secret_env: impl Into<String>) -> Self {
        Self {
            webhook_secret_env: webhook_secret_env.into(),
            publishable_key_env: "STRIPE_PUBLISHABLE_KEY".to_owned(),
            secret_key_env: "STRIPE_SECRET_KEY".to_owned(),
            checkout_success_url: "http://localhost:3000/billing/success".to_owned(),
            checkout_cancel_url: "http://localhost:3000/billing/cancel".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripeWebhookEvent {
    pub id: String,
    pub event_type: String,
    pub object_id: String,
}

impl StripeWebhookEvent {
    #[must_use]
    pub fn customer_subscription_updated(
        id: impl Into<String>,
        subscription_id: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            event_type: "customer.subscription.updated".to_owned(),
            object_id: subscription_id.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StripeBillingAdapter {
    pub config: StripeBillingConfig,
}

impl StripeBillingAdapter {
    #[must_use]
    pub fn new(config: StripeBillingConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn checkout_url(&self, customer_id: &str, plan_id: &str) -> String {
        format!(
            "https://checkout.stripe.com/c/pay/{customer_id}/{plan_id}?success_url={}&cancel_url={}",
            self.config.checkout_success_url, self.config.checkout_cancel_url
        )
    }
}

impl BillingService for StripeBillingAdapter {
    fn get_customer(&self, _id: &str) -> BlueprintResult<Option<Customer>> {
        Ok(None)
    }

    fn list_customers(&self, _request: PageRequest) -> BlueprintResult<Page<Customer>> {
        Ok(Page::new(Vec::new()))
    }

    fn list_subscriptions(&self, _customer_id: &str) -> BlueprintResult<Vec<Subscription>> {
        Ok(Vec::new())
    }

    fn create_checkout_session(
        &self,
        customer_id: &str,
        plan_id: &str,
        context: &AuditContext,
    ) -> BlueprintResult<CheckoutSession> {
        Ok(CheckoutSession {
            id: format!("stripe_checkout_{}", context.correlation_id),
            customer_id: customer_id.to_owned(),
            url: self.checkout_url(customer_id, plan_id),
        })
    }
}

/// Stripe webhook signature verification (the `t=...,v1=<hex>` HMAC-SHA256 scheme).
pub mod webhook {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use subtle::ConstantTimeEq;

    type HmacSha256 = Hmac<Sha256>;

    /// Computes the hex-encoded Stripe signature for a timestamped payload.
    ///
    /// # Panics
    ///
    /// Panics if the HMAC implementation rejects the key; SHA-256 accepts any key size.
    #[must_use]
    pub fn sign(secret: &[u8], timestamp: &str, payload: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key size");
        mac.update(format!("{timestamp}.{payload}").as_bytes());
        to_hex(&mac.finalize().into_bytes())
    }

    /// Verifies a Stripe `Stripe-Signature` header against the raw request body.
    ///
    /// Parses the `t` (timestamp) and one or more `v1` signatures, recomputes the
    /// HMAC over `"{t}.{payload}"`, and compares in constant time.
    #[must_use]
    pub fn verify(secret: &[u8], payload: &str, signature_header: &str) -> bool {
        let mut timestamp = None;
        let mut signatures = Vec::new();
        for part in signature_header.split(',') {
            match part.split_once('=') {
                Some(("t", value)) => timestamp = Some(value.trim()),
                Some(("v1", value)) => signatures.push(value.trim()),
                _ => {}
            }
        }

        let Some(timestamp) = timestamp else {
            return false;
        };
        let expected = sign(secret, timestamp, payload);
        signatures
            .iter()
            .any(|signature| constant_time_eq(signature.as_bytes(), expected.as_bytes()))
    }

    fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
        left.len() == right.len() && bool::from(left.ct_eq(right))
    }

    fn to_hex(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        bytes.iter().fold(String::new(), |mut acc, byte| {
            let _ = write!(acc, "{byte:02x}");
            acc
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stripe_webhook_signatures_verify_and_reject() {
        let secret = b"whsec_test";
        let payload = r#"{"id":"evt_1","type":"customer.subscription.updated"}"#;
        let timestamp = "1700000000";
        let signature = webhook::sign(secret, timestamp, payload);
        let header = format!("t={timestamp},v1={signature}");

        assert!(webhook::verify(secret, payload, &header));
        // tampered payload fails
        assert!(!webhook::verify(secret, "{}", &header));
        // wrong secret fails
        assert!(!webhook::verify(b"whsec_other", payload, &header));
        // missing timestamp fails
        assert!(!webhook::verify(
            secret,
            payload,
            &format!("v1={signature}")
        ));
    }

    #[test]
    fn stripe_adapter_builds_checkout_sessions() {
        let adapter = StripeBillingAdapter::new(StripeBillingConfig::new("STRIPE_WEBHOOK_SECRET"));

        let session = adapter
            .create_checkout_session("customer_1", "pro", &AuditContext::system("corr_1"))
            .expect("checkout should be created");

        assert_eq!(session.id, "stripe_checkout_corr_1");
        assert!(session.url.contains("customer_1/pro"));
    }
}
