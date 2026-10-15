//! Encrypted, audited `PostgreSQL` auth storage with forced tenant RLS.
mod member_query;

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
    permission_cache: crate::organizations::OrganizationPermissionCache,
    tenant: TenantContext,
    encryptor: Arc<FieldEncryptor>,
    audit: AuditContext,
    member_query_profile: super::MemberQueryProfile,
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
            permission_cache: crate::organizations::OrganizationPermissionCache::postgres(
                &pool,
                tenant.id(),
            ),
            pool,
            tenant,
            encryptor,
            audit,
            member_query_profile: super::MemberQueryProfile::default(),
        };
        // Existing encrypted sessions predate token indexes. Rebuild under the
        // same tenant write lock before exposing the store to authentication.
        // Duplicate/corrupt legacy tokens fail startup and roll back all claims.
        store.reindex_session_tokens().await?;
        store.reindex_relationships().await?;
        Ok(store)
    }
    /// Selects actual `PostgreSQL` filtering/sorting of transient public member
    /// projections. No plaintext projection is persisted. Configure database
    /// parameter logging according to the application's data policy.
    #[must_use]
    pub fn with_member_query_profile(mut self, profile: super::MemberQueryProfile) -> Self {
        self.member_query_profile = profile;
        self
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
    /// Rebuild encrypted relationship indexes under the tenant write lock.
    /// Also runs on startup. All writers must support these indexes after rollout.
    /// # Errors
    /// Rejects corrupt encrypted records and database failures atomically.
    pub async fn reindex_relationships(&self) -> BlueprintResult<()> {
        let mut tx = self.begin(true).await?;
        for model in [
            Model::Session,
            Model::Account,
            Model::Member,
            Model::OrganizationRole,
            Model::Team,
            Model::TeamMember,
            Model::Invitation,
            Model::Verification,
        ] {
            let mut after = None;
            loop {
                let page = tx.list_page(model, after, 256).await?;
                for row in &page.records {
                    tx.lookup_claims(model, row).await?;
                }
                after = page.next;
                if after.is_none() {
                    break;
                }
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
    fn organization_permission_cache(
        &self,
    ) -> Option<crate::organizations::OrganizationPermissionCache> {
        Some(self.permission_cache.clone())
    }
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
    async fn lookup_claims(&mut self, model: Model, row: &Value) -> BlueprintResult<()> {
        let id = record_id(row)?;
        sqlx::query(
            "DELETE FROM fl_better_auth.lookup_claims WHERE tenant_id=$1 AND model=$2 AND id=$3",
        )
        .bind(self.store.tenant.id())
        .bind(model.as_str())
        .bind(id)
        .execute(&mut *self.tx)
        .await
        .map_err(db_error)?;
        for field in model.indexed_fields() {
            if let Some(value) = row.get(*field).and_then(Value::as_str) {
                let indexes = self
                    .store
                    .encryptor
                    .blind_indexes(
                        &json!(["relationship", model.as_str(), field, value]).to_string(),
                        self.store.tenant.id(),
                    )
                    .map_err(|_| storage_error())?;
                // One statement stores all retained-key claims for this field.
                sqlx::query("INSERT INTO fl_better_auth.lookup_claims(tenant_id,model,id,field,blind_index,sequence) SELECT tenant_id,model,id,$4,unnest($5::text[]),sequence FROM fl_better_auth.records WHERE tenant_id=$1 AND model=$2 AND id=$3")
                    .bind(self.store.tenant.id()).bind(model.as_str()).bind(id).bind(field).bind(indexes)
                    .execute(&mut *self.tx).await.map_err(db_error)?;
            }
        }
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
    async fn find_page(
        &mut self,
        model: Model,
        field: &str,
        value: &str,
        after: Option<super::AuthCursor>,
        limit: usize,
    ) -> BlueprintResult<super::AuthPage> {
        let after =
            i64::try_from(super::page_start(model, after, limit)?).map_err(|_| invalid())?;
        if !model.indexed_fields().contains(&field) {
            return Err(invalid());
        }
        let indexes = self
            .store
            .encryptor
            .blind_indexes(
                &json!(["relationship", model.as_str(), field, value]).to_string(),
                self.store.tenant.id(),
            )
            .map_err(|_| storage_error())?;
        let rows: Vec<(i64, i64, String, String)> = sqlx::query_as(
            "SELECT DISTINCT c.sequence,r.sequence,r.id,r.ciphertext FROM fl_better_auth.lookup_claims c JOIN fl_better_auth.records r ON r.tenant_id=c.tenant_id AND r.model=c.model AND r.id=c.id WHERE c.tenant_id=$1 AND c.model=$2 AND c.field=$3 AND c.blind_index=ANY($4) AND c.sequence>$5 ORDER BY c.sequence LIMIT $6"
        ).bind(self.store.tenant.id()).bind(model.as_str()).bind(field).bind(indexes).bind(after)
            .bind(i64::try_from(limit + 1).map_err(|_| invalid())?)
            .fetch_all(&mut *self.tx).await.map_err(db_error)?;
        let rows = rows
            .into_iter()
            .map(|(claimed_sequence, sequence, id, ciphertext)| {
                let row = self.store.decode(model, &id, &ciphertext)?;
                if claimed_sequence != sequence
                    || row.get(field).and_then(Value::as_str) != Some(value)
                {
                    return Err(storage_error());
                }
                Ok((u64::try_from(sequence).map_err(|_| storage_error())?, row))
            })
            .collect::<BlueprintResult<Vec<_>>>()?;
        Ok(super::page(model, rows, limit))
    }
    async fn find_window(
        &mut self,
        model: Model,
        field: &str,
        value: &str,
        offset: usize,
        limit: usize,
    ) -> BlueprintResult<super::AuthWindow> {
        super::page_start(model, None, limit)?;
        let offset = i64::try_from(offset).map_err(|_| invalid())?;
        if !model.indexed_fields().contains(&field) {
            return Err(invalid());
        }
        let indexes = self
            .store
            .encryptor
            .blind_indexes(
                &json!(["relationship", model.as_str(), field, value]).to_string(),
                self.store.tenant.id(),
            )
            .map_err(|_| storage_error())?;
        // Deduplicate retained-key claims before pagination. Counts read only
        // indexed claims; the materialized window bounds ciphertext retrieval.
        let total: i64 = sqlx::query_scalar("SELECT count(DISTINCT id) FROM fl_better_auth.lookup_claims WHERE tenant_id=$1 AND model=$2 AND field=$3 AND blind_index=ANY($4)")
            .bind(self.store.tenant.id()).bind(model.as_str()).bind(field).bind(&indexes)
            .fetch_one(&mut *self.tx).await.map_err(db_error)?;
        let rows: Vec<(i64, i64, String, String)> = sqlx::query_as("WITH matched AS MATERIALIZED (SELECT DISTINCT sequence,id FROM fl_better_auth.lookup_claims WHERE tenant_id=$1 AND model=$2 AND field=$3 AND blind_index=ANY($4) ORDER BY sequence,id OFFSET $5 LIMIT $6) SELECT m.sequence,r.sequence,r.id,r.ciphertext FROM matched m JOIN fl_better_auth.records r ON r.tenant_id=$1 AND r.model=$2 AND r.id=m.id ORDER BY m.sequence")
            .bind(self.store.tenant.id()).bind(model.as_str()).bind(field).bind(indexes).bind(offset).bind(i64::try_from(limit).map_err(|_| invalid())?)
            .fetch_all(&mut *self.tx).await.map_err(db_error)?;
        let records = rows
            .into_iter()
            .map(|(claimed_sequence, sequence, id, ciphertext)| {
                let row = self.store.decode(model, &id, &ciphertext)?;
                if claimed_sequence != sequence
                    || row.get(field).and_then(Value::as_str) != Some(value)
                {
                    return Err(storage_error());
                }
                Ok(row)
            })
            .collect::<BlueprintResult<Vec<_>>>()?;
        Ok(super::AuthWindow {
            records,
            total: usize::try_from(total).map_err(|_| storage_error())?,
        })
    }
    async fn query_members(
        &mut self,
        organization: &str,
        query: &Value,
    ) -> BlueprintResult<Option<super::AuthWindow>> {
        if self.store.member_query_profile == super::MemberQueryProfile::MemoryAdapter {
            return Ok(None);
        }
        self.query_member_window(organization, query)
            .await
            .map(Some)
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
        self.lookup_claims(model, &value).await?;
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
        self.lookup_claims(model, &value).await?;
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
        let mut result = storage_error();
        if let Some(database) = error.as_database_error() {
            for (field, value) in [
                ("code", database.code().map(std::borrow::Cow::into_owned)),
                ("constraint", database.constraint().map(str::to_owned)),
                ("table", database.table().map(str::to_owned)),
                (
                    "column",
                    database
                        .try_downcast_ref::<sqlx::postgres::PgDatabaseError>()
                        .and_then(sqlx::postgres::PgDatabaseError::column)
                        .map(str::to_owned),
                ),
            ] {
                if let Some(value) = value {
                    result = result.with_detail(field, serde_json::Value::String(value));
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod diagnostics_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires FORKLAUNCH_TEST_POSTGRES_URL"]
    async fn api_error_observer_preserves_only_structural_postgres_fields() {
        let pool = PgPool::connect(&std::env::var("FORKLAUNCH_TEST_POSTGRES_URL").unwrap())
            .await
            .unwrap();
        let mut transaction = pool.begin().await.unwrap();
        sqlx::query("CREATE TEMP TABLE fl_auth_error_fixture (required_value TEXT NOT NULL, private_value TEXT) ON COMMIT DROP")
            .execute(&mut *transaction).await.unwrap();
        let error = sqlx::query("INSERT INTO fl_auth_error_fixture VALUES (NULL, $1)")
            .bind("PRIVATE_ROW_SENTINEL")
            .execute(&mut *transaction)
            .await
            .unwrap_err();
        let error = db_error(error);
        assert_eq!(error.code, "better_auth_storage_error");
        assert_eq!(error.details.get("code"), Some(&json!("23502")));
        assert_eq!(
            error.details.get("table"),
            Some(&json!("fl_auth_error_fixture"))
        );
        assert_eq!(error.details.get("column"), Some(&json!("required_value")));
        assert_eq!(error.details.len(), 3);
        assert!(!format!("{error:?}").contains("PRIVATE_ROW"));
        transaction.rollback().await.unwrap();
        pool.close().await;
    }
}
