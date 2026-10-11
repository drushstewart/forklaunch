use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use forklaunch_billing_stripe::client::{StripeBillingClient, StripeResource};
use forklaunch_blueprint_core::provider_http::ProviderHttpClient;
use serde_json::json;
use std::time::Duration;
async fn provider(Path(id): Path<String>) -> Response {
    match id.as_str() {
        "large" => Json(json!({"id":id,"private":"x".repeat(4096)})).into_response(),
        "slow" => {
            tokio::time::sleep(Duration::from_millis(150)).await;
            Json(json!({"id":id})).into_response()
        }
        "error" => (StatusCode::BAD_GATEWAY, "sk_fixture private@example.test").into_response(),
        "redirect" => (StatusCode::FOUND, [("location", "/trap")]).into_response(),
        "missing-id" => Json(json!({"secret":"sk_fixture"})).into_response(),
        "invalid-id" => Json(json!({"id":""})).into_response(),
        "array" => Json(json!([])).into_response(),
        "bad-list" => Json(json!({"data":{}})).into_response(),
        _ => Json(json!({"id":id})).into_response(),
    }
}
#[tokio::test]
async fn responses_are_bounded_sanitized_and_validated_without_redirects() {
    let app = Router::new()
        .route("/v1/plans/{id}", get(provider))
        .route("/v1/plans", get(|| async { Json(json!({"data":{}})) }))
        .route(
            "/trap",
            get(|| async {
                panic!("must never follow provider redirects");
                #[allow(unreachable_code)]
                "trap"
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = StripeBillingClient::with_transport(
        "sk_fixture".into(),
        ProviderHttpClient::new(&base, Duration::from_millis(50), 1024).unwrap(),
    )
    .unwrap();
    assert!(!format!("{client:?}").contains("sk_fixture"));
    for id in [
        "large",
        "slow",
        "error",
        "redirect",
        "missing-id",
        "invalid-id",
        "array",
    ] {
        let error = client
            .retrieve(StripeResource::Plan, id, &json!({}))
            .await
            .unwrap_err();
        assert!(!error.message.contains("sk_fixture"));
        assert!(!error.message.contains("private@example.test"));
    }
    assert!(client.list(StripeResource::Plan, &json!({})).await.is_err());
    assert_eq!(
        client
            .retrieve(StripeResource::Plan, "good", &json!({}))
            .await
            .unwrap()["id"],
        "good"
    );
    assert!(
        client
            .action(StripeResource::Plan, "good", "resume", &json!({}))
            .await
            .is_err()
    );
    server.abort();
}
