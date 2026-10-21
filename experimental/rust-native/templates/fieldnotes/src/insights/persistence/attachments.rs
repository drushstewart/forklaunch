//! Worker-owned object reclamation; no Notes server dependency.
use fieldnotes_security::SecurityError;
use s3::{
    AsyncObjectStore,
    aws::{AwsS3ObjectStore, environment::S3Environment},
};
use sqlx::{PgPool, Row};
use uuid::Uuid;

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
/// Worker-owned bounded cleanup, requiring only encrypted metadata access. No user identity
/// is accepted here: it can remove only tombstones or uploads expired for over an hour.
pub struct AttachmentMaintenance {
    pool: PgPool,
    store: AwsS3ObjectStore,
}
impl AttachmentMaintenance {
    pub async fn from_env(pool: PgPool) -> Result<Self, SecurityError> {
        let store = store_from_env().await?;
        store
            .init_bucket(false)
            .await
            .map_err(|_| SecurityError::Unavailable)?;
        Ok(Self { pool, store })
    }
    pub async fn cleanup(&self) -> Result<u64, SecurityError> {
        let rows=sqlx::query("SELECT id,workspace_id,note_id FROM fieldnotes_attachments WHERE state='deleted' OR (state='pending' AND expires_at<clock_timestamp()-interval '1 hour') ORDER BY created_at,id LIMIT 100").fetch_all(&self.pool).await?;
        let mut count = 0;
        for row in rows {
            let id: Uuid = row.try_get("id")?;
            let tenant: Uuid = row.try_get("workspace_id")?;
            let note: Uuid = row.try_get("note_id")?;
            self.store
                .delete(&object_key(tenant, note, id))
                .await
                .map_err(|_| SecurityError::Unavailable)?;
            count+=sqlx::query("DELETE FROM fieldnotes_attachments WHERE id=$1 AND workspace_id=$2 AND (state='deleted' OR (state='pending' AND expires_at<clock_timestamp()-interval '1 hour'))").bind(id).bind(tenant).execute(&self.pool).await?.rows_affected();
        }
        Ok(count)
    }
}
