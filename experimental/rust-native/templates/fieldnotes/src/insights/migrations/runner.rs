//! Service-owned queue initialization; Apalis retains ownership of its schema DDL.
use crate::WorkerError;
use apalis_postgres::PostgresStorage;
use sqlx::PgPool;

/// Initialize the pinned Apalis schema, then grant the dedicated runtime role access.
/// The caller supplies a migration-owner connection; normal worker startup never invokes this.
pub async fn run(pool: &PgPool) -> Result<(), WorkerError> {
    PostgresStorage::setup(pool)
        .await
        .map_err(|_| WorkerError::Storage)?;
    sqlx::raw_sql(include_str!("001-worker-grants.sql"))
        .execute(pool)
        .await
        .map_err(|_| WorkerError::Storage)?;
    Ok(())
}
