//! Shared test-only restricted-role `PostgreSQL` lifecycle.
use crate::store::postgres::PostgresAuthStore;
use forklaunch_blueprint_core::AuditContext;
use forklaunch_core::persistence::{TenantContext, encryption::FieldEncryptor};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{future::Future, sync::Arc};

pub(crate) struct Context {
    pub store: Arc<PostgresAuthStore>,
    pub foreign: PostgresAuthStore,
    pub owner: PgPool,
    pub tenant: String,
}

/// Run a proof in its own tenant pair and restricted role, cleaning up even if
/// the spawned proof panics. The ordinary full integration proof is retained;
/// focused suites can use the same role/encryption/RLS setup without rerunning
/// unrelated password and MFA operations.
pub(crate) async fn exercise<F, Fut>(proof: F)
where
    F: FnOnce(Context) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    run(proof).await.unwrap();
}

#[allow(clippy::too_many_lines)]
async fn run<F, Fut>(proof: F) -> Result<(), tokio::task::JoinError>
where
    F: FnOnce(Context) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let url = std::env::var("FORKLAUNCH_TEST_POSTGRES_URL").unwrap();
    let owner = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    PostgresAuthStore::migrate(&owner).await.unwrap();
    let role = format!("fl_auth_test_{}", uuid::Uuid::new_v4().simple());
    for query in [
        format!("CREATE ROLE {role} NOLOGIN NOSUPERUSER NOBYPASSRLS"),
        format!("GRANT USAGE ON SCHEMA fl_better_auth TO {role}"),
        format!(
            "GRANT SELECT,INSERT,UPDATE,DELETE ON fl_better_auth.records,fl_better_auth.unique_claims,fl_better_auth.lookup_claims,fl_better_auth.audit TO {role}"
        ),
        format!("GRANT USAGE ON ALL SEQUENCES IN SCHEMA fl_better_auth TO {role}"),
    ] {
        sqlx::query(&query).execute(&owner).await.unwrap();
    }
    let statement = format!("SET ROLE {role}");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |connection, _| {
            let statement = statement.clone();
            Box::pin(async move {
                sqlx::query(&statement).execute(connection).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    let tenants = vec![format!("{role}-a"), format!("{role}-b")];
    let key = Arc::new(FieldEncryptor::new("owned-totp-encryption-key").unwrap());
    let store = Arc::new(
        PostgresAuthStore::new(
            pool.clone(),
            TenantContext::new(&tenants[0]).unwrap(),
            key.clone(),
            AuditContext::actor("totp-operator", "totp-lifecycle"),
        )
        .await
        .unwrap(),
    );
    let foreign = PostgresAuthStore::new(
        pool.clone(),
        TenantContext::new(&tenants[1]).unwrap(),
        key,
        AuditContext::system("totp-foreign"),
    )
    .await
    .unwrap();
    let context = Context {
        store,
        foreign,
        owner: owner.clone(),
        tenant: tenants[0].clone(),
    };
    let result = tokio::spawn(async move { proof(context).await }).await;
    for table in ["records", "audit"] {
        sqlx::query(&format!(
            "DELETE FROM fl_better_auth.{table} WHERE tenant_id=ANY($1)"
        ))
        .bind(&tenants)
        .execute(&owner)
        .await
        .unwrap();
    }
    for table in ["records", "audit", "unique_claims", "lookup_claims"] {
        let remaining: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM fl_better_auth.{table} WHERE tenant_id=ANY($1)"
        ))
        .bind(&tenants)
        .fetch_one(&owner)
        .await
        .unwrap();
        assert_eq!(remaining, 0, "owned {table} cleanup");
    }
    pool.close().await;
    sqlx::query(&format!("DROP OWNED BY {role}"))
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query(&format!("DROP ROLE {role}"))
        .execute(&owner)
        .await
        .unwrap();
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_roles WHERE rolname=$1")
        .bind(&role)
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(remaining, 0, "owned role cleanup");
    owner.close().await;
    result
}

#[tokio::test]
#[ignore = "requires FORKLAUNCH_TEST_POSTGRES_URL"]
async fn restricted_postgres_proof_panic_still_cleans_owned_state() {
    use crate::store::{AuthStore, AuthTransaction, Model};
    let seen = Arc::new(std::sync::Mutex::new(None));
    let captured = seen.clone();
    let result = run(move |context| async move {
        let mut tx = context.store.begin(true).await.unwrap();
        tx.insert(
            Model::User,
            serde_json::json!({"id":"panic-user","email":"panic@owned.test"}),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        *captured.lock().unwrap() = Some(context.tenant.clone());
        panic!("intentional owned PostgreSQL proof failure");
    })
    .await;
    assert!(result.unwrap_err().is_panic());
    let tenant = seen.lock().unwrap().clone().unwrap();
    let role = tenant.strip_suffix("-a").unwrap();
    let owner = PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("FORKLAUNCH_TEST_POSTGRES_URL").unwrap())
        .await
        .unwrap();
    for table in ["records", "audit", "unique_claims", "lookup_claims"] {
        let count: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM fl_better_auth.{table} WHERE tenant_id=$1"
        ))
        .bind(&tenant)
        .fetch_one(&owner)
        .await
        .unwrap();
        assert_eq!(count, 0, "panic cleanup {table}");
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_roles WHERE rolname=$1")
        .bind(role)
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(count, 0);
    owner.close().await;
}
