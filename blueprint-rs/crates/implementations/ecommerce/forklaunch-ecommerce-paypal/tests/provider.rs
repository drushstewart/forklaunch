use axum::{
    Json, Router,
    extract::{Form, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use forklaunch_ecommerce_base::{BaseEcommerceService, store::InMemoryEcommerceStore};
use forklaunch_ecommerce_interface::{
    ConfirmPaymentDto, CreatePaymentDto, FailPaymentDto, PaymentService, PaymentStatus,
};
use forklaunch_ecommerce_paypal::{PaypalClient, PaypalPaymentService, VerifyWebhookSignature};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
#[derive(Clone, Default)]
struct Fixture {
    calls: Arc<Mutex<Vec<Value>>>,
    captures: Arc<AtomicUsize>,
    fail_capture: Arc<AtomicBool>,
}
async fn token(
    State(state): State<Fixture>,
    headers: HeaderMap,
    Form(fields): Form<HashMap<String, String>>,
) -> Json<Value> {
    assert_eq!(
        headers["authorization"],
        "Basic b3duZWQtY2xpZW50Om93bmVkLXNlY3JldA=="
    );
    assert_eq!(fields["grant_type"], "client_credentials");
    state
        .calls
        .lock()
        .unwrap()
        .push(json!({"operation":"token"}));
    Json(json!({"access_token":"owned-access-token"}))
}
async fn create(
    State(state): State<Fixture>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    assert_eq!(headers["authorization"], "Bearer owned-access-token");
    state
        .calls
        .lock()
        .unwrap()
        .push(json!({"operation":"create","body":body}));
    Json(json!({"id":body["purchase_units"][0]["reference_id"],"status":"CREATED"}))
}
async fn capture(
    State(state): State<Fixture>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    assert_eq!(headers["authorization"], "Bearer owned-access-token");
    state.captures.fetch_add(1, Ordering::SeqCst);
    state
        .calls
        .lock()
        .unwrap()
        .push(json!({"operation":"capture","id":id}));
    tokio::time::sleep(Duration::from_millis(50)).await;
    if state.fail_capture.swap(false, Ordering::SeqCst) {
        return (
            StatusCode::BAD_GATEWAY,
            "private provider failure owned-secret",
        )
            .into_response();
    }
    Json(json!({"id":id,"status":"COMPLETED"})).into_response()
}
async fn verify(State(state): State<Fixture>, Json(body): Json<Value>) -> Response {
    state
        .calls
        .lock()
        .unwrap()
        .push(json!({"operation":"verify","body":body}));
    match body["webhook_event"]["mode"].as_str().unwrap() {
        "success" => Json(json!({"verification_status":"SUCCESS"})).into_response(),
        "bad-shape" => Json(json!({"verification_status":42})).into_response(),
        "status-failure" => (StatusCode::UNAUTHORIZED, "not verified").into_response(),
        _ => Json(json!({"verification_status":"FAILURE"})).into_response(),
    }
}
fn request(id: &str) -> CreatePaymentDto {
    CreatePaymentDto {
        id: Some(id.into()),
        order_id: format!("ORDER_{id}"),
        amount_cents: 105,
        currency: "usd".into(),
    }
}
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn live_paypal_wire_capture_claims_retries_and_signature_verification() {
    let state = Fixture::default();
    let app = Router::new()
        .route("/v1/oauth2/token", post(token))
        .route("/v2/checkout/orders", post(create))
        .route("/v2/checkout/orders/{id}/capture", post(capture))
        .route("/v1/notifications/verify-webhook-signature", post(verify))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = PaypalClient::new("owned-client".into(), "owned-secret".into(), &origin).unwrap();
    assert!(!format!("{client:?}").contains("owned-secret"));
    assert!(!format!("{client:?}").contains("owned-client"));
    let service = Arc::new(PaypalPaymentService::new(
        BaseEcommerceService::new(Arc::new(InMemoryEcommerceStore::default())),
        client.clone(),
    ));
    let created = service.create_payment(request("pay")).await.unwrap();
    assert_eq!(created.status, PaymentStatus::Pending);
    assert_eq!(created.provider_ref.as_deref(), Some("ORDER_pay"));
    {
        let calls = state.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[1]["body"],
            json!({"intent":"CAPTURE","purchase_units":[{"reference_id":"ORDER_pay","amount":{"currency_code":"USD","value":"1.05"}}]})
        );
    }
    let mut jobs = Vec::new();
    for _ in 0..20 {
        let service = Arc::clone(&service);
        jobs.push(tokio::spawn(async move {
            service
                .confirm_payment(ConfirmPaymentDto {
                    provider_ref: "ORDER_pay".into(),
                })
                .await
                .unwrap()
        }));
    }
    for job in jobs {
        assert_eq!(job.await.unwrap().status, PaymentStatus::Succeeded);
    }
    assert_eq!(state.captures.load(Ordering::SeqCst), 1);
    service
        .confirm_payment(ConfirmPaymentDto {
            provider_ref: "ORDER_pay".into(),
        })
        .await
        .unwrap();
    service
        .fail_payment(FailPaymentDto {
            provider_ref: "ORDER_pay".into(),
        })
        .await
        .unwrap();
    assert_eq!(state.captures.load(Ordering::SeqCst), 1);
    assert_eq!(
        service.get_payment("pay").await.unwrap().status,
        PaymentStatus::Succeeded
    );
    assert!(
        service
            .confirm_payment(ConfirmPaymentDto {
                provider_ref: "missing".into()
            })
            .await
            .is_err()
    );
    assert_eq!(state.captures.load(Ordering::SeqCst), 1);
    service.create_payment(request("retry")).await.unwrap();
    state.fail_capture.store(true, Ordering::SeqCst);
    let error = service
        .confirm_payment(ConfirmPaymentDto {
            provider_ref: "ORDER_retry".into(),
        })
        .await
        .unwrap_err();
    assert!(!format!("{error:?}").contains("private provider"));
    assert_eq!(
        service.get_payment("retry").await.unwrap().status,
        PaymentStatus::Pending
    );
    assert_eq!(state.captures.load(Ordering::SeqCst), 2);
    service
        .confirm_payment(ConfirmPaymentDto {
            provider_ref: "ORDER_retry".into(),
        })
        .await
        .unwrap();
    assert_eq!(state.captures.load(Ordering::SeqCst), 3);
    service.create_payment(request("failed")).await.unwrap();
    service
        .fail_payment(FailPaymentDto {
            provider_ref: "ORDER_failed".into(),
        })
        .await
        .unwrap();
    service
        .confirm_payment(ConfirmPaymentDto {
            provider_ref: "ORDER_failed".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        state.captures.load(Ordering::SeqCst),
        3,
        "matches TS: a failed payment cannot claim an external capture"
    );
    for (mode, expected) in [
        ("success", true),
        ("failure", false),
        ("bad-shape", false),
        ("status-failure", false),
    ] {
        let params = VerifyWebhookSignature {
            transmission_id: "transmission".into(),
            transmission_time: "2026-10-10T00:00:00Z".into(),
            transmission_sig: "signature".into(),
            cert_url: "http://127.0.0.1:9/untrusted-cert".into(),
            auth_algo: "SHA256withRSA".into(),
            webhook_id: "WH-owned".into(),
            webhook_event: json!({"mode":mode}),
        };
        assert_eq!(
            client.verify_webhook_signature(&params).await.unwrap(),
            expected
        );
    }
    {
        let calls = state.calls.lock().unwrap();
        let last = calls
            .iter()
            .rev()
            .find(|c| c["operation"] == "verify")
            .unwrap();
        assert_eq!(
            last["body"]["cert_url"],
            "http://127.0.0.1:9/untrusted-cert"
        );
        assert_eq!(last["body"]["transmission_sig"], "signature");
        assert_eq!(last["body"]["webhook_id"], "WH-owned");
    }
    task.abort();
    let _ = task.await;
}
