//! Direct-account verification. Managed gateway context needs a separate,
//! authenticated account/mode/binding-generation contract before integration.
use super::{BillingTransport, StripeBillingClient};
use forklaunch_blueprint_core::{
    BlueprintError, BlueprintResult, provider_http::provider_status_error,
};
use reqwest::Method;
use serde_json::Value;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const CONTEXT_LIFETIME: Duration = Duration::from_secs(5);

/// Sealed, non-serializable observation tied to one immutable client generation.
/// Cloning that client preserves identity; constructing/reconfiguring a client
/// requires fresh verification. This is not a financial-operation capability or
/// authorization to provision local ownership. Future integrated operations must
/// verify again for each operation and check freshness immediately before use.
pub struct VerifiedStripeContext {
    generation: Arc<()>,
    started_at: Instant,
    account_id: String,
    livemode: bool,
}
impl VerifiedStripeContext {
    #[must_use]
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    #[must_use]
    pub const fn livemode(&self) -> bool {
        self.livemode
    }
    /// Checks the exact immutable client generation and a five-second window
    /// starting before provider verification. Does not refresh cached evidence.
    /// # Errors
    /// Rejects foreign/reconfigured clients and stale observations.
    pub fn ensure_current(&self, client: &StripeBillingClient) -> BlueprintResult<()> {
        if !Arc::ptr_eq(&self.generation, &client.config_generation)
            || self.started_at.elapsed() >= CONTEXT_LIFETIME
        {
            return Err(mismatch());
        }
        Ok(())
    }
}
impl StripeBillingClient {
    /// Authenticates the current direct account using GET /v1/account and its
    /// mode using GET /v1/balance, under the same immutable bearer/version/URL.
    /// Expectations must come from trusted server configuration, not request DTOs.
    /// Restricted keys without these read permissions fail closed. No key-prefix
    /// inference, cache, provider writes, Connect header or local binding mutation.
    /// # Errors
    /// Rejects account/mode mismatch, malformed responses, expired verification,
    /// provider errors, and managed mode (before issuing any gateway request).
    pub async fn verify_context(
        &self,
        expected_account: &str,
        expected_livemode: bool,
    ) -> BlueprintResult<VerifiedStripeContext> {
        if matches!(self.transport, BillingTransport::Managed(_)) {
            return Err(BlueprintError::new(
                "billing_managed_context_unavailable",
                "Managed billing context verification requires a scoped gateway contract",
            ));
        }
        if !expected_account.starts_with("acct_")
            || expected_account.len() <= 5
            || expected_account.len() > 128
            || !expected_account
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(mismatch());
        }
        let started_at = Instant::now();
        let account = self.context_get("account").await?;
        if account["object"] != "account" || account["id"] != expected_account {
            return Err(mismatch());
        }
        let balance = self.context_get("balance").await?;
        if balance["object"] != "balance"
            || balance["livemode"].as_bool() != Some(expected_livemode)
        {
            return Err(mismatch());
        }
        let context = VerifiedStripeContext {
            generation: self.config_generation.clone(),
            started_at,
            account_id: expected_account.into(),
            livemode: expected_livemode,
        };
        context.ensure_current(self)?;
        Ok(context)
    }
    async fn context_get(&self, resource: &str) -> BlueprintResult<Value> {
        let BillingTransport::Direct { http, secret } = &self.transport else {
            return Err(mismatch());
        };
        let request = http
            .request(Method::GET, &["v1", resource])?
            .bearer_auth(secret.as_str())
            .header("stripe-version", self.api_version.as_str());
        let response = http.execute_json(request).await?;
        if !(200..300).contains(&response.status) {
            return Err(provider_status_error("stripe", "billing", response.status));
        }
        if !response.body.is_object() {
            return Err(mismatch());
        }
        Ok(response.body)
    }
}
fn mismatch() -> BlueprintError {
    BlueprintError::new(
        "billing_provider_context_mismatch",
        "Stripe account context is invalid, changed or expired",
    )
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
        status: Arc<Mutex<StatusCode>>,
    }
    fn headers(headers: &HeaderMap) {
        assert_eq!(
            headers["authorization"],
            "Bearer opaque-credential-with-no-mode-prefix"
        );
        assert_eq!(headers["stripe-version"], super::super::STRIPE_API_VERSION);
        assert!(!headers.contains_key("stripe-account"));
    }
    async fn fixture() -> (
        StripeBillingClient,
        Fixture,
        tokio::task::JoinHandle<()>,
        String,
    ) {
        let state = Fixture {
            account: Arc::new(Mutex::new(json!({"id":"acct_owned","object":"account"}))),
            balance: Arc::new(Mutex::new(json!({"object":"balance","livemode":false}))),
            calls: Arc::new(AtomicUsize::new(0)),
            status: Arc::new(Mutex::new(StatusCode::OK)),
        };
        let app = Router::new()
            .route(
                "/v1/account",
                get(|State(state): State<Fixture>, h: HeaderMap| async move {
                    headers(&h);
                    state.calls.fetch_add(1, Ordering::SeqCst);
                    (
                        *state.status.lock().unwrap(),
                        Json(state.account.lock().unwrap().clone()),
                    )
                }),
            )
            .route(
                "/v1/balance",
                get(|State(state): State<Fixture>, h: HeaderMap| async move {
                    headers(&h);
                    state.calls.fetch_add(1, Ordering::SeqCst);
                    (
                        *state.status.lock().unwrap(),
                        Json(state.balance.lock().unwrap().clone()),
                    )
                }),
            )
            .fallback(|State(state): State<Fixture>| async move {
                state.calls.fetch_add(1, Ordering::SeqCst);
                StatusCode::NOT_FOUND
            })
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let http = ProviderHttpClient::new(&base, Duration::from_secs(2), 1024 * 1024).unwrap();
        let client = StripeBillingClient::with_transport(
            "opaque-credential-with-no-mode-prefix".into(),
            http,
        )
        .unwrap();
        (client, state, server, base)
    }
    #[tokio::test]
    async fn direct_context_requires_exact_fresh_immutable_client() {
        let (client, state, server, base) = fixture().await;
        let mut context = client.verify_context("acct_owned", false).await.unwrap();
        assert_eq!(state.calls.load(Ordering::SeqCst), 2);
        assert_eq!(context.account_id(), "acct_owned");
        assert!(!context.livemode());
        context.ensure_current(&client.clone()).unwrap();
        let other = StripeBillingClient::with_transport(
            "opaque-credential-with-no-mode-prefix".into(),
            ProviderHttpClient::new(&base, Duration::from_secs(2), 1024 * 1024).unwrap(),
        )
        .unwrap();
        assert!(context.ensure_current(&other).is_err());
        assert!(
            context
                .ensure_current(
                    &client
                        .clone()
                        .with_api_version("2026-08-26.dahlia".into())
                        .unwrap()
                )
                .is_err()
        );
        context.started_at = Instant::now().checked_sub(CONTEXT_LIFETIME).unwrap();
        assert!(context.ensure_current(&client).is_err());
        *state.account.lock().unwrap() = json!({"id":"acct_changed","object":"account"});
        assert!(client.verify_context("acct_owned", false).await.is_err());
        *state.account.lock().unwrap() = json!({"id":"acct_owned","object":"account"});
        *state.balance.lock().unwrap() = json!({"object":"balance","livemode":true});
        assert!(client.verify_context("acct_owned", false).await.is_err());
        assert!(
            client
                .verify_context("acct_owned", true)
                .await
                .unwrap()
                .livemode()
        );
        let calls = state.calls.load(Ordering::SeqCst);
        let managed =
            StripeBillingClient::managed(&base, "instance-owned".into(), "owned-hmac-key".into())
                .unwrap();
        assert_eq!(
            managed
                .verify_context("acct_owned", false)
                .await
                .err()
                .unwrap()
                .code,
            "billing_managed_context_unavailable"
        );
        assert_eq!(state.calls.load(Ordering::SeqCst), calls);
        server.abort();
    }
    #[tokio::test]
    async fn provider_context_denies_malformed_and_auth_failure() {
        let (client, state, server, _) = fixture().await;
        for account in [
            json!(null),
            json!([]),
            json!({"id":"acct_owned"}),
            json!({"id":"acct_owned","object":"customer"}),
            json!({"object":"account"}),
        ] {
            *state.account.lock().unwrap() = account;
            assert!(client.verify_context("acct_owned", false).await.is_err());
        }
        *state.account.lock().unwrap() = json!({"id":"acct_owned","object":"account"});
        for balance in [
            json!(null),
            json!([]),
            json!({"object":"balance"}),
            json!({"object":"balance","livemode":"false"}),
            json!({"object":"customer","livemode":false}),
        ] {
            *state.balance.lock().unwrap() = balance;
            assert!(client.verify_context("acct_owned", false).await.is_err());
        }
        *state.balance.lock().unwrap() = json!({"object":"balance","livemode":false});
        for status in [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN] {
            *state.status.lock().unwrap() = status;
            let error = client
                .verify_context("acct_owned", false)
                .await
                .err()
                .unwrap();
            assert!(!error.message.contains("acct_owned"));
            assert_eq!(error.code, "provider_request_failed");
        }
        let calls = state.calls.load(Ordering::SeqCst);
        assert!(client.verify_context(" ", false).await.is_err());
        assert_eq!(state.calls.load(Ordering::SeqCst), calls);
        server.abort();
    }
}
