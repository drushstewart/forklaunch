//! Restricted-role provisioning and persisted-state checks for the generated module fixture.
use sqlx::postgres::PgPoolOptions;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let role = std::env::var("FORKLAUNCH_TEST_ROLE")?;
    assert!(role.starts_with("fl_ecom_") && role.len() < 63 && role.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
    let pool = PgPoolOptions::new().max_connections(2).connect(&std::env::var("FORKLAUNCH_TEST_POSTGRES_URL")?).await?;
    let tenants = vec![format!("{role}-a"), format!("{role}-b")];
    if std::env::args().any(|arg| arg == "--cleanup") {
        for table in ["fl_ecommerce.records", "fl_ecommerce.pii_read_audit", "fl_ecommerce.inventory_event_receipts", "fl_worker.jobs"] {
            sqlx::query(&format!("DELETE FROM {table} WHERE tenant_id=ANY($1)")).bind(&tenants).execute(&pool).await?;
        }
        sqlx::raw_sql(&format!("DROP OWNED BY {role}; DROP ROLE {role}")).execute(&pool).await?;
    } else if std::env::args().any(|arg| arg == "--inspect") {
        let audits: Vec<(String,)> = sqlx::query_as("SELECT actor_id FROM fl_ecommerce.pii_read_audit WHERE tenant_id=ANY($1)").bind(&tenants).fetch_all(&pool).await?;
        assert!(!audits.is_empty());
        assert!(audits.iter().all(|(actor,)| actor == "service:default"));
        let rows: Vec<(serde_json::Value,)> = sqlx::query_as("SELECT data FROM fl_ecommerce.records WHERE tenant_id=ANY($1)").bind(&tenants).fetch_all(&pool).await?;
        assert!(!rows.is_empty());
        for (data,) in rows { let text = data.to_string(); assert!(!text.contains("Private Street")); assert!(!text.contains("fixture-client-secret")); }
        let receipts: (i64,) = sqlx::query_as("SELECT count(*) FROM fl_ecommerce.inventory_event_receipts WHERE tenant_id=ANY($1)").bind(&tenants).fetch_one(&pool).await?;
        assert!(receipts.0 >= 2);
        println!("PASS generated ecommerce encrypted addresses, audited actor identity, secret exclusion and durable worker receipts");
    } else {
        sqlx::raw_sql(&format!("CREATE ROLE {role} LOGIN NOSUPERUSER NOBYPASSRLS PASSWORD 'parity-test-only'; GRANT USAGE ON SCHEMA fl_ecommerce,fl_worker TO {role}; GRANT SELECT,INSERT,UPDATE,DELETE ON fl_ecommerce.records,fl_worker.jobs TO {role}; GRANT SELECT,INSERT ON fl_ecommerce.inventory_event_receipts TO {role}; GRANT INSERT ON fl_ecommerce.pii_read_audit TO {role}; GRANT USAGE ON ALL SEQUENCES IN SCHEMA fl_worker TO {role}")).execute(&pool).await?;
        sqlx::query("INSERT INTO fl_ecommerce.records(tenant_id,kind,id,data) VALUES ($1,'inventory','fixture-stock',$2)")
            .bind(&tenants[0]).bind(serde_json::json!({"id":"fixture-stock","variantId":"variant","stock":8,"createdAt":"2026-01-01T00:00:00.000Z","updatedAt":"2026-01-01T00:00:00.000Z"})).execute(&pool).await?;
    }
    pool.close().await;
    Ok(())
}
