use crate::{QUEUE, SealedNoteJob, WorkerError};
use apalis::prelude::TaskBuilder;
use sqlx::{PgPool, Row};
/// One bounded transaction, no plaintext decode. A lost COMMIT acknowledgement is
/// safe to retry: committed rows are no longer selected; rolled-back tasks do not exist.
/// Concurrent dispatchers skip each other's locked outbox rows.
pub async fn dispatch_batch(pool: &PgPool) -> Result<usize, WorkerError> {
    let mut tx = pool.begin().await.map_err(|_| WorkerError::Storage)?;
    let rows = sqlx::query("SELECT id,workspace_id,note_id,revision,payload FROM fieldnotes_outbox WHERE processed_at IS NULL AND kind='note-insight-v2' ORDER BY created_at,id LIMIT 32 FOR UPDATE SKIP LOCKED")
        .fetch_all(&mut *tx).await.map_err(|_| WorkerError::Storage)?;
    let mut tasks = Vec::with_capacity(rows.len());
    let mut ids = Vec::with_capacity(rows.len());
    for row in rows {
        let job = SealedNoteJob {
            operation_id: row.try_get("id").map_err(|_| WorkerError::Storage)?,
            workspace_id: row
                .try_get("workspace_id")
                .map_err(|_| WorkerError::Storage)?,
            note_id: row.try_get("note_id").map_err(|_| WorkerError::Storage)?,
            revision: row.try_get("revision").map_err(|_| WorkerError::Storage)?,
            payload: row.try_get("payload").map_err(|_| WorkerError::Storage)?,
        };
        ids.push(job.operation_id);
        let bytes = serde_json::to_vec(&job).map_err(|_| WorkerError::InvalidEnvelope)?;
        tasks.push(
            TaskBuilder::new(bytes)
                .idempotency_key(job.operation_id.to_string())
                .max_attempts(8)
                .build(),
        );
    }
    if !tasks.is_empty() {
        apalis_postgres::queries::push_tasks(&mut *tx, QUEUE, tasks)
            .await
            .map_err(|_| WorkerError::Storage)?;
        sqlx::query("UPDATE fieldnotes_outbox SET processed_at=clock_timestamp() WHERE id=ANY($1)")
            .bind(&ids)
            .execute(&mut *tx)
            .await
            .map_err(|_| WorkerError::Storage)?;
    }
    tx.commit().await.map_err(|_| WorkerError::Storage)?;
    Ok(ids.len())
}
