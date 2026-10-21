// Exercise actual generated module exports; raw body calls preserve signed fixture bytes.
use std::collections::BTreeMap;

use twilio_sdk::*;
fn headers(name: &str) -> BTreeMap<String, String> {
    BTreeMap::from([("authorization".into(), std::env::var(name).unwrap())])
}
#[tokio::test]
async fn generated_rust_sdk_calls_sms_and_url_encoded_callbacks() {
    let client = Client::new(std::env::var("SMS_BASE_URL").unwrap());
    let id = std::env::var("SMS_RECORD_ID").unwrap();
    let fetched = client
        .get_sms_record(GetSmsRecordRequest {
            params: GetSmsRecordParams { id: id.clone() },
            headers: headers("SMS_GET_HEADER"),
            ..Default::default()
        })
        .await
        .unwrap();
    let GetSmsRecordResponses::Status200(record) = fetched else {
        panic!("get failed")
    };
    assert_eq!(record.id, id);
    assert_eq!(record.status, "delivered");
    let missing = client
        .get_sms_record(GetSmsRecordRequest {
            params: GetSmsRecordParams {
                id: "missing".into(),
            },
            headers: headers("SMS_MISSING_HEADER"),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(matches!(missing, GetSmsRecordResponses::Status404(_)));
    let callback = client
        .handle_status_callback_raw(HandleStatusCallbackRequest {
            body: Some(std::env::var("SMS_CALLBACK_FORM").unwrap()),
            headers: BTreeMap::from([(
                "x-twilio-signature".into(),
                std::env::var("SMS_CALLBACK_SIGNATURE").unwrap(),
            )]),
            ..Default::default()
        })
        .await
        .unwrap();
    let HandleStatusCallbackResponses::Status200(text) = callback else {
        panic!("callback failed")
    };
    assert_eq!(text, "Status callback processed");
    let typed_callback = client
        .handle_status_callback(HandleStatusCallbackRequest {
            body: Some(HandleStatusCallbackRequestBody {
                message_sid: std::env::var("SMS_CALLBACK_SID").unwrap(),
                message_status: "delivered".into(),
                additional_properties: BTreeMap::from([
                    ("Extra".into(), serde_json::json!("💬 + = &")),
                    ("Repeated".into(), serde_json::json!(["z", "a", "a"])),
                ]),
                ..Default::default()
            }),
            headers: BTreeMap::from([(
                "x-twilio-signature".into(),
                std::env::var("SMS_TYPED_CALLBACK_SIGNATURE").unwrap(),
            )]),
            ..Default::default()
        })
        .await
        .unwrap();
    let HandleStatusCallbackResponses::Status200(text) = typed_callback else {
        panic!("typed callback failed")
    };
    assert_eq!(text, "Status callback processed");
    let sent = client
        .send_sms_raw(SendSmsRequest {
            body: Some(std::env::var("SMS_SEND_BODY").unwrap()),
            headers: headers("SMS_SEND_HEADER"),
            ..Default::default()
        })
        .await
        .unwrap();
    let SendSmsResponses::Status200(record) = sent else {
        panic!("send failed")
    };
    assert_eq!(record.status, "queued");
    assert_eq!(record.body, "private SMS from Rust SDK");
    let denied = client
        .get_sms_record(GetSmsRecordRequest {
            params: GetSmsRecordParams { id },
            ..Default::default()
        })
        .await
        .unwrap_err();
    // The HTTP probe checks the exact 401; this route declares no 401 response.
    assert!(matches!(
        denied,
        SdkError::Contract("undeclared response status or content type")
    ));
}
