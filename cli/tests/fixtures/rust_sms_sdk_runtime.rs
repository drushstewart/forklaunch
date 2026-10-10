use std::collections::BTreeMap;
use twilio_sdk::*;
fn headers(name:&str)->BTreeMap<String,String>{BTreeMap::from([("authorization".into(),std::env::var(name).unwrap())])}
#[tokio::test]
async fn generated_rust_sdk_calls_sms_and_url_encoded_callbacks() {
    let client=TwilioClient::new(std::env::var("SMS_BASE_URL").unwrap());
    let id=std::env::var("SMS_RECORD_ID").unwrap();
    let fetched=client.sms_get_sms_record(TwilioSmsGetSmsRecordRequest {params:BTreeMap::from([("id".into(),id.clone())]),headers:headers("SMS_GET_HEADER"),..Default::default()}).await.unwrap();
    let TwilioSmsGetSmsRecordResponses::Status200(record)=fetched else {panic!("get failed")};
    assert_eq!(record.id,id);assert_eq!(record.status,"delivered");
    let missing=client.sms_get_sms_record(TwilioSmsGetSmsRecordRequest {params:BTreeMap::from([("id".into(),"missing".into())]),headers:headers("SMS_MISSING_HEADER"),..Default::default()}).await.unwrap();
    assert!(matches!(missing,TwilioSmsGetSmsRecordResponses::Status404(_)));
    let callback=client.webhook_handle_status_callback(TwilioWebhookHandleStatusCallbackRequest {body:Some(std::env::var("SMS_CALLBACK_FORM").unwrap()),headers:BTreeMap::from([("x-twilio-signature".into(),std::env::var("SMS_CALLBACK_SIGNATURE").unwrap())]),..Default::default()}).await.unwrap();
    let TwilioWebhookHandleStatusCallbackResponses::Status200(text)=callback else {panic!("callback failed")};assert_eq!(text,"Status callback processed");
    let sent=client.sms_send_sms(TwilioSmsSendSmsRequest {body:Some(std::env::var("SMS_SEND_BODY").unwrap()),headers:headers("SMS_SEND_HEADER"),..Default::default()}).await.unwrap();
    let TwilioSmsSendSmsResponses::Status200(record)=sent else {panic!("send failed")};assert_eq!(record.status,"queued");assert_eq!(record.body,"private SMS from Rust SDK");
    let denied=client.sms_get_sms_record(TwilioSmsGetSmsRecordRequest {params:BTreeMap::from([("id".into(),id)]),..Default::default()}).await.unwrap();
    assert!(matches!(denied,TwilioSmsGetSmsRecordResponses::Error{status:401,..}));
}
