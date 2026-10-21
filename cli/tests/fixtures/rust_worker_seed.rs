use sqlx::postgres::PgPoolOptions;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let role = std::env::var("FORKLAUNCH_TEST_ROLE")?;
    assert!(role.starts_with("fl_worker_") && role.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_'));
    let pool = PgPoolOptions::new().max_connections(1).connect(&std::env::var("FORKLAUNCH_TEST_POSTGRES_URL")?).await?;
    if std::env::args().any(|arg| arg == "--cleanup") {
        sqlx::query("DELETE FROM fl_worker.jobs WHERE tenant_id=$1 OR tenant_id=$2")
            .bind(std::env::var("FORKLAUNCH_TEST_TENANT_A")?).bind(std::env::var("FORKLAUNCH_TEST_TENANT_B")?).execute(&pool).await?;
        sqlx::query(&format!("DROP OWNED BY {role}")).execute(&pool).await?;
        sqlx::query(&format!("DROP ROLE {role}")).execute(&pool).await?;
    } else {
        for statement in [
            format!("CREATE ROLE {role} LOGIN PASSWORD 'parity-test-only' NOSUPERUSER NOBYPASSRLS"),
            format!("GRANT USAGE ON SCHEMA fl_worker TO {role}"),
            format!("GRANT SELECT,INSERT,UPDATE,DELETE ON fl_worker.jobs TO {role}"),
            format!("GRANT USAGE ON ALL SEQUENCES IN SCHEMA fl_worker TO {role}"),
        ] { sqlx::query(&statement).execute(&pool).await?; }
    }
    pool.close().await;
    Ok(())
}
