//! Tenant-bound `PostgreSQL` records. Order addresses are encrypted with the
//! tenant key; plaintext address reads and their audit inserts commit together.
use crate::store::{
    EcommerceStore, Mutation, MutationResult, Query, Record, RecordKind, Selector, apply_mutation,
    not_found, storage_error,
};
use forklaunch_blueprint_core::{AuditContext, BlueprintError, BlueprintResult};
use forklaunch_core::persistence::{
    TenantContext, begin_tenant_transaction, encryption::FieldEncryptor,
};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction};
use std::sync::Arc;

#[derive(Clone)]
pub struct PostgresEcommerceStore {
    pool: PgPool,
    tenant: TenantContext,
    encryptor: Arc<FieldEncryptor>,
    audit: AuditContext,
}
impl PostgresEcommerceStore {
    /// Uses a schema-private migration history compatible with IAM, messaging
    /// and worker migrations in the same database.
    /// # Errors
    /// Returns sanitized migration/locking errors.
    pub async fn migrate(pool: &PgPool) -> BlueprintResult<()> {
        let mut tx = pool.begin().await.map_err(storage_error)?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(0x464C_4543_4F01_i64)
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        sqlx::query("CREATE SCHEMA IF NOT EXISTS fl_ecommerce")
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        sqlx::query("SET LOCAL search_path=fl_ecommerce,pg_catalog")
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        let mut migrator = sqlx::migrate!("./migrations");
        migrator.set_locking(false);
        migrator.run_direct(&mut *tx).await.map_err(storage_error)?;
        tx.commit().await.map_err(storage_error)
    }
    /// # Errors
    /// Rejects bypass-RLS roles and empty audit correlation IDs.
    pub async fn new(
        pool: PgPool,
        tenant: TenantContext,
        encryptor: Arc<FieldEncryptor>,
        audit: AuditContext,
    ) -> BlueprintResult<Self> {
        validate_audit(&audit)?;
        let (bypass,): (bool,) = sqlx::query_as(
            "SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname=current_user",
        )
        .fetch_one(&pool)
        .await
        .map_err(storage_error)?;
        if bypass {
            return Err(BlueprintError::new(
                "ecommerce_unsafe_database_role",
                "Ecommerce runtime requires a role without SUPERUSER or BYPASSRLS",
            ));
        }
        Ok(Self {
            pool,
            tenant,
            encryptor,
            audit,
        })
    }
    /// Reuses server-owned tenant and key configuration with a verified actor.
    /// # Errors
    /// Rejects empty audit correlation IDs.
    pub fn with_audit_context(&self, audit: AuditContext) -> BlueprintResult<Self> {
        validate_audit(&audit)?;
        Ok(Self {
            audit,
            ..self.clone()
        })
    }
    async fn transaction(&self) -> BlueprintResult<Transaction<'_, Postgres>> {
        begin_tenant_transaction(&self.pool, &self.tenant)
            .await
            .map_err(storage_error)
    }
    fn encode(&self, record: &Record) -> BlueprintResult<Value> {
        let mut data = record.data()?;
        // Check the serialized shape before storing it (including non-finite
        // native floating-point input in image positions).
        Record::from_data(record.kind(), data.clone())?;
        if record.kind() == RecordKind::Order {
            let envelope =
                json!({"recordId":record.id(),"shippingAddress":data["shippingAddress"]});
            let ciphertext = self
                .encryptor
                .seal(
                    &envelope.to_string(),
                    self.tenant.id(),
                    "[Shipping address]",
                )
                .map_err(storage_error)?;
            data["shippingAddress"] = Value::String(ciphertext);
        }
        Ok(data)
    }
    async fn decode(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        kind: RecordKind,
        id: &str,
        mut data: Value,
        operation: &str,
    ) -> BlueprintResult<Record> {
        if data["id"].as_str() != Some(id) {
            return Err(storage_error("record identity mismatch"));
        }
        if kind == RecordKind::Order {
            let ciphertext = data["shippingAddress"]
                .as_str()
                .ok_or_else(|| storage_error("unencrypted address"))?;
            let plaintext = self
                .encryptor
                .decrypt(ciphertext, self.tenant.id())
                .map_err(storage_error)?;
            let envelope: Value = serde_json::from_str(&plaintext).map_err(storage_error)?;
            if envelope["recordId"].as_str() != Some(id) {
                return Err(storage_error("address identity mismatch"));
            }
            data["shippingAddress"] = envelope["shippingAddress"].clone();
            sqlx::query("INSERT INTO fl_ecommerce.pii_read_audit(id,tenant_id,record_id,actor_id,correlation_id,operation) VALUES($1::uuid,$2,$3,$4,$5,$6)")
                .bind(uuid::Uuid::new_v4().to_string()).bind(self.tenant.id()).bind(id).bind(&self.audit.actor_id)
                .bind(&self.audit.correlation_id).bind(operation).execute(&mut **tx).await.map_err(storage_error)?;
        }
        Record::from_data(kind, data)
    }
}
fn validate_audit(audit: &AuditContext) -> BlueprintResult<()> {
    if audit.correlation_id.trim().is_empty() {
        return Err(BlueprintError::new(
            "ecommerce_invalid_audit",
            "An audit correlation ID is required",
        ));
    }
    Ok(())
}
fn database_error(error: sqlx::Error) -> BlueprintError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        BlueprintError::new("ecommerce_conflict", "Ecommerce unique key already exists")
    } else {
        storage_error(error)
    }
}
impl EcommerceStore for PostgresEcommerceStore {
    async fn insert(&self, record: Record) -> BlueprintResult<Record> {
        let data = self.encode(&record)?;
        let mut tx = self.transaction().await?;
        sqlx::query("INSERT INTO fl_ecommerce.records(tenant_id,kind,id,data) VALUES($1,$2,$3,$4)")
            .bind(self.tenant.id())
            .bind(record.kind().as_str())
            .bind(record.id())
            .bind(sqlx::types::Json(data))
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        tx.commit().await.map_err(storage_error)?;
        Ok(record)
    }
    async fn find(&self, query: Query) -> BlueprintResult<Vec<Record>> {
        let mut tx = self.transaction().await?;
        let (field, value) = query.equals.map_or((None, None), |(field, value)| {
            (Some(field.as_str()), Some(value))
        });
        let rows:Vec<(String,sqlx::types::Json<Value>)>=sqlx::query_as(
            "SELECT id,data FROM fl_ecommerce.records WHERE tenant_id=$1 AND kind=$2 AND ($3::text[] IS NULL OR cardinality($3)=0 OR id=ANY($3)) AND ($4::text IS NULL OR data->>$4::text=$5::text) AND ($6::text IS NULL OR $6='' OR data->>'title' ILIKE '%'||$6||'%') ORDER BY id")
            .bind(self.tenant.id()).bind(query.kind.as_str()).bind(query.ids).bind(field).bind(value).bind(query.title)
            .fetch_all(&mut *tx).await.map_err(storage_error)?;
        let mut records = Vec::with_capacity(rows.len());
        for (id, data) in rows {
            records.push(
                self.decode(&mut tx, query.kind, &id, data.0, "read")
                    .await?,
            );
        }
        tx.commit().await.map_err(storage_error)?;
        Ok(records)
    }
    async fn mutate(
        &self,
        selector: Selector,
        mutation: Mutation,
    ) -> BlueprintResult<MutationResult> {
        selector.validate_unique()?;
        let mut tx = self.transaction().await?;
        let row:Option<(String,sqlx::types::Json<Value>)>=sqlx::query_as(
            "SELECT id,data FROM fl_ecommerce.records WHERE tenant_id=$1 AND kind=$2 AND data->>$3::text=$4 LIMIT 1 FOR UPDATE")
            .bind(self.tenant.id()).bind(selector.kind.as_str()).bind(selector.field.as_str()).bind(selector.value)
            .fetch_optional(&mut *tx).await.map_err(storage_error)?;
        let (id, data) = row.ok_or_else(|| not_found(selector.kind))?;
        let record = self
            .decode(&mut tx, selector.kind, &id, data.0, "mutate")
            .await?;
        let result = apply_mutation(record, mutation)?;
        if result.changed {
            let data = self.encode(&result.record)?;
            sqlx::query(
                "UPDATE fl_ecommerce.records SET data=$4 WHERE tenant_id=$1 AND kind=$2 AND id=$3",
            )
            .bind(self.tenant.id())
            .bind(selector.kind.as_str())
            .bind(id)
            .bind(sqlx::types::Json(data))
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        }
        tx.commit().await.map_err(storage_error)?;
        Ok(result)
    }
    async fn delete(&self, selector: Selector) -> BlueprintResult<()> {
        let mut tx = self.transaction().await?;
        sqlx::query("DELETE FROM fl_ecommerce.records WHERE tenant_id=$1 AND kind=$2 AND data->>$3::text=$4")
            .bind(self.tenant.id()).bind(selector.kind.as_str()).bind(selector.field.as_str()).bind(selector.value)
            .execute(&mut *tx).await.map_err(storage_error)?;
        tx.commit().await.map_err(storage_error)
    }
}
