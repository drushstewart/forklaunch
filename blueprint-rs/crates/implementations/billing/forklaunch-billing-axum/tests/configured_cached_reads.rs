#[path = "../../forklaunch-billing-stripe/tests/support/bound_portal_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use axum::{
    Json, Router,
    extract::State,
    http::{Method, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use fixture::Fixture;
use forklaunch_auth::{
    jwt::{RemoteJwks, RemoteJwksConfig},
    request_hmac::{SignedBody, generate_hmac_auth_header},
};
use forklaunch_axum::module_auth::HmacJwksAuth;
use forklaunch_billing_axum::{BillingBackend, billing_router};
use forklaunch_billing_base::services::{BaseBillingService, BillingKind, BillingStore, Scope};

use forklaunch_billing_stripe::client::StripeBillingClient;
use forklaunch_blueprint_core::provider_http::ProviderHttpClient;
use forklaunch_core::ServiceInfo;
use jsonwebtoken::{Algorithm, Validation};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use uuid::Uuid;
const KEY: &[u8] = b"billing-owner-fixture";
fn sign(method: &str, path: &str, body: Option<&Value>, actor: &str) -> String {
    let encoded = body.map(Value::to_string);
    generate_hmac_auth_header(
        KEY,
        actor,
        method,
        path,
        encoded
            .as_deref()
            .map_or(SignedBody::Absent, SignedBody::Text),
        &chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        &Uuid::new_v4().to_string(),
    )
    .unwrap()
}

#[derive(Clone, Default)]
struct Provider {
    calls: Arc<Mutex<Vec<String>>>,
    response: Arc<Mutex<Value>>,
    status: Arc<Mutex<Option<u16>>>,
}
async fn provider(State(state): State<Provider>, method: Method, uri: Uri) -> Response {
    state
        .calls
        .lock()
        .await
        .push(format!("{method} {}", uri.path()));
    let status = *state.status.lock().await;
    if status == Some(599) {
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    (
        StatusCode::from_u16(status.unwrap_or(200)).unwrap(),
        Json(state.response.lock().await.clone()),
    )
        .into_response()
}
#[tokio::test]
#[ignore = "owned restricted PostgreSQL and configured cache GET HTTP"]
async fn configured_cached_reads_backend() {
    let f = Arc::new(Fixture::new().await);
    let run = f.clone();
    let result = tokio::task::LocalSet::new()
        .run_until(async move {
            tokio::task::spawn_local(async move { Box::pin(matrix(&run)).await }).await
        })
        .await;
    Arc::try_unwrap(f).ok().unwrap().cleanup().await;
    result.unwrap();
}
#[allow(clippy::too_many_lines)]
async fn matrix(f: &Fixture) {
    let state = Provider::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider_url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().fallback(provider).with_state(state.clone());
    let provider_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = StripeBillingClient::with_transport(
        "sk_fixture".into(),
        ProviderHttpClient::new(&provider_url, Duration::from_millis(100), 4096).unwrap(),
    )
    .unwrap();
    let base = BaseBillingService::new(f.store.clone());
    let backend =
        BillingBackend::stripe((*f.store).clone(), client).with_configured_cached_reads(true);
    let auth = HmacJwksAuth::new(
        KEY.to_vec(),
        RemoteJwks::new(RemoteJwksConfig::new("http://127.0.0.1:9/jwks")).unwrap(),
        Validation::new(Algorithm::RS256),
    )
    .unwrap();
    let router = billing_router(backend, auth, ServiceInfo::new("billing", "1"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router.into_router_with_default_mcp().unwrap())
            .await
            .unwrap();
    });
    let http = reqwest::Client::new();
    for checkout in [false, true] {
        let id = Uuid::new_v4().to_string();
        let kind = if checkout {
            BillingKind::CheckoutSession
        } else {
            BillingKind::PaymentLink
        };
        let evidence = if checkout {
            json!({"id":"cs_owned","object":"checkout.session","mode":"subscription","customer":null,"livemode":false})
        } else {
            json!({"id":"plink_owned","object":"payment_link","livemode":false})
        };
        let mut cached = json!({"id":id,"amount":12,"currency":"USD","status":"PENDING","paymentMethods":["card"],"providerFields":evidence});
        if checkout {
            cached["customerId"] = json!("local-party");
            cached["expiresAt"] = json!("2026-10-20T00:00:00Z");
        }
        base.store()
            .insert(Scope::Cache, kind, cached.clone(), Duration::from_secs(60))
            .await
            .unwrap();
        let path = format!(
            "/{}/{}",
            if checkout {
                "checkout-session"
            } else {
                "payment-link"
            },
            id
        );
        *state.response.lock().await = evidence.clone();
        state.calls.lock().await.clear();
        let response = http
            .get(format!("{url}{path}"))
            .header("authorization", sign("GET", &path, None, "default"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let value: Value = response.json().await.unwrap();
        assert_eq!(value["id"], id);
        assert_eq!(value["stripeFields"], evidence);
        assert_eq!(state.calls.lock().await.len(), 1);
        assert_eq!(
            base.store().get(Scope::Cache, kind, &id).await.unwrap(),
            Some(cached.clone())
        );
        // Owner-seeded legacy cache scalars exercise the real decrypt/validate
        // boundary. The configured read normalizes a copy, never the ciphertext.
        let fields = if checkout {
            vec!["expiresAt", "createdAt", "updatedAt"]
        } else {
            vec!["createdAt", "updatedAt"]
        };
        for field in fields {
            for (input, expected) in [
                (json!(0), Some(0)),
                (json!(1.9), Some(1)),
                (json!(-1.9), Some(-1)),
                (json!("2026-01-01"), Some(1_767_225_600_000)),
                (json!("+010000-01-01T00:00:00Z"), Some(253_402_300_800_000)),
                (json!(null), None),
                (json!(8_640_000_000_000_000_i64), None),
            ] {
                let mut tx = f.owner.begin().await.unwrap();
                sqlx::query("SELECT set_config('forklaunch.tenant_id','portal-a',true)")
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                let original:sqlx::types::Json<Value>=sqlx::query_scalar("SELECT data FROM fl_billing.records WHERE tenant_id='portal-a' AND scope='cache' AND kind=$1 AND id=$2").bind(kind.as_str()).bind(&id).fetch_one(&mut *tx).await.unwrap();
                let modified:sqlx::types::Json<Value>=sqlx::query_scalar("UPDATE fl_billing.records SET data=jsonb_set(data,ARRAY[$3::text],$4) WHERE tenant_id='portal-a' AND scope='cache' AND kind=$1 AND id=$2 RETURNING data").bind(kind.as_str()).bind(&id).bind(field).bind(sqlx::types::Json(input.clone())).fetch_one(&mut *tx).await.unwrap();
                tx.commit().await.unwrap();
                if input.is_number() {
                    assert!(
                        base.store().get(Scope::Cache, kind, &id).await.is_err(),
                        "generic typed read unchanged"
                    );
                }
                state.calls.lock().await.clear();
                let response = http
                    .get(format!("{url}{path}"))
                    .header("authorization", sign("GET", &path, None, "default"))
                    .send()
                    .await
                    .unwrap();
                if let Some(expected) = expected {
                    assert_eq!(response.status(), 200, "{field}/{input}");
                    let body: Value = response.json().await.unwrap();
                    assert_eq!(
                        serde_json::from_value::<chrono::DateTime<chrono::Utc>>(
                            body[field].clone()
                        )
                        .unwrap()
                        .timestamp_millis(),
                        expected
                    );
                    assert_eq!(state.calls.lock().await.len(), 1);
                } else {
                    assert_eq!(response.status(), 500);
                    assert!(state.calls.lock().await.is_empty());
                }
                let mut tx = f.owner.begin().await.unwrap();
                sqlx::query("SELECT set_config('forklaunch.tenant_id','portal-a',true)")
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                let after:sqlx::types::Json<Value>=sqlx::query_scalar("SELECT data FROM fl_billing.records WHERE tenant_id='portal-a' AND scope='cache' AND kind=$1 AND id=$2").bind(kind.as_str()).bind(&id).fetch_one(&mut *tx).await.unwrap();
                assert_eq!(after, modified);
                sqlx::query("UPDATE fl_billing.records SET data=$3 WHERE tenant_id='portal-a' AND scope='cache' AND kind=$1 AND id=$2").bind(kind.as_str()).bind(&id).bind(original).execute(&mut *tx).await.unwrap();
                tx.commit().await.unwrap();
            }
        }
        for status in [404, 429, 500, 599] {
            *state.status.lock().await = Some(status);
            for correlation in [None, Some("cache-status"), Some("")] {
                let mut request = http
                    .get(format!("{url}{path}"))
                    .header("authorization", sign("GET", &path, None, "default"));
                if let Some(correlation) = correlation {
                    request = request.header("x-correlation-id", correlation);
                }
                let response = request.send().await.unwrap();
                let expected = if status == 599 { 500 } else { status };
                assert_eq!(
                    response.status().as_u16(),
                    expected,
                    "family={checkout} provider={status} correlation={correlation:?} calls={:?}",
                    state.calls.lock().await
                );
                let observed = response
                    .headers()
                    .get("x-correlation-id")
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned();
                if correlation.is_some_and(|value| !value.is_empty()) {
                    assert_eq!(observed, correlation.unwrap());
                } else {
                    assert!(Uuid::parse_str(&observed).is_ok());
                }
                assert_eq!(
                    response.text().await.unwrap(),
                    format!(
                        "{}:\n\nCorrelation id: {observed}",
                        if expected >= 500 {
                            "Internal server error"
                        } else {
                            "Request could not be processed"
                        }
                    )
                );
            }
        }
        *state.status.lock().await = None;
        // Real restricted storage failure stays a storage error internally but
        // receives the selected application's final HTTP error formatting.
        let role: String = sqlx::query_scalar("SELECT current_user")
            .fetch_one(&f.pool)
            .await
            .unwrap();
        sqlx::query(&format!("REVOKE SELECT ON fl_billing.records FROM {role}"))
            .execute(&f.owner)
            .await
            .unwrap();
        state.calls.lock().await.clear();
        let response = http
            .get(format!("{url}{path}"))
            .header("authorization", sign("GET", &path, None, "default"))
            .header("x-correlation-id", "storage")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 500);
        assert_eq!(
            response.text().await.unwrap(),
            "Internal server error:\n\nCorrelation id: storage"
        );
        assert!(state.calls.lock().await.is_empty());
        sqlx::query(&format!("GRANT SELECT ON fl_billing.records TO {role}"))
            .execute(&f.owner)
            .await
            .unwrap();
        *state.response.lock().await = json!({"id":"wrong"});
        let response = http
            .get(format!("{url}{path}"))
            .header("authorization", sign("GET", &path, None, "default"))
            .header("x-correlation-id", "cached-read")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 500);
        assert!(response.text().await.unwrap().contains("cached-read"));
        cached["providerFields"] = json!({});
        base.store()
            .insert(Scope::Cache, kind, cached.clone(), Duration::from_secs(60))
            .await
            .unwrap();
        state.calls.lock().await.clear();
        let response = http
            .get(format!("{url}{path}"))
            .header("authorization", sign("GET", &path, None, "default"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 500);
        assert!(state.calls.lock().await.is_empty());
    }
    state.calls.lock().await.clear();
    for (path, expected) in [
        (format!("/payment-link/{}", Uuid::new_v4()), 500),
        ("/payment-link/%0A".into(), 400),
    ] {
        let response = http
            .get(format!("{url}{path}"))
            .header("authorization", sign("GET", &path, None, "default"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), expected);
        assert!(state.calls.lock().await.is_empty());
    }
    server.abort();
    provider_task.abort();
}
