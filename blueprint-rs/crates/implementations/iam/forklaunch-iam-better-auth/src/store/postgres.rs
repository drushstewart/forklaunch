//! Encrypted, audited `PostgreSQL` auth storage with forced tenant RLS.
use super::{
    AuthStore, AuthTransaction, Model, conflict, invalid, not_found, record_id, storage_error,
    validate_record,
};
use forklaunch_blueprint_core::{AuditContext, BlueprintError, BlueprintResult};
use forklaunch_core::persistence::{
    TenantContext, begin_tenant_transaction, encryption::FieldEncryptor,
};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction};
use std::sync::Arc;

#[derive(Clone)]
pub struct PostgresAuthStore {
    pool: PgPool,
    tenant: TenantContext,
    encryptor: Arc<FieldEncryptor>,
    audit: AuditContext,
}
impl PostgresAuthStore {
    /// Runs versioned auth migrations with a schema-owner connection.
    /// Uses its own migration ledger so other blueprint modules can coexist.
    /// # Errors
    /// Returns sanitized database failures, including search-path restoration.
    pub async fn migrate(pool: &PgPool) -> BlueprintResult<()> {
        let mut connection = pool.acquire().await.map_err(db_error)?;
        sqlx::query("CREATE SCHEMA IF NOT EXISTS fl_better_auth")
            .execute(&mut *connection)
            .await
            .map_err(db_error)?;
        let original: String = sqlx::query_scalar("SELECT current_setting('search_path')")
            .fetch_one(&mut *connection)
            .await
            .map_err(db_error)?;
        sqlx::query("SELECT set_config('search_path','fl_better_auth,public',false)")
            .execute(&mut *connection)
            .await
            .map_err(db_error)?;
        let result = sqlx::migrate!("./migrations").run(&mut *connection).await;
        sqlx::query("SELECT set_config('search_path',$1,false)")
            .bind(original)
            .execute(&mut *connection)
            .await
            .map_err(db_error)?;
        result.map_err(|_| storage_error())
    }

    /// # Errors
    /// Rejects RLS-bypassing runtime roles or missing audit correlation IDs.
    pub async fn new(
        pool: PgPool,
        tenant: TenantContext,
        encryptor: Arc<FieldEncryptor>,
        audit: AuditContext,
    ) -> BlueprintResult<Self> {
        validate_audit(&audit)?;
        let bypass: bool = sqlx::query_scalar(
            "SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname=current_user",
        )
        .fetch_one(&pool)
        .await
        .map_err(db_error)?;
        if bypass {
            return Err(BlueprintError::new(
                "better_auth_unsafe_database_role",
                "Authentication runtime requires a role without SUPERUSER or BYPASSRLS",
            ));
        }
        let store = Self {
            pool,
            tenant,
            encryptor,
            audit,
        };
        // Existing encrypted sessions predate token indexes. Rebuild under the
        // same tenant write lock before exposing the store to authentication.
        // Duplicate/corrupt legacy tokens fail startup and roll back all claims.
        store.reindex_session_tokens().await?;
        Ok(store)
    }
    /// Rebuild token indexes for this trusted tenant in bounded pages. Call on
    /// startup (also done by `new`) and when rotating retained encryption keys.
    /// Deploy writers with token-index support together; legacy binaries do not
    /// maintain these claims and must not write concurrently after backfill.
    /// # Errors
    /// Rejects undecryptable records, duplicate tokens and database failures.
    pub async fn reindex_session_tokens(&self) -> BlueprintResult<()> {
        let mut tx = self.begin(true).await?;
        let mut after = None;
        loop {
            let page = tx.list_page(Model::Session, after, 256).await?;
            for row in &page.records {
                tx.claims(Model::Session, row).await?;
            }
            after = page.next;
            if after.is_none() {
                break;
            }
        }
        tx.commit().await
    }
    /// # Errors
    /// Rejects empty audit correlation IDs.
    pub fn with_audit_context(&self, audit: AuditContext) -> BlueprintResult<Self> {
        validate_audit(&audit)?;
        Ok(Self {
            audit,
            ..self.clone()
        })
    }
    fn encode(&self, model: Model, row: &Value) -> BlueprintResult<String> {
        let id = record_id(row)?;
        self.encryptor
            .seal(
                &json!({"model":model.as_str(),"id":id,"data":row}).to_string(),
                self.tenant.id(),
                "[Authentication data]",
            )
            .map_err(|_| storage_error())
    }
    fn decode(&self, model: Model, id: &str, ciphertext: &str) -> BlueprintResult<Value> {
        let plaintext = self
            .encryptor
            .decrypt(ciphertext, self.tenant.id())
            .map_err(|_| storage_error())?;
        let envelope: Value = serde_json::from_str(&plaintext).map_err(|_| storage_error())?;
        if envelope["model"] != model.as_str()
            || envelope["id"] != id
            || record_id(&envelope["data"]).map_err(|_| storage_error())? != id
        {
            return Err(storage_error());
        }
        Ok(envelope["data"].clone())
    }
}
pub struct PostgresAuthTransaction<'a> {
    store: &'a PostgresAuthStore,
    tx: Transaction<'a, Postgres>,
    write: bool,
}
impl AuthStore for PostgresAuthStore {
    type Transaction<'a> = PostgresAuthTransaction<'a>;
    async fn begin(&self, write: bool) -> BlueprintResult<Self::Transaction<'_>> {
        let mut tx = begin_tenant_transaction(&self.pool, &self.tenant)
            .await
            .map_err(|_| storage_error())?;
        let query = if write {
            "SELECT pg_advisory_xact_lock(hashtextextended($1,0))"
        } else {
            "SELECT pg_advisory_xact_lock_shared(hashtextextended($1,0))"
        };
        sqlx::query(query)
            .bind(format!("forklaunch:better-auth:{}", self.tenant.id()))
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        Ok(PostgresAuthTransaction {
            store: self,
            tx,
            write,
        })
    }
}
impl PostgresAuthTransaction<'_> {
    fn require_write(&self) -> BlueprintResult<()> {
        if self.write { Ok(()) } else { Err(invalid()) }
    }
    async fn audit(&mut self, model: Model, id: &str, operation: &str) -> BlueprintResult<()> {
        sqlx::query("INSERT INTO fl_better_auth.audit(id,tenant_id,model,record_id,actor_id,correlation_id,operation) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(uuid::Uuid::new_v4()).bind(self.store.tenant.id()).bind(model.as_str()).bind(id).bind(&self.store.audit.actor_id).bind(&self.store.audit.correlation_id).bind(operation)
            .execute(&mut *self.tx).await.map_err(db_error)?;
        Ok(())
    }
    async fn claims(&mut self, model: Model, row: &Value) -> BlueprintResult<()> {
        let id = record_id(row)?;
        sqlx::query(
            "DELETE FROM fl_better_auth.unique_claims WHERE tenant_id=$1 AND model=$2 AND id=$3",
        )
        .bind(self.store.tenant.id())
        .bind(model.as_str())
        .bind(id)
        .execute(&mut *self.tx)
        .await
        .map_err(db_error)?;
        for field in model.unique_fields() {
            if let Some(value) = row.get(*field).filter(|value| !value.is_null()) {
                let value = value.as_str().ok_or_else(invalid)?;
                for index in self
                    .store
                    .encryptor
                    .blind_indexes(
                        &json!([model.as_str(), field, value]).to_string(),
                        self.store.tenant.id(),
                    )
                    .map_err(|_| storage_error())?
                {
                    sqlx::query("INSERT INTO fl_better_auth.unique_claims(tenant_id,model,id,field,blind_index) VALUES($1,$2,$3,$4,$5)")
                        .bind(self.store.tenant.id()).bind(model.as_str()).bind(id).bind(field).bind(index).execute(&mut *self.tx).await.map_err(db_error)?;
                }
            }
        }
        Ok(())
    }
}
impl AuthTransaction for PostgresAuthTransaction<'_> {
    async fn get(&mut self, model: Model, id: &str) -> BlueprintResult<Option<Value>> {
        let encrypted: Option<String> = sqlx::query_scalar("SELECT ciphertext FROM fl_better_auth.records WHERE tenant_id=$1 AND model=$2 AND id=$3")
            .bind(self.store.tenant.id()).bind(model.as_str()).bind(id).fetch_optional(&mut *self.tx).await.map_err(db_error)?;
        encrypted
            .map(|value| self.store.decode(model, id, &value))
            .transpose()
    }
    async fn find_unique(
        &mut self,
        model: Model,
        field: &str,
        value: &str,
    ) -> BlueprintResult<Option<Value>> {
        if !model.unique_fields().contains(&field) {
            return Err(invalid());
        }
        let indexes = self
            .store
            .encryptor
            .blind_indexes(
                &json!([model.as_str(), field, value]).to_string(),
                self.store.tenant.id(),
            )
            .map_err(|_| storage_error())?;
        // Multiple active keys can point at the same row. Deduplicate before
        // bounding results, and fail closed if inconsistent indexes find two IDs.
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT DISTINCT r.id,r.ciphertext FROM fl_better_auth.unique_claims c JOIN fl_better_auth.records r ON r.tenant_id=c.tenant_id AND r.model=c.model AND r.id=c.id WHERE c.tenant_id=$1 AND c.model=$2 AND c.field=$3 AND c.blind_index=ANY($4) LIMIT 2",
        ).bind(self.store.tenant.id()).bind(model.as_str()).bind(field).bind(indexes)
            .fetch_all(&mut *self.tx).await.map_err(db_error)?;
        if rows.len() > 1 {
            return Err(storage_error());
        }
        rows.into_iter()
            .next()
            .map(|(id, ciphertext)| {
                let row = self.store.decode(model, &id, &ciphertext)?;
                if row.get(field).and_then(Value::as_str) != Some(value) {
                    return Err(storage_error());
                }
                Ok(row)
            })
            .transpose()
    }
    async fn get_many(
        &mut self,
        model: Model,
        ids: &[String],
        limit: usize,
    ) -> BlueprintResult<Vec<Value>> {
        super::validate_many(ids, limit)?;
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id,ciphertext FROM fl_better_auth.records WHERE tenant_id=$1 AND model=$2 AND id=ANY($3) ORDER BY sequence LIMIT $4",
        ).bind(self.store.tenant.id()).bind(model.as_str()).bind(ids).bind(i64::try_from(limit).map_err(|_| invalid())?)
            .fetch_all(&mut *self.tx).await.map_err(db_error)?;
        rows.into_iter()
            .map(|(id, ciphertext)| self.store.decode(model, &id, &ciphertext))
            .collect()
    }
    async fn list_page(
        &mut self,
        model: Model,
        after: Option<super::AuthCursor>,
        limit: usize,
    ) -> BlueprintResult<super::AuthPage> {
        let after =
            i64::try_from(super::page_start(model, after, limit)?).map_err(|_| invalid())?;
        let rows: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT sequence,id,ciphertext FROM fl_better_auth.records WHERE tenant_id=$1 AND model=$2 AND sequence>$3 ORDER BY sequence LIMIT $4",
        ).bind(self.store.tenant.id()).bind(model.as_str()).bind(after).bind(i64::try_from(limit + 1).map_err(|_| invalid())?)
            .fetch_all(&mut *self.tx).await.map_err(db_error)?;
        let rows = rows
            .into_iter()
            .map(|(sequence, id, ciphertext)| {
                Ok((
                    u64::try_from(sequence).map_err(|_| storage_error())?,
                    self.store.decode(model, &id, &ciphertext)?,
                ))
            })
            .collect::<BlueprintResult<Vec<_>>>()?;
        Ok(super::page(model, rows, limit))
    }
    async fn list(&mut self, model: Model) -> BlueprintResult<Vec<Value>> {
        let rows: Vec<(String,String)> = sqlx::query_as("SELECT id,ciphertext FROM fl_better_auth.records WHERE tenant_id=$1 AND model=$2 ORDER BY sequence LIMIT 4097")
            .bind(self.store.tenant.id()).bind(model.as_str()).fetch_all(&mut *self.tx).await.map_err(db_error)?;
        if rows.len() > 4096 {
            return Err(invalid());
        }
        rows.into_iter()
            .map(|(id, value)| self.store.decode(model, &id, &value))
            .collect()
    }
    async fn insert(&mut self, model: Model, value: Value) -> BlueprintResult<()> {
        self.require_write()?;
        validate_record(model, &value)?;
        let id = record_id(&value)?.to_owned();
        let encrypted = self.store.encode(model, &value)?;
        sqlx::query(
            "INSERT INTO fl_better_auth.records(tenant_id,model,id,ciphertext) VALUES($1,$2,$3,$4)",
        )
        .bind(self.store.tenant.id())
        .bind(model.as_str())
        .bind(&id)
        .bind(encrypted)
        .execute(&mut *self.tx)
        .await
        .map_err(db_error)?;
        self.claims(model, &value).await?;
        self.audit(model, &id, "create").await
    }
    async fn replace(&mut self, model: Model, value: Value) -> BlueprintResult<()> {
        self.require_write()?;
        validate_record(model, &value)?;
        let id = record_id(&value)?.to_owned();
        let encrypted = self.store.encode(model, &value)?;
        let affected = sqlx::query("UPDATE fl_better_auth.records SET ciphertext=$4 WHERE tenant_id=$1 AND model=$2 AND id=$3")
            .bind(self.store.tenant.id()).bind(model.as_str()).bind(&id).bind(encrypted).execute(&mut *self.tx).await.map_err(db_error)?.rows_affected();
        if affected == 0 {
            return Err(not_found());
        }
        self.claims(model, &value).await?;
        self.audit(model, &id, "update").await
    }
    async fn delete(&mut self, model: Model, id: &str) -> BlueprintResult<()> {
        self.require_write()?;
        let affected = sqlx::query(
            "DELETE FROM fl_better_auth.records WHERE tenant_id=$1 AND model=$2 AND id=$3",
        )
        .bind(self.store.tenant.id())
        .bind(model.as_str())
        .bind(id)
        .execute(&mut *self.tx)
        .await
        .map_err(db_error)?
        .rows_affected();
        if affected > 0 {
            self.audit(model, id, "delete").await?;
        }
        Ok(())
    }
    async fn commit(self) -> BlueprintResult<()> {
        self.tx.commit().await.map_err(db_error)
    }
}
fn validate_audit(audit: &AuditContext) -> BlueprintResult<()> {
    if audit.correlation_id.trim().is_empty() {
        Err(invalid())
    } else {
        Ok(())
    }
}
#[allow(clippy::needless_pass_by_value)] // Consumed by Result::map_err.
fn db_error(error: sqlx::Error) -> BlueprintError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        conflict()
    } else {
        storage_error()
    }
}
