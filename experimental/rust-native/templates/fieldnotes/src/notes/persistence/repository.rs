use chrono::{DateTime, Utc};
use fieldnotes_security::{EnvelopeKey, SecurityError, VerifiedTenant};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::domain::*;

use super::compliance_schema::NoteStorage;
use framework_core::persistence::protected::{
    ProtectedContext, ProtectedField, storage::ProtectedModel,
};

impl NoteStorage {
    fn from_row(row: &sqlx::postgres::PgRow) -> Result<Self, SecurityError> {
        let binding = |result: framework_core::common::FrameworkResult<ProtectedField>| {
            result.map_err(|_| SecurityError::Encryption)
        };
        let excerpt: Option<String> = row.try_get("insight_excerpt")?;
        let stored = Self {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            title: binding(ProtectedField::from_ciphertext(
                Self::protected_title().map_err(|_| SecurityError::Encryption)?,
                row.try_get("title")?,
            ))?,
            body: binding(ProtectedField::from_ciphertext(
                Self::protected_body().map_err(|_| SecurityError::Encryption)?,
                row.try_get("body")?,
            ))?,
            insight_excerpt: excerpt
                .map(|value| {
                    binding(ProtectedField::from_ciphertext(
                        Self::protected_insight_excerpt().map_err(|_| SecurityError::Encryption)?,
                        value,
                    ))
                })
                .transpose()?,
            revision: row.try_get("revision")?,
            created_by: row.try_get("created_by")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            insight_word_count: row.try_get("insight_word_count")?,
            cache_version: row.try_get("cache_version")?,
        };
        stored
            .validate_protected_storage()
            .map_err(|_| SecurityError::Encryption)?;
        Ok(stored)
    }
}

impl NotesRepository {
    fn protect(
        &self,
        tenant: Uuid,
        id: Uuid,
        purpose: &str,
        value: &str,
    ) -> Result<ProtectedField, SecurityError> {
        super::compliance::seal_note(&self.key, tenant, id, purpose, value)
    }
    async fn reveal(
        &self,
        scope: &VerifiedTenant,
        id: Uuid,
        field: &ProtectedField,
    ) -> Result<String, SecurityError> {
        let audit = super::compliance::AccessAudit::new(&self.audit_pool, scope);
        let plain = field
            .reveal_async(
                self.key.as_keyring(),
                ProtectedContext::new(scope.tenant_id(), id)
                    .map_err(|_| SecurityError::InvalidInput)?,
                audit.actor(),
                "map-authorized-note",
                &audit,
            )
            .await
            .map_err(|_| SecurityError::Unavailable)?;
        String::from_utf8(plain.to_vec()).map_err(|_| SecurityError::Encryption)
    }
    pub(crate) async fn record(
        &self,
        scope: &VerifiedTenant,
        row: &sqlx::postgres::PgRow,
    ) -> Result<NoteRecord, SecurityError> {
        let tenant: Uuid = row.try_get("workspace_id")?;
        if tenant != scope.tenant_id() {
            return Err(SecurityError::Forbidden);
        }
        let stored = NoteStorage::from_row(row)?;
        let insight_excerpt = match &stored.insight_excerpt {
            Some(value) => Some(self.reveal(scope, stored.id, value).await?),
            None => None,
        };
        Ok(NoteRecord {
            id: stored.id,
            workspace_id: stored.workspace_id,
            title: self.reveal(scope, stored.id, &stored.title).await?,
            body: self.reveal(scope, stored.id, &stored.body).await?,
            revision: stored.revision,
            created_at: stored.created_at,
            updated_at: stored.updated_at,
            insight_excerpt,
            insight_word_count: stored.insight_word_count,
        })
    }
    pub async fn list(&self, scope: &VerifiedTenant) -> Result<Vec<NoteRecord>, SecurityError> {
        let mut tx = self.transaction(scope.tenant_id()).await?;
        let rows = sqlx::query(
            "SELECT * FROM fieldnotes_notes WHERE workspace_id=$1 ORDER BY created_at,id LIMIT 200",
        )
        .bind(scope.tenant_id())
        .fetch_all(&mut *tx)
        .await?;
        let mut result = Vec::with_capacity(rows.len());
        for row in &rows {
            result.push(self.record(scope, row).await?);
        }
        tx.commit().await?;
        Ok(result)
    }
    pub async fn get(&self, scope: &VerifiedTenant, id: Uuid) -> Result<NoteRecord, SecurityError> {
        self.get_with_cache(scope, id).await
    }

    async fn enqueue(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        tenant: Uuid,
        id: Uuid,
        revision: i64,
        body: &str,
    ) -> Result<InsightRequest, SecurityError> {
        let operation_id = Uuid::new_v4();
        let job = Zeroizing::new(worker::TracedJob::new(fieldnotes_contracts::events::insight::NoteJob {
            operation_id,
            workspace_id: tenant,
            note_id: id,
            revision,
            body: body.to_owned(),
        }));
        let payload = worker::seal_job(
            self.key.as_keyring(),
            fieldnotes_contracts::events::insight::PURPOSE,
            &*job,
        )
        .map_err(|_| SecurityError::Encryption)?;
        sqlx::query("INSERT INTO fieldnotes_outbox(id,workspace_id,note_id,revision,kind,payload) VALUES($1,$2,$3,$4,'note-insight-v2',$5)").bind(operation_id).bind(tenant).bind(id).bind(revision).bind(payload).execute(&mut **tx).await?;
        Ok(InsightRequest {
            operation_id,
            revision,
        })
    }
    async fn event(
        tx: &mut Transaction<'_, Postgres>,
        tenant: Uuid,
        id: Uuid,
        revision: i64,
        kind: &str,
    ) -> Result<(), SecurityError> {
        sqlx::query(
            "INSERT INTO fieldnotes_events(workspace_id,note_id,revision,kind) VALUES($1,$2,$3,$4)",
        )
        .bind(tenant)
        .bind(id)
        .bind(revision)
        .bind(kind)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
    pub async fn create(
        &self,
        scope: &VerifiedTenant,
        title: &str,
        body: &str,
    ) -> Result<NoteRecord, SecurityError> {
        scope.require_write()?;
        crate::domain::validate(title, body)?;
        let id = Uuid::new_v4();
        let protected_title = self.protect(scope.tenant_id(), id, "note-title", title)?;
        let protected_body = self.protect(scope.tenant_id(), id, "note-body", body)?;
        let mut tx = self.transaction(scope.tenant_id()).await?;
        let row=sqlx::query("INSERT INTO fieldnotes_notes(id,workspace_id,title,body,created_by) VALUES($1,$2,$3,$4,$5) RETURNING *").bind(id).bind(scope.tenant_id()).bind(protected_title.ciphertext()).bind(protected_body.ciphertext()).bind(scope.user_id()).fetch_one(&mut *tx).await?;
        self.enqueue(&mut tx, scope.tenant_id(), id, 1, body)
            .await?;
        Self::event(&mut tx, scope.tenant_id(), id, 1, "note.created").await?;
        let note = self.record(scope, &row).await?;
        tx.commit().await?;
        Ok(note)
    }
    pub async fn update(
        &self,
        scope: &VerifiedTenant,
        id: Uuid,
        expected_revision: i64,
        title: Option<&str>,
        body: Option<&str>,
    ) -> Result<NoteRecord, SecurityError> {
        scope.require_write()?;
        if title.is_none() && body.is_none() {
            return Err(SecurityError::InvalidInput);
        }
        let mut tx = self.transaction(scope.tenant_id()).await?;
        let row = sqlx::query(
            "SELECT * FROM fieldnotes_notes WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(scope.tenant_id())
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(SecurityError::NotFound)?;
        let old = self.record(scope, &row).await?;
        if old.revision != expected_revision {
            return Err(SecurityError::Conflict);
        }
        let title = title.unwrap_or(&old.title);
        let body = body.unwrap_or(&old.body);
        crate::domain::validate(title, body)?;
        let title = self.protect(scope.tenant_id(), id, "note-title", title)?;
        let protected_body = self.protect(scope.tenant_id(), id, "note-body", body)?;
        let revision = old.revision.checked_add(1).ok_or(SecurityError::Storage)?;
        let row=sqlx::query("UPDATE fieldnotes_notes SET title=$3,body=$4,revision=$5,updated_at=now(),insight_excerpt=NULL,insight_word_count=NULL WHERE workspace_id=$1 AND id=$2 RETURNING *").bind(scope.tenant_id()).bind(id).bind(title.ciphertext()).bind(protected_body.ciphertext()).bind(revision).fetch_one(&mut *tx).await?;
        self.enqueue(&mut tx, scope.tenant_id(), id, revision, body)
            .await?;
        Self::event(&mut tx, scope.tenant_id(), id, revision, "note.updated").await?;
        let note = self.record(scope, &row).await?;
        tx.commit().await?;
        Ok(note)
    }
    pub async fn delete(&self, scope: &VerifiedTenant, id: Uuid) -> Result<(), SecurityError> {
        scope.require_write()?;
        let mut tx = self.transaction(scope.tenant_id()).await?;
        let revision: Option<i64> = sqlx::query_scalar(
            "DELETE FROM fieldnotes_notes WHERE workspace_id=$1 AND id=$2 RETURNING revision",
        )
        .bind(scope.tenant_id())
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        sqlx::query("UPDATE fieldnotes_attachments SET state='deleted' WHERE workspace_id=$1 AND note_id=$2")
            .bind(scope.tenant_id()).bind(id).execute(&mut *tx).await?;
        Self::event(
            &mut tx,
            scope.tenant_id(),
            id,
            revision.ok_or(SecurityError::NotFound)?,
            "note.deleted",
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn request_insights(
        &self,
        scope: &VerifiedTenant,
        id: Uuid,
    ) -> Result<InsightRequest, SecurityError> {
        scope.require_write()?;
        let mut tx = self.transaction(scope.tenant_id()).await?;
        let row = sqlx::query(
            "SELECT * FROM fieldnotes_notes WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(scope.tenant_id())
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(SecurityError::NotFound)?;
        let note = self.record(scope, &row).await?;
        let request = self
            .enqueue(&mut tx, scope.tenant_id(), id, note.revision, &note.body)
            .await?;
        tx.commit().await?;
        Ok(request)
    }
    pub async fn events(
        &self,
        scope: &VerifiedTenant,
        after_id: i64,
    ) -> Result<Vec<NoteEventRecord>, SecurityError> {
        let mut tx = self.transaction(scope.tenant_id()).await?;
        let rows = sqlx::query(
            "SELECT * FROM fieldnotes_events WHERE workspace_id=$1 AND id>$2 ORDER BY id LIMIT 100",
        )
        .bind(scope.tenant_id())
        .bind(after_id.max(0))
        .fetch_all(&mut *tx)
        .await?;
        let result = rows
            .iter()
            .map(|r| {
                Ok(NoteEventRecord {
                    id: r.try_get("id")?,
                    workspace_id: r.try_get("workspace_id")?,
                    note_id: r.try_get("note_id")?,
                    revision: r.try_get("revision")?,
                    kind: r.try_get("kind")?,
                    created_at: r.try_get("created_at")?,
                })
            })
            .collect();
        tx.commit().await?;
        result
    }
    pub async fn apply_insight(
        &self,
        tenant: Uuid,
        id: Uuid,
        input: InsightInput,
    ) -> Result<InsightApplyResult, SecurityError> {
        if input.excerpt.chars().count() > 160 || input.word_count < 0 || input.word_count > 65536 {
            return Err(SecurityError::InvalidInput);
        }
        let mut tx = self.transaction(tenant).await?;
        // Serialize on note before outbox to match create/update/request lock ordering.
        let revision: Option<i64> = sqlx::query_scalar(
            "SELECT revision FROM fieldnotes_notes WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(tenant)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let row=sqlx::query("SELECT revision,applied_at FROM fieldnotes_outbox WHERE id=$1 AND workspace_id=$2 AND note_id=$3 FOR UPDATE").bind(input.operation_id).bind(tenant).bind(id).fetch_optional(&mut *tx).await?.ok_or(SecurityError::NotFound)?;
        if row.try_get::<i64, _>("revision")? != input.revision {
            return Err(SecurityError::InvalidInput);
        }
        if row
            .try_get::<Option<DateTime<Utc>>, _>("applied_at")?
            .is_some()
        {
            tx.commit().await?;
            return Ok(InsightApplyResult::Duplicate);
        }
        if revision != Some(input.revision) {
            tx.commit().await?;
            return Ok(InsightApplyResult::Stale);
        }
        let excerpt = self.protect(tenant, id, "note-insight", &input.excerpt)?;
        sqlx::query("UPDATE fieldnotes_notes SET insight_excerpt=$3,insight_word_count=$4 WHERE workspace_id=$1 AND id=$2").bind(tenant).bind(id).bind(excerpt.ciphertext()).bind(input.word_count).execute(&mut *tx).await?;
        sqlx::query("UPDATE fieldnotes_outbox SET applied_at=now() WHERE id=$1")
            .bind(input.operation_id)
            .execute(&mut *tx)
            .await?;
        Self::event(&mut tx, tenant, id, input.revision, "note.insight.ready").await?;
        tx.commit().await?;
        Ok(InsightApplyResult::Applied)
    }
}

/// Notes operations require identity verified by the configured auth service.
/// Caller-supplied workspace text cannot substitute for that capability:
/// ```compile_fail
/// async fn forged(repository: &fieldnotes_notes::NotesRepository) {
///     repository.list("11111111-1111-4111-8111-111111111111").await.unwrap();
/// }
/// ```
pub struct NotesRepository {
    pub(super) pool: PgPool,
    pub(super) audit_pool: PgPool,
    pub(super) key: EnvelopeKey,
    pub(super) objects: Option<s3::aws::AwsS3ObjectStore>,
    pub(super) cache: Option<super::cache::NoteCache>,
}
impl NotesRepository {
    pub fn new(pool: PgPool, key: EnvelopeKey) -> Result<Self, SecurityError> {
        use framework_core::persistence::postgres09::{
            PostgresConnectionPolicy, clone_runtime_postgres_pool,
        };
        let policy = PostgresConnectionPolicy::from_env(4).map_err(|_| SecurityError::Storage)?;
        let audit_pool =
            clone_runtime_postgres_pool(&pool, policy).map_err(|_| SecurityError::Storage)?;
        Ok(Self {
            audit_pool,
            pool,
            key,
            objects: None,
            cache: None,
        })
    }
    pub(super) async fn transaction(
        &self,
        tenant: Uuid,
    ) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, SecurityError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('fieldnotes.tenant_id',$1,true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }
}
