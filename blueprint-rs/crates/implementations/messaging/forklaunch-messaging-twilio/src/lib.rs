//! Twilio Messages REST provider. Dispatch is single-attempt: a transport or
//! persistence failure may follow carrier acceptance and must not trigger blind
//! retries. Callback verification uses a configured URL, never forwarded hosts.
#![forbid(unsafe_code)]
pub mod signature;
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use forklaunch_messaging_base::new_record;
use forklaunch_messaging_interface::{SendSmsDto, SmsRecordDto, SmsService, SmsStatus, SmsStore};
use reqwest::{Client, Url, redirect::Policy};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use zeroize::Zeroizing;

#[must_use]
pub fn map_twilio_message_status(status: &str) -> SmsStatus {
    match status {
        "sent" => SmsStatus::Sent,
        "delivered" | "read" => SmsStatus::Delivered,
        "undelivered" => SmsStatus::Undelivered,
        "failed" | "canceled" => SmsStatus::Failed,
        _ => SmsStatus::Queued,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TwilioStatusCallbackDto {
    #[serde(rename = "MessageSid")]
    pub message_sid: String,
    #[serde(rename = "MessageStatus")]
    pub message_status: String,
    #[serde(
        rename = "ErrorMessage",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub error_message: Option<String>,
}
#[derive(Deserialize)]
struct TwilioMessageResource {
    sid: String,
    status: String,
    error_message: Option<String>,
}

fn config_error() -> BlueprintError {
    BlueprintError::new("twilio_invalid_config", "Invalid Twilio configuration")
}
fn provider_error() -> BlueprintError {
    BlueprintError::new(
        "twilio_provider_error",
        "Twilio request failed; delivery may have occurred",
    )
}

/// Credentials deliberately do not implement `Debug`. TLS is required except
/// for explicit loopback provider fixtures. Redirects are never followed.
pub struct TwilioSmsService<S> {
    store: S,
    client: Client,
    endpoint: Url,
    account_sid: String,
    auth_token: Zeroizing<String>,
    from_number: String,
    callback_url: Option<String>,
}
impl<S: SmsStore> TwilioSmsService<S> {
    /// # Errors
    /// Rejects empty credentials, unsafe account IDs, and HTTP client failures.
    pub fn new(
        store: S,
        account_sid: String,
        auth_token: String,
        from_number: String,
    ) -> BlueprintResult<Self> {
        if account_sid.is_empty()
            || account_sid.len() > 128
            || !account_sid.bytes().all(|c| c.is_ascii_alphanumeric())
            || auth_token.is_empty()
            || from_number.is_empty()
        {
            return Err(config_error());
        }
        let client = Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| config_error())?;
        Ok(Self {
            store,
            client,
            endpoint: Url::parse(&format!(
                "https://api.twilio.com/2010-04-01/Accounts/{account_sid}/Messages.json"
            ))
            .map_err(|_| config_error())?,
            account_sid,
            auth_token: Zeroizing::new(auth_token),
            from_number,
            callback_url: None,
        })
    }
    #[must_use]
    pub const fn store(&self) -> &S {
        &self.store
    }
    /// Rebinds persistence/auditing while reusing the connection pool and trusted
    /// provider configuration; no credentials are taken from a request.
    pub fn with_store<T: SmsStore>(&self, store: T) -> TwilioSmsService<T> {
        TwilioSmsService {
            store,
            client: self.client.clone(),
            endpoint: self.endpoint.clone(),
            account_sid: self.account_sid.clone(),
            auth_token: self.auth_token.clone(),
            from_number: self.from_number.clone(),
            callback_url: self.callback_url.clone(),
        }
    }
    /// Overrides the API origin for a TLS proxy or explicit local fixture.
    /// # Errors
    /// Rejects non-TLS remote origins, credentials, queries, and fragments.
    pub fn with_api_base(mut self, base: &str) -> BlueprintResult<Self> {
        let mut url = Url::parse(base).map_err(|_| config_error())?;
        let loopback = url.host_str().is_some_and(|h| {
            h == "localhost"
                || h.parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(config_error());
        }
        url.set_path(&format!(
            "{}/Accounts/{}/Messages.json",
            url.path().trim_end_matches('/'),
            self.account_sid
        ));
        self.endpoint = url;
        Ok(self)
    }
    /// Sends this exact trusted external URL in `StatusCallback`. Preserve its
    /// original spelling for signature validation. No URL is inferred from HTTP.
    /// # Errors
    /// Rejects non-HTTPS URLs, embedded credentials, and fragments.
    pub fn with_status_callback(mut self, external_url: String) -> BlueprintResult<Self> {
        signature::validate_callback_url(&external_url)?;
        self.callback_url = Some(external_url);
        Ok(self)
    }
    /// Verifies all form parameters before parsing the callback's known fields.
    /// Multiple values for a required field are rejected as ambiguous.
    /// # Errors
    /// Returns an authentication/input/storage error. Unknown SIDs return not found.
    pub async fn process_signed_callback(
        &self,
        params: &[(String, String)],
        signature: &str,
    ) -> BlueprintResult<SmsRecordDto> {
        self.process_status_callback(self.verify_callback(params, signature)?)
            .await
    }
    /// Authenticates all callback fields without reading or changing persistence.
    /// # Errors
    /// Rejects invalid signatures, missing fields and ambiguous fields.
    pub fn verify_callback(
        &self,
        params: &[(String, String)],
        signature: &str,
    ) -> BlueprintResult<TwilioStatusCallbackDto> {
        let url = self.callback_url.as_deref().ok_or_else(config_error)?;
        signature::verify(&self.auth_token, url, params, signature)?;
        let field = |name: &str| -> BlueprintResult<Option<String>> {
            let mut values = params
                .iter()
                .filter(|(key, _)| key == name)
                .map(|(_, value)| value.clone());
            let value = values.next();
            if values.next().is_some() {
                return Err(BlueprintError::new(
                    "twilio_invalid_callback",
                    "Ambiguous callback field",
                ));
            }
            Ok(value)
        };
        Ok(TwilioStatusCallbackDto {
            message_sid: field("MessageSid")?.ok_or_else(|| {
                BlueprintError::new("twilio_invalid_callback", "Missing callback field")
            })?,
            message_status: field("MessageStatus")?.ok_or_else(|| {
                BlueprintError::new("twilio_invalid_callback", "Missing callback field")
            })?,
            error_message: field("ErrorMessage")?,
        })
    }
    /// Trusted in-process callback hook, equivalent to the TS service method.
    /// HTTP callers must use `process_signed_callback` instead.
    /// # Errors
    /// Returns a storage error or `sms_not_found`.
    pub async fn process_status_callback(
        &self,
        callback: TwilioStatusCallbackDto,
    ) -> BlueprintResult<SmsRecordDto> {
        self.store
            .update_delivery(
                &callback.message_sid,
                map_twilio_message_status(&callback.message_status),
                callback.error_message,
            )
            .await
    }
}
impl<S: SmsStore> SmsService for TwilioSmsService<S> {
    async fn send_sms(&self, request: SendSmsDto) -> BlueprintResult<SmsRecordDto> {
        let mut record = new_record(request);
        let mut form = vec![
            ("To", record.to.as_str()),
            ("From", self.from_number.as_str()),
            ("Body", record.body.as_str()),
        ];
        if let Some(url) = self.callback_url.as_deref() {
            form.push(("StatusCallback", url));
        }
        let mut response = self
            .client
            .post(self.endpoint.clone())
            .basic_auth(&self.account_sid, Some(self.auth_token.as_str()))
            .form(&form)
            .send()
            .await
            .map_err(|_| provider_error())?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| provider_error())? {
            if bytes.len().saturating_add(chunk.len()) > 64 * 1024 {
                return Err(provider_error());
            }
            bytes.extend_from_slice(&chunk);
        }
        if status.is_success() {
            let message: TwilioMessageResource =
                serde_json::from_slice(&bytes).map_err(|_| provider_error())?;
            record.provider_message_id = Some(message.sid);
            record.status = map_twilio_message_status(&message.status);
            record.error = message.error_message;
        } else {
            record.status = SmsStatus::Failed;
            record.error = Some(format!(
                "Twilio API error ({}): {}",
                status.as_u16(),
                String::from_utf8_lossy(&bytes)
            ));
        }
        self.store.insert(record).await
    }
    async fn get_sms_record(&self, id: &str) -> BlueprintResult<SmsRecordDto> {
        self.store.get(id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forklaunch_messaging_base::InMemorySmsStore;
    const URL: &str = "https://sms.example.com/webhook?source=fixture";
    fn provider() -> TwilioSmsService<InMemorySmsStore> {
        TwilioSmsService::new(
            InMemorySmsStore::default(),
            "ACfixture".into(),
            "test-token-only".into(),
            "+15550000000".into(),
        )
        .unwrap()
        .with_status_callback(URL.into())
        .unwrap()
    }
    #[tokio::test]
    async fn signed_callback_authenticates_every_field_and_rejects_ambiguity_before_mutation() {
        let provider = provider();
        let mut record = new_record(SendSmsDto {
            to: "+15550000001".into(),
            body: "private".into(),
            metadata: None,
        });
        record.provider_message_id = Some("SMfixture".into());
        provider.store().insert(record.clone()).await.unwrap();
        let params = vec![
            ("MessageSid".into(), "SMfixture".into()),
            ("MessageStatus".into(), "sent".into()),
            ("Unexpected".into(), "must be signed".into()),
        ];
        let signature = signature::expected_signature("test-token-only", URL, &params).unwrap();
        provider
            .process_signed_callback(&params, &signature)
            .await
            .unwrap();
        assert_eq!(
            provider.get_sms_record(&record.id).await.unwrap().status,
            SmsStatus::Sent
        );
        let mut modified = params.clone();
        modified[2].1 = "tampered".into();
        assert!(
            provider
                .process_signed_callback(&modified, &signature)
                .await
                .is_err()
        );
        let mut duplicate = params.clone();
        duplicate.push(("MessageStatus".into(), "failed".into()));
        let signed = signature::expected_signature("test-token-only", URL, &duplicate).unwrap();
        assert_eq!(
            provider
                .process_signed_callback(&duplicate, &signed)
                .await
                .unwrap_err()
                .code,
            "twilio_invalid_callback"
        );
        assert_eq!(
            provider.get_sms_record(&record.id).await.unwrap().status,
            SmsStatus::Sent
        );
        let mut unknown = params.clone();
        unknown[0].1 = "unknown".into();
        let signed = signature::expected_signature("test-token-only", URL, &unknown).unwrap();
        assert_eq!(
            provider
                .process_signed_callback(&unknown, &signed)
                .await
                .unwrap_err()
                .code,
            "sms_not_found"
        );
    }
    #[test]
    fn credentials_and_destination_configuration_fail_closed() {
        assert!(
            TwilioSmsService::new(
                InMemorySmsStore::default(),
                "ACfixture".into(),
                String::new(),
                "+15550000000".into()
            )
            .is_err()
        );
        for base in [
            "http://remote.example",
            "https://user:secret@example.com",
            "https://api.example.com?query=x",
            "https://api.example.com#fragment",
            "file:///tmp/provider",
        ] {
            assert!(provider().with_api_base(base).is_err());
        }
        for url in [
            "http://sms.example.com/webhook",
            "https://user:secret@sms.example.com/webhook",
            "https://sms.example.com/webhook#fragment",
            "",
        ] {
            assert!(provider().with_status_callback(url.into()).is_err());
        }
        assert!(
            provider()
                .with_api_base("http://127.0.0.1:9999/2010-04-01")
                .is_ok()
        );
    }
}
