use cac_sdk::*;
use std::collections::BTreeMap;
fn headers(name: &str) -> BTreeMap<String, String> {
    BTreeMap::from([("authorization".to_owned(), std::env::var(name).unwrap())])
}
#[tokio::test]
async fn generated_client_calls_authenticated_cac_routes() {
    let client = CacClient::new(std::env::var("CAC_BASE_URL").unwrap());
    let response = client.clone().with_hmac_authorization(std::env::var("CAC_DESCRIPTOR_HEADER").unwrap())
        .code_set_describe_code_set(Default::default()).await.unwrap();
    let CacCodeSetDescribeCodeSetResponses::Status200(value) = response else { panic!("descriptor failed"); };
    assert_eq!(value.code_set_type, "mock"); assert!(!value.licensed);
    let response = client.code_set_lookup_procedure_code(CacCodeSetLookupProcedureCodeRequest {
        params: BTreeMap::from([("code".to_owned(), "PROC-002".to_owned())]),
        headers: headers("CAC_LOOKUP_HEADER"), ..Default::default()
    }).await.unwrap();
    let CacCodeSetLookupProcedureCodeResponses::Status200(value) = response else { panic!("lookup failed"); };
    assert_eq!(value.description, "Annual Physical Exam");
    let response = client.code_set_lookup_procedure_code(CacCodeSetLookupProcedureCodeRequest {
        params: BTreeMap::from([("code".to_owned(), "unknown".to_owned())]),
        headers: headers("CAC_UNKNOWN_HEADER"), ..Default::default()
    }).await.unwrap();
    let CacCodeSetLookupProcedureCodeResponses::Status404(value) = response else { panic!("unknown response failed"); };
    assert_eq!(value, "Procedure code 'unknown' not found");
    let response = client.compliance_export_user_data(CacComplianceExportUserDataRequest {
        params: BTreeMap::from([("userId".to_owned(), "user".to_owned())]),
        headers: headers("CAC_JWT_HEADER"), ..Default::default()
    }).await.unwrap();
    let CacComplianceExportUserDataResponses::Status404(value) = response else { panic!("empty export failed"); };
    assert_eq!(value, "User not found or no PII data to export");
    let response = client.code_set_describe_code_set(Default::default()).await.unwrap();
    assert!(matches!(response, CacCodeSetDescribeCodeSetResponses::Error { status: 401, .. }));
}
