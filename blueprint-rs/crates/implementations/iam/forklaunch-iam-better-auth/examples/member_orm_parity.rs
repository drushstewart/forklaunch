//! Actual encrypted `PostgreSQL` member-query bridge for the configured ORM oracle.
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::AuditContext;
use forklaunch_core::persistence::{TenantContext, encryption::FieldEncryptor};
use forklaunch_iam_better_auth::{
    http::AuthHttp,
    store::{AuthStore, AuthTransaction, MemberQueryProfile, Model, postgres::PostgresAuthStore},
};
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use std::{
    io::{self, BufRead},
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
};
use tower::ServiceExt;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("FORKLAUNCH_TEST_POSTGRES_URL")?;
    let owner = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await?;
    PostgresAuthStore::migrate(&owner).await?;
    let role = format!("fl_member_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!(
        "CREATE ROLE {role} LOGIN NOSUPERUSER NOBYPASSRLS PASSWORD 'parity-test-only'"
    ))
    .execute(&owner)
    .await?;
    for grant in [
        format!("GRANT USAGE ON SCHEMA fl_better_auth TO {role}"),
        format!(
            "GRANT SELECT,INSERT,UPDATE,DELETE ON ALL TABLES IN SCHEMA fl_better_auth TO {role}"
        ),
        format!("GRANT USAGE ON ALL SEQUENCES IN SCHEMA fl_better_auth TO {role}"),
    ] {
        sqlx::query(&grant).execute(&owner).await?;
    }
    let result = run(&url, &role).await;
    for table in ["records", "audit"] {
        sqlx::query(&format!(
            "DELETE FROM fl_better_auth.{table} WHERE tenant_id=$1"
        ))
        .bind(&role)
        .execute(&owner)
        .await?;
    }
    sqlx::query(&format!("DROP OWNED BY {role}"))
        .execute(&owner)
        .await?;
    sqlx::query(&format!("DROP ROLE {role}"))
        .execute(&owner)
        .await?;
    owner.close().await;
    result
}
async fn run(url: &str, role: &str) -> Result<()> {
    let mut url = url::Url::parse(url)?;
    url.set_username(role)
        .map_err(|()| "invalid fixture username")?;
    url.set_password(Some("parity-test-only"))
        .map_err(|()| "invalid fixture password")?;
    let timezone = std::env::var("FORKLAUNCH_TEST_MEMBER_TIMEZONE").ok();
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .after_connect(move |connection, _| {
            let timezone = timezone.clone();
            Box::pin(async move {
                if let Some(timezone) = timezone {
                    sqlx::query("SELECT set_config('TimeZone',$1,false)")
                        .bind(&timezone)
                        .execute(&mut *connection)
                        .await?;
                    let actual: String = sqlx::query_scalar("SHOW TimeZone")
                        .fetch_one(&mut *connection)
                        .await?;
                    assert_eq!(actual, timezone);
                    eprintln!("Native session TimeZone: {actual}");
                }
                Ok(())
            })
        })
        .connect(url.as_str())
        .await?;
    let store = Arc::new(
        PostgresAuthStore::new(
            pool.clone(),
            TenantContext::new(role)?,
            Arc::new(FieldEncryptor::new("member-projection-fixture-key")?),
            AuditContext::actor("member-oracle", "member-oracle"),
        )
        .await?
        .with_member_query_profile(MemberQueryProfile::Postgres),
    );
    let clock = Arc::new(AtomicI64::new(0));
    let server_clock = clock.clone();
    let router = AuthHttp::new(
        store.clone(),
        "http://localhost:3000".into(),
        b"better-auth-protocol-fixture-secret-32-characters".to_vec(),
        vec![],
    )?
    .with_clock(move || {
        DateTime::<Utc>::from_timestamp_millis(server_clock.load(Ordering::SeqCst)).unwrap()
    })
    .router();
    for line in io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line?)?;
        clock.store(input["now"].as_i64().unwrap(), Ordering::SeqCst);
        let output = if input["op"] == "replace-seed" {
            let mut tx = store.begin(true).await?;
            for (field, model) in [
                ("users", Model::User),
                ("accounts", Model::Account),
                ("sessions", Model::Session),
                ("organizations", Model::Organization),
                ("members", Model::Member),
            ] {
                for row in input[field].as_array().into_iter().flatten() {
                    tx.insert(model, row.clone()).await?;
                }
            }
            tx.commit().await?;
            json!({"ok":true})
        } else {
            let mut request = Request::builder()
                .method(input["method"].as_str().unwrap())
                .uri(input["path"].as_str().unwrap())
                .header("host", "localhost:3000");
            for (key, value) in input["headers"].as_object().unwrap() {
                request = request.header(key, value.as_str().unwrap());
            }
            let response = router.clone().oneshot(request.body(Body::empty())?).await?;
            let status = response.status().as_u16();
            let bytes = to_bytes(response.into_body(), 1024 * 1024).await?;
            let body: Value = serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)));
            json!({"status":status,"body":body})
        };
        println!("{output}");
    }
    projection_boundaries(&store).await?;
    drop(router);
    drop(store);
    pool.close().await;
    Ok(())
}

// Kept outside the response stream so the oracle still compares only HTTP data.
async fn projection_boundaries(store: &PostgresAuthStore) -> Result<()> {
    let mut tx = store.begin(true).await?;
    let uppercase = uuid::Uuid::new_v4().to_string().to_uppercase();
    let member = |id: String, org: &str, role: Value| json!({"id":id,"organizationId":org,"userId":"opaque-user","role":role,"createdAt":"2026-01-01T00:00:00Z"});
    tx.insert(
        Model::Member,
        member(uppercase.clone(), "projection-small", json!("owner")),
    )
    .await?;
    tx.insert(
        Model::Member,
        member(
            uuid::Uuid::new_v4().to_string(),
            "projection-large",
            json!("x".repeat(1024 * 1024)),
        ),
    )
    .await?;
    let selected = tx
        .query_members(
            "projection-small",
            &json!({"filterField":"id","filterValue":uppercase.to_lowercase()}),
        )
        .await?
        .unwrap();
    assert_eq!(selected.total, 1);
    assert_eq!(
        selected.records[0]["id"], uppercase,
        "SQL UUID normalization must not change opaque stored hydration keys"
    );
    let error = tx
        .query_members("projection-large", &json!({"sortBy":"role"}))
        .await
        .unwrap_err();
    assert_eq!(error.code, "better_auth_member_query_limit");
    for _ in 0..4097 {
        tx.insert(
            Model::Member,
            member(
                uuid::Uuid::new_v4().to_string(),
                "projection-count",
                json!("member"),
            ),
        )
        .await?;
    }
    let window = tx
        .find_window(
            Model::Member,
            "organizationId",
            "projection-count",
            4090,
            20,
        )
        .await?;
    assert_eq!(window.total, 4097);
    assert_eq!(window.records.len(), 7);
    let error = tx
        .query_members(
            "projection-count",
            &json!({"filterField":"role","filterValue":"absent"}),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.code, "better_auth_member_query_limit",
        "overflow must fail even when no candidate would match"
    );
    drop(tx); // All synthetic graph rows and audits roll back together.
    let mut tx = store.begin(false).await?;
    assert_eq!(
        tx.find_window(Model::Member, "organizationId", "projection-count", 0, 1)
            .await?
            .total,
        0
    );
    tx.commit().await?;
    eprintln!(
        "PostgreSQL projection boundaries: UUID spelling, organization scope, byte/count budgets, indexed large page and rollback passed"
    );
    Ok(())
}
