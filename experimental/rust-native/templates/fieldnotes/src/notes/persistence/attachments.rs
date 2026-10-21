//! Private object storage. SQL and S3 cannot share a transaction: publication is a
//! durable pending→ready transition; abandoned ciphertext is reclaimed separately.
use super::compliance::{ProtectedPayload, reveal_payload};
use super::entities::AttachmentRecord;
use crate::NotesRepository;
use fieldnotes_security::{SecurityError, VerifiedTenant};
use s3::{
    AsyncObjectStore, ObjectMetadata,
    aws::{AwsS3ObjectStore, environment::S3Environment},
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;
use zeroize::Zeroizing;

pub struct AttachmentDownload {
    pub metadata: AttachmentRecord,
    pub body: Zeroizing<Vec<u8>>,
}
#[derive(Serialize, Deserialize)]
struct Metadata {
    filename: String,
    content_type: String,
}
fn object_key(tenant: Uuid, note: Uuid, id: Uuid) -> String {
    format!("attachments/{tenant}/{note}/{id}")
}
async fn store_from_env() -> Result<AwsS3ObjectStore, SecurityError> {
    let mut config = S3Environment::from_env().map_err(|_| SecurityError::InvalidInput)?;
    config.options.max_object_bytes = 1_400_000;
    config.options.operation_timeout = std::time::Duration::from_secs(30);
    config
        .into_store()
        .await
        .map_err(|_| SecurityError::Unavailable)
}
pub async fn initialize_attachment_bucket() -> Result<(), SecurityError> {
    store_from_env()
        .await?
        .init_bucket(true)
        .await
        .map_err(|_| SecurityError::Unavailable)
}
impl NotesRepository {
    pub async fn with_object_store_from_env(mut self) -> Result<Self, SecurityError> {
        let store = store_from_env().await?;
        store
            .init_bucket(false)
            .await
            .map_err(|_| SecurityError::Unavailable)?;
        self.objects = Some(store);
        Ok(self)
    }
    async fn attachment(
        &self,
        scope: &VerifiedTenant,
        row: &sqlx::postgres::PgRow,
    ) -> Result<AttachmentRecord, SecurityError> {
        let id: Uuid = row.try_get("id")?;
        let tenant: Uuid = row.try_get("workspace_id")?;
        let note: Uuid = row.try_get("note_id")?;
        if tenant != scope.tenant_id() {
            return Err(SecurityError::Forbidden);
        }
        let audit = super::compliance::AccessAudit::new(&self.audit_pool, scope);
        let raw = reveal_payload(
            &self.key,
            tenant,
            id,
            ProtectedPayload::AttachmentMetadata { note },
            row.try_get("metadata")?,
            audit.actor(),
            &audit,
        )
        .await?;
        let metadata: Metadata =
            serde_json::from_slice(&raw).map_err(|_| SecurityError::Encryption)?;
        Ok(AttachmentRecord {
            id,
            workspace_id: tenant,
            note_id: row.try_get("note_id")?,
            filename: metadata.filename,
            content_type: metadata.content_type,
            size: row.try_get("size")?,
            created_at: row.try_get("created_at")?,
        })
    }
    pub async fn upload_attachment(
        &self,
        scope: &VerifiedTenant,
        note: Uuid,
        filename: &str,
        content_type: &str,
        body: &[u8],
    ) -> Result<AttachmentRecord, SecurityError> {
        scope.require_write()?;
        if body.len() > 1_048_576
            || filename.is_empty()
            || filename.len() > 255
            || filename.chars().any(char::is_control)
            || content_type.is_empty()
            || content_type.len() > 128
            || !content_type
                .bytes()
                .all(|c| c.is_ascii_graphic() || c == b' ')
        {
            return Err(SecurityError::InvalidInput);
        }
        let store = self.objects.as_ref().ok_or(SecurityError::Unavailable)?;
        let id = Uuid::new_v4();
        let raw = Zeroizing::new(
            serde_json::to_vec(&Metadata {
                filename: filename.into(),
                content_type: content_type.into(),
            })
            .map_err(|_| SecurityError::InvalidInput)?,
        );
        let metadata = self.key.seal(
            scope.tenant_id(),
            id,
            &format!("attachment-metadata-{note}"),
            &raw,
        )?;
        let ciphertext = self.key.seal(
            scope.tenant_id(),
            id,
            &format!("attachment-body-{note}"),
            body,
        )?;
        let mut tx = self.transaction(scope.tenant_id()).await?;
        let exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM fieldnotes_notes WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(scope.tenant_id())
        .bind(note)
        .fetch_optional(&mut *tx)
        .await?;
        if exists.is_none() {
            return Err(SecurityError::NotFound);
        }
        sqlx::query("INSERT INTO fieldnotes_attachments(id,workspace_id,note_id,metadata,size) VALUES($1,$2,$3,$4,$5)").bind(id).bind(scope.tenant_id()).bind(note).bind(metadata).bind(body.len() as i64).execute(&mut *tx).await?;
        tx.commit().await?;
        // No database transaction is held during object I/O. Pending rows are never exposed.
        store
            .put(
                &object_key(scope.tenant_id(), note, id),
                ciphertext.into_bytes(),
                ObjectMetadata::new().with_content_type("application/octet-stream"),
            )
            .await
            .map_err(|_| SecurityError::Unavailable)?;
        let mut tx = self.transaction(scope.tenant_id()).await?;
        let exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM fieldnotes_notes WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(scope.tenant_id())
        .bind(note)
        .fetch_optional(&mut *tx)
        .await?;
        if exists.is_none() {
            return Err(SecurityError::NotFound);
        }
        let row=sqlx::query("UPDATE fieldnotes_attachments SET state='ready' WHERE id=$1 AND workspace_id=$2 AND state='pending' AND expires_at>clock_timestamp() RETURNING *").bind(id).bind(scope.tenant_id()).fetch_optional(&mut *tx).await?.ok_or(SecurityError::Conflict)?;
        let result = self.attachment(scope, &row).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn list_attachments(
        &self,
        scope: &VerifiedTenant,
        note: Uuid,
    ) -> Result<Vec<AttachmentRecord>, SecurityError> {
        let mut tx = self.transaction(scope.tenant_id()).await?;
        let exists: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM fieldnotes_notes WHERE workspace_id=$1 AND id=$2")
                .bind(scope.tenant_id())
                .bind(note)
                .fetch_optional(&mut *tx)
                .await?;
        if exists.is_none() {
            return Err(SecurityError::NotFound);
        }
        let rows=sqlx::query("SELECT * FROM fieldnotes_attachments WHERE workspace_id=$1 AND note_id=$2 AND state='ready' ORDER BY created_at,id LIMIT 100").bind(scope.tenant_id()).bind(note).fetch_all(&mut *tx).await?;
        let mut result = Vec::with_capacity(rows.len());
        for row in &rows {
            result.push(self.attachment(scope, row).await?);
        }
        tx.commit().await?;
        Ok(result)
    }
    pub async fn download_attachment(
        &self,
        scope: &VerifiedTenant,
        note: Uuid,
        id: Uuid,
    ) -> Result<AttachmentDownload, SecurityError> {
        let store = self.objects.as_ref().ok_or(SecurityError::Unavailable)?;
        let mut tx = self.transaction(scope.tenant_id()).await?;
        let row=sqlx::query("SELECT a.* FROM fieldnotes_attachments a JOIN fieldnotes_notes n ON n.id=a.note_id AND n.workspace_id=a.workspace_id WHERE a.workspace_id=$1 AND a.note_id=$2 AND a.id=$3 AND a.state='ready'").bind(scope.tenant_id()).bind(note).bind(id).fetch_optional(&mut *tx).await?.ok_or(SecurityError::NotFound)?;
        let metadata = self.attachment(scope, &row).await?;
        tx.commit().await?;
        let object = store
            .get(&object_key(scope.tenant_id(), note, id))
            .await
            .map_err(|_| SecurityError::Unavailable)?
            .ok_or(SecurityError::NotFound)?;
        let envelope = std::str::from_utf8(&object.body).map_err(|_| SecurityError::Encryption)?;
        let audit = super::compliance::AccessAudit::new(&self.audit_pool, scope);
        let body = reveal_payload(
            &self.key,
            scope.tenant_id(),
            id,
            ProtectedPayload::AttachmentBody { note },
            envelope,
            audit.actor(),
            &audit,
        )
        .await?;
        if body.len() as i64 != metadata.size {
            return Err(SecurityError::Encryption);
        }
        Ok(AttachmentDownload { metadata, body })
    }
}
