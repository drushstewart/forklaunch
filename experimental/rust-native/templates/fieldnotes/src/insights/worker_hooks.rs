//! Application-owned database registration and typed Apalis backend hooks.
use crate::WorkerError;
use ::worker::native::RuntimeError;
use framework_core::persistence::postgres09::{
    PostgresConnectionPolicy, connect_migration_postgres, connect_runtime_postgres,
};
pub async fn migrate() -> Result<(), RuntimeError> {
    let url = std::env::var("DATABASE_URL").map_err(|_| WorkerError::Configuration)?;
    let policy = PostgresConnectionPolicy::from_env(5).map_err(|_| WorkerError::Configuration)?;
    let pool = connect_migration_postgres(&url, policy)
        .await
        .map_err(|_| WorkerError::Storage)?;
    let result = crate::migrations::run(&pool).await;
    pool.close().await;
    Ok(result?)
}
pub async fn run() -> Result<(), RuntimeError> {
    let url = std::env::var("DATABASE_URL").map_err(|_| WorkerError::Configuration)?;
    let policy = PostgresConnectionPolicy::from_env(5).map_err(|_| WorkerError::Configuration)?;
    let pool = connect_runtime_postgres(&url, policy)
        .await
        .map_err(|_| WorkerError::Storage)?;
    let dependencies = crate::registrations::register(pool).await?;
    Ok(crate::domain::services::runtime::run(dependencies).await?)
}
