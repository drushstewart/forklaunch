//! PostgreSQL connection and schema migration for the IAM stores.
use crate::{domain::services::PostgresIamService, error::Result};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::env;

/// Connect with the restricted runtime role; privileged roles are rejected by the stores.
pub async fn connect() -> Result<PgPool> {
    let url = env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?;
    Ok(PgPoolOptions::new().max_connections(10).connect(&url).await.map_err(|_| "Unable to connect to PostgreSQL")?)
}

/// Apply migrations with a schema-owner `DATABASE_URL`.
pub async fn migrate() -> Result<()> {
    let pool = connect().await?;
    PostgresIamService::migrate(&pool).await?;
    pool.close().await;
    Ok(())
}
