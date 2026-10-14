//! Direct-account observations bound to one immutable ecommerce client.
use crate::StripeClient;
use forklaunch_blueprint_core::{
    BlueprintError, BlueprintResult, provider_http::provider_status_error,
};
use reqwest::Method;
use serde_json::Value;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const LIFETIME: Duration = Duration::from_secs(5);

/// Non-serializable account/mode observation, not permission to move funds or
/// provision ownership. Clones of the same client share its generation.
pub struct VerifiedDirectStripeContext {
    generation: Arc<()>,
    started_at: Instant,
    account_id: String,
    livemode: bool,
}
impl VerifiedDirectStripeContext {
    #[must_use]
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    #[must_use]
    pub const fn livemode(&self) -> bool {
        self.livemode
    }
    /// # Errors
    /// Rejects another client generation and observations older than five seconds.
    pub fn ensure_current(&self, client: &StripeClient) -> BlueprintResult<()> {
        if !Arc::ptr_eq(&self.generation, &client.generation)
            || self.started_at.elapsed() >= LIFETIME
        {
            return Err(mismatch());
        }
        Ok(())
    }
}
pub(crate) fn mismatch() -> BlueprintError {
    BlueprintError::new(
        "ecommerce_provider_context_mismatch",
        "Payment provider context does not match",
    )
}
impl StripeClient {
    /// Authenticated direct-account GETs using the same immutable bearer and URL.
    /// No Connect header, key-prefix inference, provider write or ownership grant.
    /// Trusted expectations must come from server configuration, not request fields.
    /// # Errors
    /// Rejects mismatched/malformed account or mode, denied reads and stale context.
    pub async fn verify_direct_context(
        &self,
        account_id: &str,
        livemode: bool,
    ) -> BlueprintResult<VerifiedDirectStripeContext> {
        if !account_id.starts_with("acct_")
            || account_id.len() <= 5
            || account_id.len() > 128
            || !account_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(mismatch());
        }
        let context = VerifiedDirectStripeContext {
            generation: self.generation.clone(),
            started_at: Instant::now(),
            account_id: account_id.into(),
            livemode,
        };
        let account = self.direct_context_get("account").await?;
        context.ensure_current(self)?;
        if account["object"] != "account" || account["id"] != account_id {
            return Err(mismatch());
        }
        let balance = self.direct_context_get("balance").await?;
        context.ensure_current(self)?;
        if balance["object"] != "balance" || balance["livemode"].as_bool() != Some(livemode) {
            return Err(mismatch());
        }
        Ok(context)
    }
    async fn direct_context_get(&self, resource: &str) -> BlueprintResult<Value> {
        let request = self
            .http
            .request(Method::GET, &["v1", resource])?
            .bearer_auth(self.secret.as_str());
        let response = self.http.execute_json(request).await?;
        if !response.is_success() {
            return Err(provider_status_error(
                "Stripe",
                "verify direct payment context",
                response.status,
            ));
        }
        Ok(response.body)
    }
    /// Reads an intent within a direct-account observation. Returns raw sensitive
    /// provider data only to trusted callers; do not log or persist the body.
    /// # Errors
    /// Rejects stale/foreign context and mismatched intent identity/type/mode.
    pub async fn retrieve_intent_in_context(
        &self,
        context: &VerifiedDirectStripeContext,
        id: &str,
    ) -> BlueprintResult<Value> {
        context.ensure_current(self)?;
        let intent = self.retrieve_intent_details(id, None).await?;
        context.ensure_current(self)?;
        if intent["object"] != "payment_intent"
            || intent["livemode"].as_bool() != Some(context.livemode)
        {
            return Err(mismatch());
        }
        Ok(intent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::get,
    };
    use forklaunch_blueprint_core::provider_http::ProviderHttpClient;
    use serde_json::json;
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Clone)]
    struct Fixture {
        account: Arc<Mutex<Value>>,
        balance: Arc<Mutex<Value>>,
        calls: Arc<AtomicUsize>,
    }
    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn direct_context_exact_account_mode_generation_expiry_and_wire() {
        let state = Fixture {
            account: Arc::new(Mutex::new(json!({"object":"account","id":"acct_owned"}))),
            balance: Arc::new(Mutex::new(json!({"object":"balance","livemode":false}))),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let app = Router::new()
            .route(
                "/v1/account",
                get(|State(s): State<Fixture>, headers: HeaderMap| async move {
                    assert_eq!(headers["authorization"], "Bearer owned-secret");
                    assert!(!headers.contains_key("stripe-account"));
                    s.calls.fetch_add(1, Ordering::SeqCst);
                    let value = s.account.lock().unwrap().clone();
                    (
                        if value.is_null() {
                            StatusCode::UNAUTHORIZED
                        } else {
                            StatusCode::OK
                        },
                        Json(value),
                    )
                }),
            )
            .route(
                "/v1/balance",
                get(|State(s): State<Fixture>, headers: HeaderMap| async move {
                    assert_eq!(headers["authorization"], "Bearer owned-secret");
                    assert!(!headers.contains_key("stripe-account"));
                    s.calls.fetch_add(1, Ordering::SeqCst);
                    Json(s.balance.lock().unwrap().clone())
                }),
            )
            .route(
                "/v1/payment_intents/pi_owned",
                get(|| async {
                    Json(json!({"id":"pi_owned","object":"payment_intent","livemode":false}))
                }),
            )
            .route(
                "/v1/payment_intents/pi_slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    Json(json!({"id":"pi_slow","object":"payment_intent","livemode":false}))
                }),
            )
            .route(
                "/v1/payment_intents/pi_denied",
                get(|| async { (StatusCode::UNAUTHORIZED, "private-provider-body") }),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = StripeClient::with_transport(
            "owned-secret".into(),
            ProviderHttpClient::new(&format!("http://{address}"), Duration::from_secs(1), 1024)
                .unwrap(),
        )
        .unwrap();
        let mut context = client
            .verify_direct_context("acct_owned", false)
            .await
            .unwrap();
        assert!(context.ensure_current(&client.clone()).is_ok());
        assert!(
            client
                .retrieve_intent_in_context(&context, "pi_owned")
                .await
                .is_ok()
        );
        let error = client
            .retrieve_intent_in_context(&context, "pi_denied")
            .await
            .unwrap_err();
        assert!(!format!("{error:?}").contains("private-provider-body"));
        let other =
            StripeClient::with_transport("owned-secret".into(), client.http.clone()).unwrap();
        assert!(context.ensure_current(&other).is_err());
        context.started_at =
            Instant::now().checked_sub(LIFETIME).unwrap() + Duration::from_millis(10);
        assert!(
            client
                .retrieve_intent_in_context(&context, "pi_slow")
                .await
                .is_err()
        );
        context.started_at = Instant::now().checked_sub(LIFETIME).unwrap();
        assert!(
            client
                .retrieve_intent_in_context(&context, "pi_owned")
                .await
                .is_err()
        );
        for account in [
            Value::Null,
            json!({"object":"account","id":"acct_other"}),
            json!({"object":"wrong","id":"acct_owned"}),
            json!({"id":"acct_owned"}),
        ] {
            *state.account.lock().unwrap() = account;
            assert!(
                client
                    .verify_direct_context("acct_owned", false)
                    .await
                    .is_err()
            );
        }
        *state.account.lock().unwrap() = json!({"object":"account","id":"acct_owned"});
        for balance in [
            json!({"object":"balance","livemode":true}),
            json!({"object":"balance","livemode":"false"}),
            json!({"object":"balance"}),
            json!({"object":"wrong","livemode":false}),
        ] {
            *state.balance.lock().unwrap() = balance;
            assert!(
                client
                    .verify_direct_context("acct_owned", false)
                    .await
                    .is_err()
            );
        }
        let count = state.calls.load(Ordering::SeqCst);
        assert!(
            client
                .verify_direct_context("acct_unsafe/path", false)
                .await
                .is_err()
        );
        assert_eq!(state.calls.load(Ordering::SeqCst), count);
        server.abort();
    }
}
