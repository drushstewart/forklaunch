//! Provider read identity is checked without changing local data or issuing writes.
use axum::{Json, Router, extract::State, routing::get};
use forklaunch_billing_base::services::{BaseBillingService, memory::MemoryBillingStore};
use forklaunch_billing_interface::services::{PlanService, SubscriptionService};
use forklaunch_billing_stripe::{client::StripeBillingClient, services::StripeBillingService};
use forklaunch_blueprint_core::provider_http::ProviderHttpClient;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

async fn response(State(body): State<Arc<Mutex<Value>>>) -> Json<Value> {
    Json(body.lock().await.clone())
}

async fn unexpected_request() -> axum::http::StatusCode {
    panic!("Unexpected provider request or mutation");
}

async fn fixture(
    party: &str,
) -> (
    StripeBillingService<MemoryBillingStore>,
    Arc<Mutex<Value>>,
    tokio::task::JoinHandle<()>,
) {
    let body = Arc::new(Mutex::new(Value::Null));
    let app = Router::new()
        .route("/v1/plans/plan_owned", get(response))
        .route("/v1/subscriptions/sub_owned", get(response))
        .fallback(unexpected_request)
        .with_state(body.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let base = BaseBillingService::new(Arc::new(MemoryBillingStore::new()));
    base.create_plan(
        serde_json::from_value(json!({
            "id":"local-plan", "name":"Owned", "price":12, "cadence":"month", "currency":"USD",
            "active":true, "externalId":"plan_owned", "billingProvider":"stripe"
        }))
        .unwrap(),
    )
    .await
    .unwrap();
    base.create_subscription(
            serde_json::from_value(json!({
                "id":"local-user-sub","partyId":"local-party","partyType":party,"productId":"local-plan",
                "active":true,"startDate":"2026-01-01T00:00:00Z","status":"ACTIVE",
                "externalId":"sub_owned","billingProvider":"stripe"
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let client = StripeBillingClient::with_transport(
        "sk_fixture".into(),
        ProviderHttpClient::new(&url, Duration::from_secs(2), 4096).unwrap(),
    )
    .unwrap();
    (StripeBillingService::new(base, client), body, server)
}

fn invalid_responses(id: &str, kind: &str) -> Vec<Value> {
    vec![
        json!({"id":"private_wrong_reference","object":kind}),
        json!({"object":kind}),
        json!({"id":null,"object":kind}),
        json!({"id":7,"object":kind}),
        json!({"id":id,"object":"customer"}),
        json!({"id":id}),
        json!({"id":id,"object":null}),
        Value::Null,
        json!([]),
    ]
}

#[tokio::test]
async fn plan_reads_reject_wrong_identity_and_lists_skip_failures_without_local_writes() {
    let (service, body, server) = fixture("USER").await;
    let before = service.base().get_plan("local-plan").await.unwrap();
    for invalid in invalid_responses("plan_owned", "plan") {
        *body.lock().await = invalid;
        let error = service.get_plan("local-plan").await.unwrap_err();
        assert_eq!(error.message, "Invalid Stripe billing response");
        assert!(service.list_plans(None).await.unwrap().is_empty());
        assert_eq!(
            serde_json::to_value(service.base().get_plan("local-plan").await.unwrap()).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
    }
    *body.lock().await =
        json!({"id":"plan_owned","object":"plan","product":{"id":"prod_owned","object":"product"}});
    let result = service.get_plan("local-plan").await.unwrap();
    assert_eq!(result.id, "local-plan");
    assert_eq!(result.external_id, "plan_owned");
    assert_eq!(result.provider_fields.unwrap()["id"], "prod_owned");
    assert_eq!(service.list_plans(None).await.unwrap().len(), 1);
    server.abort();
}

#[tokio::test]
async fn all_subscription_read_variants_check_exact_identity_without_customer_uuid_assumptions() {
    for party in ["USER", "ORGANIZATION"] {
        check_subscription_reads(party).await;
    }
}

async fn check_subscription_reads(party: &str) {
    let (service, body, server) = fixture(party).await;
    let before = service
        .base()
        .get_subscription("local-user-sub")
        .await
        .unwrap();
    for invalid in invalid_responses("sub_owned", "subscription") {
        *body.lock().await = invalid;
        let party_result = if party == "USER" {
            service.get_user_subscription("local-party").await
        } else {
            service.get_organization_subscription("local-party").await
        };
        for result in [
            service.get_subscription("local-user-sub").await,
            party_result,
        ] {
            assert_eq!(
                result.unwrap_err().message,
                "Invalid Stripe billing response"
            );
        }
        assert!(service.list_subscriptions(None).await.unwrap().is_empty());
        assert_eq!(
            serde_json::to_value(
                service
                    .base()
                    .get_subscription("local-user-sub")
                    .await
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_value(&before).unwrap()
        );
    }
    let provider = json!({"id":"sub_owned","object":"subscription","customer":"cus_distinct_from_local_party","metadata":{"note":"preserved"}});
    *body.lock().await = provider.clone();
    let party_result = if party == "USER" {
        service.get_user_subscription("local-party").await
    } else {
        service.get_organization_subscription("local-party").await
    };
    for result in [
        service.get_subscription("local-user-sub").await,
        party_result,
    ] {
        let result = result.unwrap();
        assert_eq!(result.id, "local-user-sub");
        assert_eq!(result.provider_fields, Some(provider.clone()));
    }
    assert_eq!(service.list_subscriptions(None).await.unwrap().len(), 1);
    server.abort();
}
