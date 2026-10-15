//! Actual configured read-hook counterparts: public identity stays local.
use axum::{
    Json, Router,
    extract::State,
    http::{Method, Uri},
};
use forklaunch_billing_base::services::{
    BaseBillingService, BillingKind, BillingStore, Scope, memory::MemoryBillingStore,
};
use forklaunch_billing_interface::services::{CheckoutSessionService, PaymentLinkService};
use forklaunch_billing_stripe::{client::StripeBillingClient, services::StripeBillingService};
use forklaunch_blueprint_core::provider_http::ProviderHttpClient;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
#[derive(Clone, Default)]
struct Provider {
    result: Arc<Mutex<Value>>,
    calls: Arc<Mutex<Vec<String>>>,
}
async fn provider(State(state): State<Provider>, method: Method, uri: Uri) -> Json<Value> {
    state
        .calls
        .lock()
        .await
        .push(format!("{method} {}", uri.path()));
    Json(state.result.lock().await.clone())
}
fn evidence(checkout: bool) -> Value {
    if checkout {
        json!({"id":"cs_owned","object":"checkout.session","mode":"subscription","livemode":false,"customer":"cus_owned"})
    } else {
        json!({"id":"plink_owned","object":"payment_link","livemode":false})
    }
}
fn record(checkout: bool) -> Value {
    let mut value = json!({"id":"public-local-id","amount":12,"currency":"USD","status":"PENDING","paymentMethods":["card"],"providerFields":evidence(checkout)});
    if checkout {
        value["customerId"] = json!("local-customer");
        value["expiresAt"] = json!("2026-10-20T00:00:00Z");
        value["uri"] = json!("https://local.example/original");
    }
    value
}
async fn read<S: BillingStore>(
    service: &StripeBillingService<S>,
    checkout: bool,
    id: &str,
) -> Result<Value, String> {
    if checkout {
        service
            .get_checkout_session(id)
            .await
            .map(|value| serde_json::to_value(value).unwrap())
            .map_err(|error| error.code)
    } else {
        service
            .get_payment_link(id)
            .await
            .map(|value| serde_json::to_value(value).unwrap())
            .map_err(|error| error.code)
    }
}
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn configured_cache_reference_matrix_preserves_public_fields_and_never_writes() {
    let state = Provider::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().fallback(provider).with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for checkout in [false, true] {
        let kind = if checkout {
            BillingKind::CheckoutSession
        } else {
            BillingKind::PaymentLink
        };
        let store = Arc::new(MemoryBillingStore::new());
        let client = StripeBillingClient::with_transport(
            "sk_fixture".into(),
            ProviderHttpClient::new(&url, Duration::from_secs(2), 4096).unwrap(),
        )
        .unwrap();
        let service = StripeBillingService::new(BaseBillingService::new(store.clone()), client)
            .with_configured_cached_reads(true);
        let original = record(checkout);
        store
            .insert(
                Scope::Cache,
                kind,
                original.clone(),
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        let mut fresh = evidence(checkout);
        fresh["metadata"] = json!({"opaqueDate":"2026-01-02T00:00:00Z"});
        fresh["active"] = json!(false); // Single GET is enrichment, never an active-only filter.
        *state.result.lock().await = fresh.clone();
        state.calls.lock().await.clear();
        let result = read(&service, checkout, "public-local-id").await.unwrap();
        assert_eq!(result["id"], original["id"]);
        assert_eq!(result["status"], original["status"]);
        assert_eq!(result["providerFields"], fresh);
        assert_eq!(
            *state.calls.lock().await,
            vec![if checkout {
                "GET /v1/checkout/sessions/cs_owned"
            } else {
                "GET /v1/payment_links/plink_owned"
            }]
        );
        assert_eq!(
            store
                .get(Scope::Cache, kind, "public-local-id")
                .await
                .unwrap(),
            Some(original.clone())
        );
        for patch in [
            json!({"id":"other"}),
            json!({"id":null}),
            json!({"id":4}),
            json!({"object":"other"}),
            json!({"livemode":true}),
            json!({"livemode":null}),
        ] {
            let mut bad = evidence(checkout);
            bad.as_object_mut()
                .unwrap()
                .extend(patch.as_object().unwrap().clone());
            *state.result.lock().await = bad;
            state.calls.lock().await.clear();
            assert_eq!(
                read(&service, checkout, "public-local-id")
                    .await
                    .unwrap_err(),
                "billing_configured_mapper_error"
            );
            assert_eq!(state.calls.lock().await.len(), 1);
            assert_eq!(
                store
                    .get(Scope::Cache, kind, "public-local-id")
                    .await
                    .unwrap(),
                Some(original.clone())
            );
        }
        for patch in [
            json!({"currency":"invalid"}),
            json!({"status":"invalid"}),
            json!({"paymentMethods":["invalid"]}),
            json!({"providerFields":{}}),
        ] {
            let mut bad = original.clone();
            bad.as_object_mut()
                .unwrap()
                .extend(patch.as_object().unwrap().clone());
            store
                .insert(Scope::Cache, kind, bad, Duration::from_secs(60))
                .await
                .unwrap();
            state.calls.lock().await.clear();
            assert!(read(&service, checkout, "public-local-id").await.is_err());
            assert!(state.calls.lock().await.is_empty());
        }
        state.calls.lock().await.clear();
        assert!(read(&service, checkout, "missing").await.is_err());
        assert!(state.calls.lock().await.is_empty());
        store
            .insert(
                Scope::Cache,
                kind,
                original.clone(),
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        if checkout {
            for customer in [
                json!(null),
                json!("cus_other"),
                json!({"id":"cus_owned","object":"customer","deleted":null}),
                json!({"id":"cus_owned","object":"customer","deleted":true}),
                json!({"id":"cus_owned","object":"other"}),
            ] {
                let mut bad = evidence(true);
                bad["customer"] = customer;
                *state.result.lock().await = bad;
                state.calls.lock().await.clear();
                assert!(read(&service, true, "public-local-id").await.is_err());
                assert_eq!(state.calls.lock().await.len(), 1);
            }
            let mut cached = original.clone();
            cached["providerFields"]["customer"] = Value::Null;
            store
                .insert(Scope::Cache, kind, cached.clone(), Duration::from_secs(60))
                .await
                .unwrap();
            *state.result.lock().await = evidence(true);
            assert_eq!(
                read(&service, true, "public-local-id").await.unwrap()["customerId"],
                "local-customer"
            );
            assert_eq!(
                store
                    .get(Scope::Cache, kind, "public-local-id")
                    .await
                    .unwrap(),
                Some(cached)
            );
        }
        // Opt-out keeps the custom/generic provider-first caller-ID sequence.
        let service = service.with_configured_cached_reads(false);
        *state.result.lock().await = json!({"id":"public-local-id","custom":true});
        state.calls.lock().await.clear();
        read(&service, checkout, "public-local-id").await.unwrap();
        assert_eq!(
            *state.calls.lock().await,
            vec![if checkout {
                "GET /v1/checkout/sessions/public-local-id"
            } else {
                "GET /v1/payment_links/public-local-id"
            }]
        );
    }
    server.abort();
}

struct RawCache(Value);
impl BillingStore for RawCache {
    async fn get(
        &self,
        _: Scope,
        _: BillingKind,
        _: &str,
    ) -> forklaunch_blueprint_core::BlueprintResult<Option<Value>> {
        Ok(Some(self.0.clone()))
    }
    async fn insert(
        &self,
        _: Scope,
        _: BillingKind,
        _: Value,
        _: Duration,
    ) -> forklaunch_blueprint_core::BlueprintResult<Value> {
        panic!("read wrote cache")
    }
    async fn patch(
        &self,
        _: Scope,
        _: BillingKind,
        _: &str,
        _: Value,
        _: Duration,
    ) -> forklaunch_blueprint_core::BlueprintResult<Value> {
        panic!("read patched cache")
    }
    async fn delete(
        &self,
        _: Scope,
        _: BillingKind,
        _: &str,
    ) -> forklaunch_blueprint_core::BlueprintResult<bool> {
        panic!("read deleted cache")
    }
    async fn list(
        &self,
        _: Scope,
        _: BillingKind,
        _: Option<&[String]>,
    ) -> forklaunch_blueprint_core::BlueprintResult<Vec<Value>> {
        panic!("read enumerated")
    }
    async fn find_active_subscription(
        &self,
        _: &str,
        _: &str,
    ) -> forklaunch_blueprint_core::BlueprintResult<Option<Value>> {
        panic!("unrelated read")
    }
    async fn find_subscription_id_by_external_id(
        &self,
        _: &str,
    ) -> forklaunch_blueprint_core::BlueprintResult<Option<String>> {
        panic!("unrelated read")
    }
}
#[tokio::test]
async fn malformed_cached_dto_is_mapper_failure_without_provider_or_store_mutation() {
    for checkout in [false, true] {
        for id in [" ", "\u{feff}", "\u{2028}"] {
            let mut cached = record(checkout);
            cached["id"] = json!(id);
            let client = StripeBillingClient::with_transport(
                "sk_fixture".into(),
                ProviderHttpClient::new("http://127.0.0.1:9", Duration::from_secs(1), 4096)
                    .unwrap(),
            )
            .unwrap();
            let service = StripeBillingService::new(
                BaseBillingService::new(Arc::new(RawCache(cached))),
                client,
            )
            .with_configured_cached_reads(true);
            assert_eq!(
                read(&service, checkout, id).await.unwrap_err(),
                "billing_configured_mapper_error"
            );
        }
        for patch in [
            json!({"id":"different"}),
            json!({"createdAt":"invalid"}),
            json!({"updatedAt":null}),
            json!({"providerFields":null}),
            json!({"paymentMethods":3}),
            json!({"currency":null}),
        ] {
            let mut cached = record(checkout);
            cached
                .as_object_mut()
                .unwrap()
                .extend(patch.as_object().unwrap().clone());
            let client = StripeBillingClient::with_transport(
                "sk_fixture".into(),
                ProviderHttpClient::new("http://127.0.0.1:9", Duration::from_secs(1), 4096)
                    .unwrap(),
            )
            .unwrap();
            let service = StripeBillingService::new(
                BaseBillingService::new(Arc::new(RawCache(cached.clone()))),
                client,
            )
            .with_configured_cached_reads(true);
            assert_eq!(
                read(&service, checkout, "public-local-id")
                    .await
                    .unwrap_err(),
                "billing_configured_mapper_error"
            );
            assert_eq!(service.base().store().0, cached);
            assert_eq!(
                read(&service, checkout, "").await.unwrap_err(),
                "billing_validation"
            );
        }
    }
}
