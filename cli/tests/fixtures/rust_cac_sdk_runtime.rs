use std::collections::BTreeMap;

use cac_sdk::*;
fn headers(name: &str) -> BTreeMap<String, String> {
    BTreeMap::from([("authorization".to_owned(), std::env::var(name).unwrap())])
}
#[tokio::test]
async fn generated_client_calls_authenticated_cac_routes() {
    let client = Client::new(std::env::var("CAC_BASE_URL").unwrap())
        .with_plaintext_development_transport();
    let response = client
        .clone()
        .with_hmac_authorization(std::env::var("CAC_DESCRIPTOR_HEADER").unwrap())
        .describe_code_set_with_options(Default::default())
        .await
        .unwrap();
    let DescribeCodeSetResponses::Status200(value) = response else {
        panic!("descriptor failed");
    };
    assert_eq!(value.code_set_type.as_str(), "mock");
    assert!(!value.licensed);
    let response = client
        .lookup_procedure_code(LookupProcedureCodeRequest {
            params: LookupProcedureCodeParams {
                code: "PROC-002".into(),
            },
            headers: headers("CAC_LOOKUP_HEADER"),
            ..Default::default()
        })
        .await
        .unwrap();
    let LookupProcedureCodeResponses::Status200(value) = response else {
        panic!("lookup failed");
    };
    assert_eq!(value.description, "Annual Physical Exam");
    let response = client
        .lookup_procedure_code(LookupProcedureCodeRequest {
            params: LookupProcedureCodeParams {
                code: "unknown".into(),
            },
            headers: headers("CAC_UNKNOWN_HEADER"),
            ..Default::default()
        })
        .await
        .unwrap();
    let LookupProcedureCodeResponses::Status404(value) = response else {
        panic!("unknown response failed");
    };
    assert_eq!(value, "Procedure code 'unknown' not found");
    let response = client
        .export_user_data(ExportUserDataRequest {
            params: ExportUserDataParams {
                user_id: "user".into(),
            },
            headers: headers("CAC_JWT_HEADER"),
            ..Default::default()
        })
        .await
        .unwrap();
    let ExportUserDataResponses::Status404(value) = response else {
        panic!("empty export failed");
    };
    assert_eq!(value, "User not found or no PII data to export");
    let error = client
        .describe_code_set_with_options(Default::default())
        .await
        .unwrap_err();
    // CAC's authored descriptor route declares only 200. The HTTP probe checks
    // the exact unauthorized 401; the typed client rejects undeclared statuses.
    assert!(matches!(
        error,
        SdkError::Contract("undeclared response status or content type")
    ));
}
