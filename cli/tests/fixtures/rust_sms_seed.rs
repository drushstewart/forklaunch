//! Test-only provisioning for generated SMS modules. Never uses real providers.
use sqlx::postgres::PgPoolOptions;
#[tokio::main]
async fn main() -> Result<(),Box<dyn std::error::Error>> {
    let role=std::env::var("FORKLAUNCH_TEST_ROLE")?;
    assert!(role.starts_with("fl_sms_")&&role.len()<63&&role.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'_'));
    let pool=PgPoolOptions::new().max_connections(2).connect(&std::env::var("FORKLAUNCH_TEST_POSTGRES_URL")?).await?;
    let tenants=vec![format!("{role}-a"),format!("{role}-b")];
    if std::env::args().any(|a|a=="--cleanup") {
        for table in ["sms_records","pii_read_audit"] {
            sqlx::query(&format!("DELETE FROM fl_messaging.{table} WHERE tenant_id=ANY($1)")).bind(&tenants).execute(&pool).await?;
        }
        sqlx::raw_sql(&format!("DROP OWNED BY {role}; DROP ROLE {role}")).execute(&pool).await?;
    } else if std::env::args().any(|a|a=="--inspect") {
        let operations:Vec<(String,String)> = sqlx::query_as("SELECT operation,actor_id FROM fl_messaging.pii_read_audit WHERE tenant_id=ANY($1)").bind(&tenants).fetch_all(&pool).await?;
        assert!(operations.iter().any(|(op,actor)|op=="delivery_callback"&&actor=="provider:twilio"));
        assert!(operations.iter().any(|(op,actor)|op=="compliance_export"&&actor=="sms-system-user"));
        assert!(operations.iter().any(|(op,actor)|op=="get_sms_record"&&actor=="service:default"));
        let rows:Vec<(String,)> = sqlx::query_as("SELECT encrypted_record FROM fl_messaging.sms_records WHERE tenant_id=ANY($1)").bind(&tenants).fetch_all(&pool).await?;
        for (ciphertext,) in rows {assert!(ciphertext.starts_with("v4:"));assert!(!ciphertext.contains("private SMS"));}
        println!("PASS generated messaging audited reads, callback/compliance actor identity and encrypted persisted records");
    } else {
        sqlx::raw_sql(&format!("CREATE ROLE {role} LOGIN NOSUPERUSER NOBYPASSRLS PASSWORD 'parity-test-only'; GRANT USAGE ON SCHEMA fl_messaging TO {role}; GRANT SELECT,INSERT,UPDATE,DELETE ON fl_messaging.sms_records TO {role}; GRANT INSERT ON fl_messaging.pii_read_audit TO {role}")).execute(&pool).await?;
    }
    pool.close().await;Ok(())
}
