use axum::{
    Json, Router,
    extract::{Form, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use forklaunch_blueprint_core::provider_http::ProviderHttpClient;
use forklaunch_ecommerce_base::{
    BaseEcommerceService,
    store::{EcommerceStore, InMemoryEcommerceStore, Query, RecordKind},
};
use forklaunch_ecommerce_interface::{
    ConfirmPaymentDto, CreatePaymentDto, FailPaymentDto, PaymentService, PaymentStatus,
};
use forklaunch_ecommerce_stripe::{StripeClient, StripeConnect, StripePaymentService};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
#[derive(Clone, Default)]
struct Fixture {
    calls: Arc<Mutex<Vec<serde_json::Value>>>,
}
async fn create(
    State(state): State<Fixture>,
    headers: HeaderMap,
    Form(fields): Form<HashMap<String, String>>,
) -> Response {
    assert_eq!(headers["authorization"], "Bearer owned-test-stripe-secret");
    state.calls.lock().unwrap().push(json!({"method":"POST","fields":fields,"account":headers.get("stripe-account").map(|v|v.to_str().unwrap())}));
    match fields["metadata[orderId]"].as_str() {
        "error" => (
            StatusCode::BAD_GATEWAY,
            "private provider body: owned-test-stripe-secret",
        )
            .into_response(),
        "redirect" => (StatusCode::FOUND, [("location", "/trap")]).into_response(),
        "large" => Json(json!({"id":"pi_large","client_secret":"x".repeat(4000)})).into_response(),
        "slow" => {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Json(json!({"id":"pi_slow"})).into_response()
        }
        "no-secret" => Json(json!({"id":"pi_no_secret","client_secret":null})).into_response(),
        _ => {
            Json(json!({"id":"pi_owned","client_secret":"ephemeral-create-secret"})).into_response()
        }
    }
}
async fn retrieve(
    State(state): State<Fixture>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Json<serde_json::Value> {
    state.calls.lock().unwrap().push(json!({"method":"GET","id":id,"account":headers.get("stripe-account").map(|v|v.to_str().unwrap())}));
    Json(json!({"id":id,"client_secret":"ephemeral-resume-secret"}))
}
fn request(id: &str, order: &str, amount: i64) -> CreatePaymentDto {
    CreatePaymentDto {
        id: Some(id.into()),
        order_id: order.into(),
        amount_cents: amount,
        currency: "usd".into(),
    }
}
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn live_stripe_connect_resume_secrets_failures_bounds_and_persistence() {
    let state = Fixture::default();
    let app = Router::new()
        .route("/v1/payment_intents", post(create))
        .route("/v1/payment_intents/{id}", get(retrieve))
        .route(
            "/trap",
            get(|| async {
                panic!("Provider redirects must never be followed");
                #[allow(unreachable_code)]
                "trap"
            }),
        )
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let store = Arc::new(InMemoryEcommerceStore::default());
    let base = BaseEcommerceService::new(Arc::clone(&store));
    let client = StripeClient::with_transport(
        "owned-test-stripe-secret".into(),
        ProviderHttpClient::new(&origin, Duration::from_secs(2), 1024).unwrap(),
    )
    .unwrap();
    assert!(!format!("{client:?}").contains("owned-test-stripe-secret"));
    let service = StripePaymentService::new(base.clone(), client.clone())
        .with_connect(StripeConnect::new("acct_merchant".into(), 250).unwrap());
    let created = service
        .create_payment(request("pay", "order", 5350))
        .await
        .unwrap();
    assert_eq!(
        created.client_secret.as_deref(),
        Some("ephemeral-create-secret")
    );
    assert_eq!(created.provider_ref.as_deref(), Some("pi_owned"));
    assert!(!format!("{created:?}").contains("ephemeral-create-secret"));
    let wire = serde_json::to_value(&created).unwrap();
    assert_eq!(wire["clientSecret"], "ephemeral-create-secret");
    assert_eq!(wire["id"], "pay");
    assert!(wire.get("payment").is_none());
    let persisted =
        serde_json::to_value(store.find(Query::all(RecordKind::Payment)).await.unwrap())
            .unwrap()
            .to_string();
    assert!(!persisted.contains("Secret"));
    assert!(!persisted.contains("ephemeral"));
    let resumed = service.resume_payment("pay").await.unwrap();
    assert_eq!(
        resumed.client_secret.as_deref(),
        Some("ephemeral-resume-secret")
    );
    assert_eq!(resumed.id, "pay");
    assert_eq!(
        service.get_payment("pay").await.unwrap().status,
        PaymentStatus::Pending
    );
    {
        let calls = state.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["fields"]["application_fee_amount"], "134");
        assert_eq!(calls[0]["fields"]["metadata[orderId]"], "order");
        for call in calls.iter() {
            assert_eq!(call["account"], "acct_merchant");
        }
    }
    base.create_payment(request("providerless", "other", 100))
        .await
        .unwrap();
    let count = state.calls.lock().unwrap().len();
    assert!(
        service
            .resume_payment("providerless")
            .await
            .unwrap()
            .client_secret
            .is_none()
    );
    assert_eq!(state.calls.lock().unwrap().len(), count);
    service
        .confirm_payment(ConfirmPaymentDto {
            provider_ref: "pi_owned".into(),
        })
        .await
        .unwrap();
    service
        .fail_payment(FailPaymentDto {
            provider_ref: "pi_owned".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        service.get_payment("pay").await.unwrap().status,
        PaymentStatus::Succeeded
    );
    let direct = StripePaymentService::new(
        BaseEcommerceService::new(Arc::new(InMemoryEcommerceStore::default())),
        client,
    );
    assert!(
        direct
            .create_payment(request("none", "no-secret", 1))
            .await
            .unwrap()
            .client_secret
            .is_none()
    );
    {
        let calls = state.calls.lock().unwrap();
        let last = calls.last().unwrap();
        assert!(last["account"].is_null());
        assert!(last["fields"].get("application_fee_amount").is_none());
    }
    for order in ["error", "redirect", "large"] {
        let count = state.calls.lock().unwrap().len();
        let error = direct
            .create_payment(request(order, order, 100))
            .await
            .unwrap_err();
        assert!(!format!("{error:?}").contains("private provider"));
        assert!(!format!("{error:?}").contains("owned-test-stripe-secret"));
        assert!(direct.get_payment(order).await.is_err());
        assert_eq!(
            state.calls.lock().unwrap().len(),
            count + 1,
            "Provider failures must not retry"
        );
    }
    let short = StripeClient::with_transport(
        "owned-test-stripe-secret".into(),
        ProviderHttpClient::new(&origin, Duration::from_millis(10), 1024).unwrap(),
    )
    .unwrap();
    assert!(
        short
            .create_intent(&request("slow", "slow", 100), None)
            .await
            .is_err()
    );
    task.abort();
    let _ = task.await;
}
#[test]
fn connect_fees_round_and_cap_without_overflow() {
    for (amount, bps, expected) in [
        (5350, 250, 134),
        (1, 9999, 0),
        (0, 9999, 0),
        (100, 0, 0),
        (101, 5000, 51),
        (100, 9999, 99),
        (i64::MAX, 9999, 9_222_449_699_651_090_329),
    ] {
        assert_eq!(
            StripeConnect::new("acct_test".into(), bps)
                .unwrap()
                .fee_cents(amount)
                .unwrap(),
            expected
        );
    }
    for bps in [10000, 10001, u16::MAX] {
        assert!(StripeConnect::new("acct_test".into(), bps).is_err());
    }
    assert!(StripeConnect::new("acct\ninvalid".into(), 0).is_err());
    assert!(
        StripeConnect::new("acct_test".into(), 1)
            .unwrap()
            .fee_cents(-1)
            .is_err()
    );
}
