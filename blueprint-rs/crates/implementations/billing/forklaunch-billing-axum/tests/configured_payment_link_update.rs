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
use forklaunch_billing_base::services::{BillingKind, BillingStore, Scope};

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
        path.split('?').next().unwrap(),
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
#[ignore = "owned restricted PostgreSQL configured PaymentLink update HTTP"]
async fn configured_payment_link_update_backend() {
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
async fn update(
    http: &reqwest::Client,
    url: &str,
    id: &str,
    body: &Value,
    correlation: Option<&str>,
) -> reqwest::Response {
    let path = format!("/payment-link/{id}");
    let mut request = http
        .put(format!("{url}{path}"))
        .header("authorization", sign("PUT", &path, Some(body), "default"))
        .json(body);
    if let Some(value) = correlation {
        request = request.header("x-correlation-id", value);
    }
    request.send().await.unwrap()
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
    let backend = BillingBackend::stripe((*f.store).clone(), client)
        .with_configured_payment_link_updates(true);
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
    let id = Uuid::new_v4().to_string();
    let cached = json!({"id":id,"amount":12,"currency":"USD","status":"PENDING","paymentMethods":["card"],"providerFields":{"id":"plink_retained","object":"payment_link","livemode":false}});
    *state.response.lock().await =
        json!({"id":id,"object":"payment_link","line_items":{"data":[{"amount_total":34}]}});
    for correlation in [None, Some("preflight"), Some("")] {
        let response = update(&http, &url, &id, &json!({}), correlation).await;
        assert_eq!(response.status(), 500);
        let observed = response.headers()["x-correlation-id"]
            .to_str()
            .unwrap()
            .to_owned();
        if let Some(value) = correlation.filter(|value| !value.is_empty()) {
            assert_eq!(observed, value);
        } else {
            assert!(Uuid::parse_str(&observed).is_ok());
        }
        assert_eq!(
            response.text().await.unwrap(),
            format!("Internal server error:\n\nCorrelation id: {observed}")
        );
        assert!(state.calls.lock().await.is_empty());
    }
    f.store
        .insert(
            Scope::Cache,
            BillingKind::PaymentLink,
            cached.clone(),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
    // Request admission precedes this service hook. Explicit null remains the
    // existing native admission restriction; TS coercion is separately audited.
    for patch in [
        json!({"status":"INVALID"}),
        json!({"currency":"INVALID"}),
        json!({"paymentMethods":["invalid"]}),
        json!({"amount":"bad"}),
        json!({"amount":null}),
    ] {
        assert_eq!(update(&http, &url, &id, &patch, None).await.status(), 400);
        assert!(state.calls.lock().await.is_empty());
    }
    for (field, value) in [
        ("id", json!("other")),
        ("createdAt", json!("bad-date")),
        ("status", json!("INVALID")),
    ] {
        let mut tx = f.owner.begin().await.unwrap();
        sqlx::query("SELECT set_config('forklaunch.tenant_id','portal-a',true)")
            .execute(&mut *tx)
            .await
            .unwrap();
        let original:sqlx::types::Json<Value>=sqlx::query_scalar("SELECT data FROM fl_billing.records WHERE tenant_id='portal-a' AND scope='cache' AND kind='payment_link' AND id=$1").bind(&id).fetch_one(&mut *tx).await.unwrap();
        sqlx::query("UPDATE fl_billing.records SET data=jsonb_set(data,ARRAY[$2::text],$3) WHERE tenant_id='portal-a' AND scope='cache' AND kind='payment_link' AND id=$1").bind(&id).bind(field).bind(sqlx::types::Json(value)).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let response = update(&http, &url, &id, &json!({}), Some("bad-cache")).await;
        assert_eq!(response.status(), 500);
        assert_eq!(
            response.text().await.unwrap(),
            "Internal server error:\n\nCorrelation id: bad-cache"
        );
        assert!(state.calls.lock().await.is_empty());
        let mut tx = f.owner.begin().await.unwrap();
        sqlx::query("SELECT set_config('forklaunch.tenant_id','portal-a',true)")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE fl_billing.records SET data=$2 WHERE tenant_id='portal-a' AND scope='cache' AND kind='payment_link' AND id=$1").bind(&id).bind(original).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
    for status in [404, 429, 500, 599] {
        *state.status.lock().await = Some(status);
        let response = update(&http, &url, &id, &json!({}), Some("provider")).await;
        let expected = if status == 599 { 500 } else { status };
        assert_eq!(response.status().as_u16(), expected);
        assert_eq!(
            response.text().await.unwrap(),
            format!(
                "{}:\n\nCorrelation id: provider",
                if expected >= 500 {
                    "Internal server error"
                } else {
                    "Request could not be processed"
                }
            )
        );
        assert_eq!(
            f.store
                .get(Scope::Cache, BillingKind::PaymentLink, &id)
                .await
                .unwrap(),
            Some(cached.clone())
        );
    }
    *state.status.lock().await = None;
    state.calls.lock().await.clear();
    let response = update(&http, &url, &id, &json!({"status":"COMPLETED"}), None).await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["status"], "COMPLETED");
    assert_eq!(body["amount"], 34.0);
    assert_eq!(
        *state.calls.lock().await,
        vec![format!("POST /v1/payment_links/{id}")]
    );
    assert_eq!(body["stripeFields"]["id"], id);
    server.abort();
    provider_task.abort();
}
