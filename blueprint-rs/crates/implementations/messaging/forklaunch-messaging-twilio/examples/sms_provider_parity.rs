use forklaunch_messaging_base::{BaseSmsService, InMemorySmsStore};
use forklaunch_messaging_interface::{SendSmsDto, SmsService};
use forklaunch_messaging_twilio::{
    TwilioSmsService, TwilioStatusCallbackDto, map_twilio_message_status, signature,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Input {
    statuses: Vec<String>,
    requests: Vec<SendSmsDto>,
    signatures: Vec<SignatureInput>,
}
#[derive(Deserialize)]
struct SignatureInput {
    token: String,
    url: String,
    params: Vec<(String, String)>,
}
fn normalize(record: impl serde::Serialize) -> Value {
    let mut value = serde_json::to_value(record).unwrap();
    for field in ["id", "createdAt", "updatedAt"] {
        value.as_object_mut().unwrap().remove(field);
    }
    value
}
#[tokio::main]
async fn main() {
    let input: Input = serde_json::from_reader(std::io::stdin()).unwrap();
    let service = TwilioSmsService::new(
        InMemorySmsStore::default(),
        "ACfixture".into(),
        "fixture-secret-only".into(),
        "+15550000000".into(),
    )
    .unwrap()
    .with_api_base(&std::env::var("TWILIO_FIXTURE_URL").unwrap())
    .unwrap();
    let base = BaseSmsService::new(InMemorySmsStore::default());
    let mut base_results = Vec::new();
    let mut sends = Vec::new();
    let mut callbacks = Vec::new();
    let mut errors = Vec::new();
    for request in input.requests {
        base_results.push(normalize(base.send_sms(request.clone()).await.unwrap()));
        match service.send_sms(request).await {
            Ok(record) => {
                let id = record.id.clone();
                sends.push(normalize(record.clone()));
                assert_eq!(service.get_sms_record(&id).await.unwrap(), record);
                if let Some(sid) = record.provider_message_id {
                    for (index, status) in input.statuses.iter().enumerate() {
                        let updated = service
                            .process_status_callback(TwilioStatusCallbackDto {
                                message_sid: sid.clone(),
                                message_status: status.clone(),
                                error_message: if index == 0 {
                                    Some("delivery error".into())
                                } else {
                                    None
                                },
                            })
                            .await
                            .unwrap();
                        callbacks.push(normalize(updated));
                    }
                }
            }
            Err(error) => errors.push(error.code),
        }
    }
    assert_eq!(
        service.get_sms_record("missing").await.unwrap_err().code,
        "sms_not_found"
    );
    assert_eq!(
        service
            .process_status_callback(TwilioStatusCallbackDto {
                message_sid: "missing".into(),
                message_status: "delivered".into(),
                error_message: None
            })
            .await
            .unwrap_err()
            .code,
        "sms_not_found"
    );
    let signatures: Vec<_> = input
        .signatures
        .into_iter()
        .map(|s| signature::expected_signature(&s.token, &s.url, &s.params).unwrap())
        .collect();
    let mapped: Vec<_> = input
        .statuses
        .iter()
        .map(|s| map_twilio_message_status(s))
        .collect();
    println!(
        "{}",
        json!({"base":base_results,"sends":sends,"callbacks":callbacks,"errors":errors,"mapped":mapped,"signatures":signatures})
    );
}
