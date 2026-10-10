//! Base SMS persistence. Like the TypeScript base provider, this implementation
//! marks records sent without dispatching to an external carrier.
#![forbid(unsafe_code)]
#[cfg(feature = "postgres")]
pub mod postgres;
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use forklaunch_messaging_interface::{SendSmsDto, SmsRecordDto, SmsService, SmsStatus, SmsStore};
use std::{collections::BTreeMap, sync::Mutex};

#[must_use]
pub fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[must_use]
pub fn new_record(request: SendSmsDto) -> SmsRecordDto {
    let now = timestamp();
    SmsRecordDto {
        id: uuid::Uuid::new_v4().to_string(),
        to: request.to,
        body: request.body,
        metadata: request.metadata,
        status: SmsStatus::Queued,
        provider_message_id: None,
        error: None,
        created_at: Some(now.clone()),
        updated_at: Some(now),
    }
}

#[must_use]
pub fn not_found() -> BlueprintError {
    BlueprintError::new("sms_not_found", "SMS record not found")
}

pub struct BaseSmsService<S> {
    store: S,
}
impl<S: SmsStore> BaseSmsService<S> {
    pub const fn new(store: S) -> Self {
        Self { store }
    }
}
impl<S: SmsStore> SmsService for BaseSmsService<S> {
    async fn send_sms(&self, request: SendSmsDto) -> BlueprintResult<SmsRecordDto> {
        let mut record = new_record(request);
        record.status = SmsStatus::Sent;
        self.store.insert(record).await
    }
    async fn get_sms_record(&self, id: &str) -> BlueprintResult<SmsRecordDto> {
        self.store.get(id).await
    }
}

/// Process-local test/development store. Production uses the encrypted, tenant-
/// bound Postgres store; this store intentionally makes no durability claim.
#[derive(Default)]
pub struct InMemorySmsStore {
    records: Mutex<BTreeMap<String, SmsRecordDto>>,
}
fn lock_error() -> BlueprintError {
    BlueprintError::new("sms_storage_error", "SMS storage operation failed")
}
impl SmsStore for InMemorySmsStore {
    async fn insert(&self, record: SmsRecordDto) -> BlueprintResult<SmsRecordDto> {
        let mut records = self.records.lock().map_err(|_| lock_error())?;
        if records.contains_key(&record.id)
            || record.provider_message_id.as_ref().is_some_and(|id| {
                records
                    .values()
                    .any(|r| r.provider_message_id.as_ref() == Some(id))
            })
        {
            return Err(BlueprintError::new(
                "sms_duplicate",
                "SMS record already exists",
            ));
        }
        records.insert(record.id.clone(), record.clone());
        Ok(record)
    }
    async fn get(&self, id: &str) -> BlueprintResult<SmsRecordDto> {
        self.records
            .lock()
            .map_err(|_| lock_error())?
            .get(id)
            .cloned()
            .ok_or_else(not_found)
    }
    async fn update_delivery(
        &self,
        provider_id: &str,
        status: SmsStatus,
        error: Option<String>,
    ) -> BlueprintResult<SmsRecordDto> {
        let mut records = self.records.lock().map_err(|_| lock_error())?;
        let record = records
            .values_mut()
            .find(|r| r.provider_message_id.as_deref() == Some(provider_id))
            .ok_or_else(not_found)?;
        record.status = status;
        record.error = error;
        record.updated_at = Some(timestamp());
        Ok(record.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn base_matches_typescript_and_missing_rows_fail() {
        let service = BaseSmsService::new(InMemorySmsStore::default());
        let request = SendSmsDto {
            to: String::new(),
            body: "Unicode 💬".into(),
            metadata: Some(serde_json::from_str(r#"{"x":[1,null,true]}"#).unwrap()),
        };
        let record = service.send_sms(request.clone()).await.unwrap();
        assert_eq!(record.status, SmsStatus::Sent);
        assert_eq!(record.to, request.to);
        assert_eq!(record.body, request.body);
        assert_eq!(record.metadata, request.metadata);
        assert_eq!(service.get_sms_record(&record.id).await.unwrap(), record);
        assert_eq!(
            service.get_sms_record("missing").await.unwrap_err().code,
            "sms_not_found"
        );
        let json = serde_json::to_value(record).unwrap();
        assert!(json.get("providerMessageId").is_none());
        assert!(json.get("error").is_none());
        assert!(
            json.get("createdAt")
                .unwrap()
                .as_str()
                .unwrap()
                .ends_with('Z')
        );
    }
}
