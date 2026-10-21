//! Worker callback transport: encrypted results and shared replay protection.
use super::NotesRepository;
use fieldnotes_contracts::{
    InternalInsightInput,
    events::insight::{InsightEvent, RESULT_PURPOSE},
};
use fieldnotes_security::SecurityError;
use uuid::Uuid;
impl NotesRepository {
    pub async fn claim_internal_nonce(
        &self,
        nonce: String,
        expiry_ms: i64,
    ) -> Result<bool, SecurityError> {
        // Atomic across replicas. Retain for at least the maximum supported
        // signature window measured on the database clock as well, so a stable
        // API/database clock offset cannot make cleanup expire a nonce early.
        sqlx::query("DELETE FROM fieldnotes_internal_nonces WHERE nonce IN (SELECT nonce FROM fieldnotes_internal_nonces WHERE expires_at < clock_timestamp() LIMIT 100)")
            .execute(&self.pool).await?;
        Ok(sqlx::query("INSERT INTO fieldnotes_internal_nonces(nonce,expires_at) VALUES($1,GREATEST(to_timestamp($2::bigint::double precision/1000),clock_timestamp()+interval '10 minutes')) ON CONFLICT DO NOTHING")
            .bind(nonce).bind(expiry_ms).execute(&self.pool).await?.rows_affected() == 1)
    }
    pub async fn decode_insight(
        &self,
        tenant: Uuid,
        note: Uuid,
        input: &InternalInsightInput,
    ) -> Result<crate::InsightInput, SecurityError> {
        let context = worker::JobContext {
            tenant_id: tenant,
            resource_id: note,
            operation_id: Uuid::parse_str(&input.operation_id)
                .map_err(|_| SecurityError::InvalidInput)?,
            revision: input.revision,
        };
        let channel = worker::EncryptedChannel::<InsightEvent>::new(RESULT_PURPOSE)
            .map_err(|_| SecurityError::InvalidInput)?;
        let event = worker::SealedEvent::from_parts(
            RESULT_PURPOSE,
            context,
            input.encrypted_payload.clone(),
        );
        let audit = fieldnotes_security::ServiceJobAudit::new(
            self.audit_pool.clone(),
            fieldnotes_security::ServiceActor::Notes,
            RESULT_PURPOSE,
            "InsightEvent",
        );
        let payload = channel
            .open_audited(
                self.key.as_keyring(),
                tenant,
                &event,
                "service:notes",
                &audit,
            )
            .await
            .map_err(|error| match error {
                worker::ProcessorError::InvalidEnvelope => SecurityError::InvalidInput,
                _ => SecurityError::Unavailable,
            })?;
        Ok(crate::InsightInput {
            operation_id: payload.operation_id,
            revision: payload.revision,
            excerpt: payload.excerpt.clone(),
            word_count: i64::try_from(payload.word_count)
                .map_err(|_| SecurityError::InvalidInput)?,
        })
    }
}
