// Exercise actual generated module exports; raw body calls preserve signed fixture bytes.
use std::collections::BTreeMap;

use billing_sdk::*;
fn headers(name: &str) -> BTreeMap<String, String> {
    BTreeMap::from([("authorization".into(), std::env::var(name).unwrap())])
}
#[tokio::test]
async fn generated_rust_sdk_calls_persistent_billing_and_portal() {
    let client = Client::new(std::env::var("BILLING_BASE_URL").unwrap());
    let id = std::env::var("BILLING_PLAN_ID").unwrap();
    let response = client
        .get_plan(GetPlanRequest {
            params: GetPlanParams { id: id.clone() },
            headers: headers("BILLING_GET_HEADER"),
            ..Default::default()
        })
        .await
        .unwrap();
    let GetPlanResponses::Status200(plan) = response else {
        panic!("get failed")
    };
    assert_eq!(plan.id, id);
    let response = client
        .list_plans(ListPlansRequest {
            query: serde_json::from_value(serde_json::json!({"ids":[id.clone()]})).unwrap(),
            headers: headers("BILLING_LIST_HEADER"),
            ..Default::default()
        })
        .await
        .unwrap();
    let ListPlansResponses::Status200(plans) = response else {
        panic!("list failed")
    };
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].id, id);
    let response = client
        .create_plan_raw(CreatePlanRequest {
            body: Some(std::env::var("BILLING_CREATE_BODY").unwrap()),
            headers: headers("BILLING_CREATE_HEADER"),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(matches!(response, CreatePlanResponses::Status200(_)));
    // Per-request signing: every call carries a fresh timestamp and nonce over the
    // full wire path, so the same client calls plan and payment-link routes.
    let signed = Client::new(std::env::var("BILLING_BASE_URL").unwrap())
        .with_hmac_secret(std::env::var("BILLING_HMAC_SECRET").unwrap());
    for _ in 0..2 {
        let response = signed
            .get_plan(GetPlanRequest {
                params: GetPlanParams { id: id.clone() },
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(matches!(response, GetPlanResponses::Status200(_)));
    }
    let response = signed
        .create_payment_link_raw(CreatePaymentLinkRequest {
            body: Some(std::env::var("BILLING_LINK_BODY").unwrap()),
            ..Default::default()
        })
        .await
        .unwrap();
    let CreatePaymentLinkResponses::Status200(link) = response else {
        panic!("signed payment link creation failed")
    };
    // Leave no record behind for the next service sharing the fixture tenant.
    let response = signed
        .expire_payment_link(ExpirePaymentLinkRequest {
            params: ExpirePaymentLinkParams { id: link.id },
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(matches!(response, ExpirePaymentLinkResponses::Status200(_)));
    let response = client
        .get_billing_portal_session(GetBillingPortalSessionRequest {
            params: GetBillingPortalSessionParams {
                id: std::env::var("BILLING_PORTAL_ID").unwrap(),
            },
            headers: headers("BILLING_PORTAL_HEADER"),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(matches!(
        response,
        GetBillingPortalSessionResponses::Status200(_)
    ));
    if let Ok(id) = std::env::var("BILLING_WEBHOOK_PORTAL_ID") {
        let response = client
            .get_billing_portal_session(GetBillingPortalSessionRequest {
                params: GetBillingPortalSessionParams { id },
                headers: headers("BILLING_PORTAL_HEADER"),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(matches!(
            response,
            GetBillingPortalSessionResponses::Status200(_)
        ));
    }
    let response = client
        .get_plan(GetPlanRequest {
            params: GetPlanParams { id },
            ..Default::default()
        })
        .await;
    assert_missing_authorization(response);
}

fn assert_missing_authorization(response: Result<GetPlanResponses, SdkError>) {
    // The HTTP probe checks the exact 401; this route declares no 401 response.
    assert!(matches!(
        response,
        Err(SdkError::Contract(
            "undeclared response status or content type"
        ))
    ));
}
