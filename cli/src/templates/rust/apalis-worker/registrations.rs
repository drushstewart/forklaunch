use std::env;
use forklaunch_core::persistence::postgres09::{PostgresConnectionPolicy,connect_migration_postgres,connect_runtime_postgres};
use forklaunch_worker_interface::native::RuntimeError;
use sqlx::PgPool;
pub async fn pool(migrate:bool)->Result<PgPool,RuntimeError> {
    let url=env::var("DATABASE_URL")?;
    let policy=PostgresConnectionPolicy::from_env(5)?;
    Ok(if migrate {connect_migration_postgres(&url,policy).await?} else {connect_runtime_postgres(&url,policy).await?})
}
