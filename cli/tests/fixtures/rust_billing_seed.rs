//! Test-only role provisioning and storage inspection for generated billing modules.
use sqlx::postgres::PgPoolOptions;
#[tokio::main]
async fn main()->Result<(),Box<dyn std::error::Error>> {
    let role=std::env::var("FORKLAUNCH_TEST_ROLE")?;
    assert!(role.starts_with("fl_billing_")&&role.len()<63&&role.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'_'));
    let pool=PgPoolOptions::new().max_connections(2).connect(&std::env::var("FORKLAUNCH_TEST_POSTGRES_URL")?).await?;
    let tenants=vec![format!("{role}-a"),format!("{role}-b")];
    if std::env::args().any(|arg|arg=="--cleanup") {
        for table in ["records","sensitive_read_audit","webhook_receipts","webhook_attempts","webhook_reconciliation_audit","platform_event_nonces"] {sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM fl_billing.{table} WHERE tenant_id=ANY($1)"))).bind(&tenants).execute(&pool).await?;}
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP OWNED BY {role};DROP ROLE {role}"))).execute(&pool).await?;
    } else if std::env::args().any(|arg|arg=="--inspect") {
        let actors:Vec<(Option<String>,)>=sqlx::query_as("SELECT actor_id FROM fl_billing.sensitive_read_audit WHERE tenant_id=ANY($1)").bind(&tenants).fetch_all(&pool).await?;
        assert!(!actors.is_empty());assert!(actors.iter().all(|(actor,)|matches!(actor.as_deref(),Some("service:default"|"billing-admin"|"provider:stripe"|"platform:payments"))));
        let rows:Vec<(String,)>=sqlx::query_as("SELECT data::text FROM fl_billing.records WHERE tenant_id=ANY($1)").bind(&tenants).fetch_all(&pool).await?;
        for (row,) in rows {assert!(!row.contains("private-provider-value"));assert!(!row.contains("provider.example"));}
        println!("PASS generated billing encrypted provider fields/session URIs and verified read actors");
    } else {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE ROLE {role} LOGIN NOSUPERUSER NOBYPASSRLS PASSWORD 'parity-test-only';GRANT USAGE ON SCHEMA fl_billing TO {role};GRANT SELECT,INSERT,UPDATE,DELETE ON fl_billing.records TO {role};GRANT INSERT ON fl_billing.sensitive_read_audit TO {role};GRANT SELECT,INSERT ON fl_billing.webhook_receipts TO {role};GRANT SELECT,INSERT,DELETE ON fl_billing.webhook_attempts TO {role};GRANT INSERT ON fl_billing.webhook_reconciliation_audit TO {role};GRANT SELECT,INSERT,DELETE ON fl_billing.platform_event_nonces TO {role}"))).execute(&pool).await?;
    }
    pool.close().await;Ok(())
}
