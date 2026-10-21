//! Restricted-role provisioning and persisted-state checks for the generated module fixture.
use sqlx::postgres::PgPoolOptions;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let role = std::env::var("FORKLAUNCH_TEST_ROLE")?;
    assert!(
        role.starts_with("fl_ecom_")
            && role.len() < 63
            && role.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    );
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("FORKLAUNCH_TEST_POSTGRES_URL")?)
        .await?;
    let tenants = vec![format!("{role}-a"), format!("{role}-b")];
    if std::env::args().any(|arg| arg == "--cleanup") {
        for table in [
            "fl_ecommerce.records",
            "fl_ecommerce.pii_read_audit",
            "fl_ecommerce.inventory_event_receipts",
            "fl_ecommerce.webhook_events",
            "fl_ecommerce.payment_reconciliation_audit",
            "fl_ecommerce.payment_cancellation_audit",
            "fl_worker.jobs",
        ] {
            sqlx::query(&format!("DELETE FROM {table} WHERE tenant_id=ANY($1)"))
                .bind(&tenants)
                .execute(&pool)
                .await?;
        }
        sqlx::raw_sql(&format!("DROP OWNED BY {role}; DROP ROLE {role}"))
            .execute(&pool)
            .await?;
    } else if std::env::args().any(|arg| arg == "--wait-checkout-lock") {
        // Prove both checkout requests overlap at the real database boundary.
        // A fixed provider delay cannot guarantee overlap on a loaded CI host.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(8);
        loop {
            let waiting: (i64,) = sqlx::query_as(
                "SELECT count(*) FROM pg_locks lock JOIN pg_stat_activity activity ON activity.pid=lock.pid WHERE lock.locktype='advisory' AND NOT lock.granted AND activity.usename=$1 AND activity.datname=current_database()",
            )
            .bind(&role)
            .fetch_one(&pool)
            .await?;
            if waiting.0 > 0 {
                println!("PASS concurrent checkout waits on its database advisory lock");
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "Second checkout did not reach the database lock while first checkout was pricing"
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    } else if std::env::args().any(|arg| arg == "--inspect") {
        let audits: Vec<(String,)> = sqlx::query_as(
            "SELECT actor_id FROM fl_ecommerce.pii_read_audit WHERE tenant_id=ANY($1)",
        )
        .bind(&tenants)
        .fetch_all(&pool)
        .await?;
        assert!(!audits.is_empty());
        for provider in ["provider:stripe", "provider:paypal"] {
            assert!(audits.iter().any(|(actor,)| actor == provider));
        }
        assert!(audits.iter().all(|(actor,)| {
            ["service:default", "provider:stripe", "provider:paypal"].contains(&actor.as_str())
        }));
        let rows: Vec<(serde_json::Value,)> =
            sqlx::query_as("SELECT data FROM fl_ecommerce.records WHERE tenant_id=ANY($1)")
                .bind(&tenants)
                .fetch_all(&pool)
                .await?;
        assert!(!rows.is_empty());
        for (data,) in rows {
            let text = data.to_string();
            assert!(!text.contains("Private Street"));
            assert!(!text.contains("fixture-client-secret"));
        }
        let receipts: (i64,) = sqlx::query_as(
            "SELECT count(*) FROM fl_ecommerce.inventory_event_receipts WHERE tenant_id=ANY($1)",
        )
        .bind(&tenants)
        .fetch_one(&pool)
        .await?;
        assert!(receipts.0 >= 2);
        let invalid_receipts: (i64,) = sqlx::query_as("SELECT count(*) FROM fl_ecommerce.webhook_events WHERE tenant_id=ANY($1) AND provider_event_id LIKE '%invalid%'").bind(&tenants).fetch_one(&pool).await?;
        assert_eq!(
            invalid_receipts.0, 0,
            "Unverified callbacks must not create receipts"
        );
        let processed_receipts: (i64,) = sqlx::query_as("SELECT count(*) FROM fl_ecommerce.webhook_events WHERE tenant_id=ANY($1) AND processed").bind(&tenants).fetch_one(&pool).await?;
        assert!(processed_receipts.0 >= 10);
        println!(
            "PASS generated ecommerce encrypted addresses, audited actor identity, secret exclusion and durable worker receipts"
        );
    } else {
        sqlx::raw_sql(&format!("CREATE ROLE {role} LOGIN NOSUPERUSER NOBYPASSRLS PASSWORD 'parity-test-only'; GRANT USAGE ON SCHEMA fl_ecommerce,fl_worker TO {role}; GRANT SELECT,INSERT,UPDATE,DELETE ON fl_ecommerce.order_retention,fl_ecommerce.records,fl_worker.jobs TO {role}; GRANT SELECT,INSERT ON fl_ecommerce.inventory_event_receipts TO {role}; GRANT SELECT,INSERT,UPDATE ON fl_ecommerce.webhook_events TO {role}; GRANT INSERT ON fl_ecommerce.pii_read_audit TO {role}; GRANT SELECT,INSERT ON fl_ecommerce.payment_reconciliation_audit,fl_ecommerce.payment_cancellation_audit TO {role}; GRANT USAGE ON ALL SEQUENCES IN SCHEMA fl_worker TO {role}")).execute(&pool).await?;
        sqlx::query("INSERT INTO fl_ecommerce.records(tenant_id,kind,id,data) VALUES ($1,'inventory','fixture-stock',$2)")
            .bind(&tenants[0]).bind(serde_json::json!({"id":"fixture-stock","variantId":"variant","stock":8,"createdAt":"2026-01-01T00:00:00.000Z","updatedAt":"2026-01-01T00:00:00.000Z"})).execute(&pool).await?;
    }
    pool.close().await;
    Ok(())
}
