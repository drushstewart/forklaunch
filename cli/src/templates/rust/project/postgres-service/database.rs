//! Connection policy and trusted tenant configuration; never derives identity from request input.
use crate::error::{AppError, Result};
use forklaunch_core::persistence::{PostgresConnectionPolicy, connect_migration_postgres};
use uuid::Uuid;
use std::env::{self, VarError};

pub struct DatabaseConfig {
    pub url: String,
    pub tenant: Uuid,
    pub policy: PostgresConnectionPolicy,
}
fn policy() -> Result<PostgresConnectionPolicy> {
    let allow = match env::var("DATABASE_ALLOW_PLAINTEXT_DEVELOPMENT").as_deref() {
        Err(VarError::NotPresent) | Ok("false") => false,
        Ok("true") => true,
        _ => return Err(AppError::InvalidConfig("Invalid database development transport flag".into())),
    };
    Ok(PostgresConnectionPolicy { allow_plaintext_development: allow, max_connections: 8 })
}
impl DatabaseConfig {
    pub fn from_env() -> Result<Self> {
        let url = env::var("DATABASE_URL").map_err(|_| invalid())?;
        let tenant: Uuid = env::var("TENANT_ID").map_err(|_| invalid())?.parse().map_err(|_| invalid())?;
        if tenant.is_nil() { return Err(invalid()); }
        Ok(Self { url, tenant, policy: policy()? })
    }
}
fn invalid() -> AppError { AppError::InvalidConfig("DATABASE_URL and a trusted non-nil TENANT_ID UUID are required".into()) }

pub async fn migrate() -> Result<()> {
    let url = env::var("MIGRATION_DATABASE_URL").map_err(|_| AppError::InvalidConfig("MIGRATION_DATABASE_URL is required".into()))?;
    let pool = connect_migration_postgres(&url, policy()?).await.map_err(|_| AppError::Storage("Migration connection rejected".into()))?;
    let result = sqlx::migrate!("./migrations").run(&pool).await;
    pool.close().await;
    result.map_err(|_| AppError::Storage("Service migration failed".into()))
}
