//! Owns only the generated fixture role and its two tenant scopes.
use sqlx::postgres::PgPoolOptions;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let role = std::env::var("FORKLAUNCH_TEST_ROLE")?;
    assert!(
        role.starts_with("fl_auth_gen_")
            && role.len() < 63
            && role
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("FORKLAUNCH_TEST_POSTGRES_URL")?)
        .await?;
    if std::env::args().any(|argument| argument == "--cleanup") {
        for table in ["records", "audit"] {
            sqlx::query(&format!(
                "DELETE FROM fl_better_auth.{table} WHERE tenant_id=$1 OR tenant_id=$2"
            ))
            .bind(format!("{role}-a"))
            .bind(format!("{role}-b"))
            .execute(&pool)
            .await?;
        }
        sqlx::query(&format!("DROP OWNED BY {role}"))
            .execute(&pool)
            .await?;
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&pool)
            .await?;
    } else {
        sqlx::query(&format!(
            "CREATE ROLE {role} LOGIN NOSUPERUSER NOBYPASSRLS PASSWORD 'parity-test-only'"
        ))
        .execute(&pool)
        .await?;
        sqlx::query(&format!("GRANT USAGE ON SCHEMA fl_better_auth TO {role}"))
            .execute(&pool)
            .await?;
        sqlx::query(&format!(
            "GRANT SELECT,INSERT,UPDATE,DELETE ON ALL TABLES IN SCHEMA fl_better_auth TO {role}"
        ))
        .execute(&pool)
        .await?;
        sqlx::query(&format!(
            "GRANT USAGE ON ALL SEQUENCES IN SCHEMA fl_better_auth TO {role}"
        ))
        .execute(&pool)
        .await?;
    }
    pool.close().await;
    Ok(())
}
