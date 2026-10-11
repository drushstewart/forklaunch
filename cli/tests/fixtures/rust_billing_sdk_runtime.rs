use billing_sdk::*;
use std::collections::BTreeMap;
fn headers(name:&str)->BTreeMap<String,String>{BTreeMap::from([("authorization".into(),std::env::var(name).unwrap())])}
#[tokio::test]
async fn generated_rust_sdk_calls_persistent_billing_and_portal() {
    let client=BillingClient::new(std::env::var("BILLING_BASE_URL").unwrap());
    let id=std::env::var("BILLING_PLAN_ID").unwrap();
    let response=client.plan_get_plan(BillingPlanGetPlanRequest {params:BTreeMap::from([("id".into(),id.clone())]),headers:headers("BILLING_GET_HEADER"),..Default::default()}).await.unwrap();
    let BillingPlanGetPlanResponses::Status200(plan)=response else {panic!("get failed")};assert_eq!(plan.id,id);
    let response=client.plan_list_plans(BillingPlanListPlansRequest {query:BTreeMap::from([("ids".into(),serde_json::to_string(&vec![id.clone()]).unwrap())]),headers:headers("BILLING_LIST_HEADER"),..Default::default()}).await.unwrap();
    let BillingPlanListPlansResponses::Status200(plans)=response else {panic!("list failed")};assert_eq!(plans.len(),1);assert_eq!(plans[0].id,id);
    let response=client.plan_create_plan(BillingPlanCreatePlanRequest {body:Some(std::env::var("BILLING_CREATE_BODY").unwrap()),headers:headers("BILLING_CREATE_HEADER"),..Default::default()}).await.unwrap();
    assert!(matches!(response,BillingPlanCreatePlanResponses::Status200(_)));
    let response=client.billing_portal_get_billing_portal_session(BillingBillingPortalGetBillingPortalSessionRequest {params:BTreeMap::from([("id".into(),std::env::var("BILLING_PORTAL_ID").unwrap())]),headers:headers("BILLING_PORTAL_HEADER"),..Default::default()}).await.unwrap();
    assert!(matches!(response,BillingBillingPortalGetBillingPortalSessionResponses::Status200(_)));
    if let Ok(id)=std::env::var("BILLING_WEBHOOK_PORTAL_ID") {
        let response=client.billing_portal_get_billing_portal_session(BillingBillingPortalGetBillingPortalSessionRequest {params:BTreeMap::from([("id".into(),id)]),headers:headers("BILLING_PORTAL_HEADER"),..Default::default()}).await.unwrap();
        assert!(matches!(response,BillingBillingPortalGetBillingPortalSessionResponses::Status200(_)));
    }
    let response=client.plan_get_plan(BillingPlanGetPlanRequest {params:BTreeMap::from([("id".into(),id)]),..Default::default()}).await.unwrap();assert!(matches!(response,BillingPlanGetPlanResponses::Error{status:401,..}));
}
