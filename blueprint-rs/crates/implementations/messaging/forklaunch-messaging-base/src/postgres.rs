//! Tenant-bound encrypted SMS storage. Apply migrations as a schema owner and
//! run the service with a restricted role. Decryption and its audit insert share
//! a transaction: audit failure prevents plaintext from leaving the store.
use crate::{not_found, timestamp};
use forklaunch_blueprint_core::{AuditContext, BlueprintError, BlueprintResult};
use forklaunch_core::persistence::{
    TenantContext, begin_tenant_transaction, encryption::FieldEncryptor,
};
use forklaunch_messaging_interface::{SmsRecordDto, SmsStatus, SmsStore};
use sqlx::{PgPool, Postgres, Transaction};
use std::sync::Arc;

fn storage_error(_: impl std::fmt::Display) -> BlueprintError {
    BlueprintError::new("sms_storage_error", "SMS storage operation failed")
}

#[derive(Clone)]
pub struct PostgresSmsStore {
    pool: PgPool,
    tenant: TenantContext,
    encryptor: Arc<FieldEncryptor>,
    audit: AuditContext,
}
impl PostgresSmsStore {
    /// Uses an isolated migration namespace so IAM/worker migrations can coexist.
    /// # Errors
    /// Returns a sanitized error if migration locking or execution fails.
    pub async fn migrate(pool: &PgPool) -> BlueprintResult<()> {
        let mut tx = pool.begin().await.map_err(storage_error)?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(0x464C_534D_5301_i64)
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        sqlx::query("CREATE SCHEMA IF NOT EXISTS fl_messaging")
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        sqlx::query("SET LOCAL search_path = fl_messaging, pg_catalog")
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        let mut migrator = sqlx::migrate!("./migrations");
        migrator.set_locking(false);
        migrator.run_direct(&mut *tx).await.map_err(storage_error)?;
        tx.commit().await.map_err(storage_error)
    }

    /// # Errors
    /// Rejects bypass-RLS roles and missing audit correlation IDs.
    pub async fn new(
        pool: PgPool,
        tenant: TenantContext,
        encryptor: Arc<FieldEncryptor>,
        audit: AuditContext,
    ) -> BlueprintResult<Self> {
        if audit.correlation_id.trim().is_empty() {
            return Err(BlueprintError::new(
                "sms_invalid_audit",
                "An audit correlation ID is required",
            ));
        }
        let (bypass,): (bool,) = sqlx::query_as(
            "SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname=current_user",
        )
        .fetch_one(&pool)
        .await
        .map_err(storage_error)?;
        if bypass {
            return Err(BlueprintError::new(
                "sms_unsafe_database_role",
                "SMS runtime requires a role without SUPERUSER or BYPASSRLS",
            ));
        }
        Ok(Self {
            pool,
            tenant,
            encryptor,
            audit,
        })
    }
    /// Reuses trusted tenant/key configuration with a verified request actor.
    /// # Errors
    /// Rejects missing correlation IDs.
    pub fn with_audit_context(&self, audit: AuditContext) -> BlueprintResult<Self> {
        if audit.correlation_id.trim().is_empty() {
            return Err(BlueprintError::new(
                "sms_invalid_audit",
                "An audit correlation ID is required",
            ));
        }
        Ok(Self {
            audit,
            ..self.clone()
        })
    }
    fn seal(&self, record: &SmsRecordDto) -> BlueprintResult<String> {
        self.encryptor
            .seal(
                &serde_json::to_string(record).map_err(storage_error)?,
                self.tenant.id(),
                "[SMS record]",
            )
            .map_err(storage_error)
    }
    fn open(&self, ciphertext: &str, expected_id: &str) -> BlueprintResult<SmsRecordDto> {
        let plaintext = self
            .encryptor
            .decrypt(ciphertext, self.tenant.id())
            .map_err(storage_error)?;
        let record: SmsRecordDto = serde_json::from_str(&plaintext).map_err(storage_error)?;
        if record.id != expected_id {
            return Err(storage_error("record identity mismatch"));
        }
        Ok(record)
    }
    async fn transaction(&self) -> BlueprintResult<Transaction<'_, Postgres>> {
        begin_tenant_transaction(&self.pool, &self.tenant)
            .await
            .map_err(storage_error)
    }
    async fn audit_read(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        id: &str,
        operation: &str,
    ) -> BlueprintResult<()> {
        sqlx::query("INSERT INTO fl_messaging.pii_read_audit(id,tenant_id,record_id,actor_id,correlation_id,operation) VALUES ($1::uuid,$2,$3,$4,$5,$6)")
            .bind(uuid::Uuid::new_v4().to_string()).bind(self.tenant.id()).bind(id).bind(&self.audit.actor_id).bind(&self.audit.correlation_id).bind(operation)
            .execute(&mut **tx).await.map_err(storage_error)?;
        Ok(())
    }
    /// Exports all records linked to a phone number using indexes for every
    /// configured key. This matches the TS module's `SmsRecord: 'to'` selector.
    /// # Errors
    /// Returns a storage/decryption/audit error; no partial plaintext is returned.
    pub async fn export_subject(&self, recipient: &str) -> BlueprintResult<Vec<SmsRecordDto>> {
        let indexes = self
            .encryptor
            .blind_indexes(recipient, self.tenant.id())
            .map_err(storage_error)?;
        let mut tx = self.transaction().await?;
        let rows: Vec<(String,String)> = sqlx::query_as("SELECT id,encrypted_record FROM fl_messaging.sms_records WHERE tenant_id=$1 AND recipient_indexes && $2 ORDER BY id")
            .bind(self.tenant.id()).bind(indexes).fetch_all(&mut *tx).await.map_err(storage_error)?;
        let mut records = Vec::with_capacity(rows.len());
        for (id, ciphertext) in rows {
            let record = self.open(&ciphertext, &id)?;
            self.audit_read(&mut tx, &id, "compliance_export").await?;
            records.push(record);
        }
        tx.commit().await.map_err(storage_error)?;
        Ok(records)
    }
    /// # Errors
    /// Returns a sanitized storage error. Erasure does not decrypt message data.
    pub async fn erase_subject(&self, recipient: &str) -> BlueprintResult<u64> {
        let indexes = self
            .encryptor
            .blind_indexes(recipient, self.tenant.id())
            .map_err(storage_error)?;
        let mut tx = self.transaction().await?;
        let affected = sqlx::query(
            "DELETE FROM fl_messaging.sms_records WHERE tenant_id=$1 AND recipient_indexes && $2",
        )
        .bind(self.tenant.id())
        .bind(indexes)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?
        .rows_affected();
        tx.commit().await.map_err(storage_error)?;
        Ok(affected)
    }
}
impl SmsStore for PostgresSmsStore {
    async fn insert(&self, record: SmsRecordDto) -> BlueprintResult<SmsRecordDto> {
        let ciphertext = self.seal(&record)?;
        let indexes = self
            .encryptor
            .blind_indexes(&record.to, self.tenant.id())
            .map_err(storage_error)?;
        let mut tx = self.transaction().await?;
        sqlx::query("INSERT INTO fl_messaging.sms_records(tenant_id,id,provider_message_id,recipient_indexes,encrypted_record) VALUES($1,$2,$3,$4,$5)")
            .bind(self.tenant.id()).bind(&record.id).bind(&record.provider_message_id).bind(indexes).bind(ciphertext).execute(&mut *tx).await.map_err(storage_error)?;
        tx.commit().await.map_err(storage_error)?;
        Ok(record)
    }
    async fn get(&self, id: &str) -> BlueprintResult<SmsRecordDto> {
        let mut tx = self.transaction().await?;
        let (ciphertext,): (String,) = sqlx::query_as(
            "SELECT encrypted_record FROM fl_messaging.sms_records WHERE tenant_id=$1 AND id=$2",
        )
        .bind(self.tenant.id())
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(storage_error)?
        .ok_or_else(not_found)?;
        let record = self.open(&ciphertext, id)?;
        self.audit_read(&mut tx, id, "get_sms_record").await?;
        tx.commit().await.map_err(storage_error)?;
        Ok(record)
    }
    async fn update_delivery(
        &self,
        provider_id: &str,
        status: SmsStatus,
        error: Option<String>,
    ) -> BlueprintResult<SmsRecordDto> {
        let mut tx = self.transaction().await?;
        let (id,ciphertext): (String,String) = sqlx::query_as("SELECT id,encrypted_record FROM fl_messaging.sms_records WHERE tenant_id=$1 AND provider_message_id=$2 FOR UPDATE")
            .bind(self.tenant.id()).bind(provider_id).fetch_optional(&mut *tx).await.map_err(storage_error)?.ok_or_else(not_found)?;
        let mut record = self.open(&ciphertext, &id)?;
        record.status = status;
        record.error = error;
        record.updated_at = Some(timestamp());
        sqlx::query("UPDATE fl_messaging.sms_records SET encrypted_record=$3,recipient_indexes=$4,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2")
            .bind(self.tenant.id()).bind(&record.id).bind(self.seal(&record)?).bind(self.encryptor.blind_indexes(&record.to,self.tenant.id()).map_err(storage_error)?).execute(&mut *tx).await.map_err(storage_error)?;
        self.audit_read(&mut tx, &record.id, "delivery_callback")
            .await?;
        tx.commit().await.map_err(storage_error)?;
        Ok(record)
    }
}
