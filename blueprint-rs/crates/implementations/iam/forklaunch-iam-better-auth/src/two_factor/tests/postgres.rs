use super::*;
use crate::store::postgres::PostgresAuthStore;
use forklaunch_blueprint_core::AuditContext;
use forklaunch_core::persistence::{TenantContext, encryption::FieldEncryptor};
use sqlx::postgres::PgPoolOptions;

#[tokio::test]
#[ignore = "requires FORKLAUNCH_TEST_POSTGRES_URL"]
#[allow(clippy::too_many_lines)]
async fn restricted_postgres_totp_lifecycle_encryption_tenant_and_atomicity() {
    let url = std::env::var("FORKLAUNCH_TEST_POSTGRES_URL").unwrap();
    let owner = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    PostgresAuthStore::migrate(&owner).await.unwrap();
    let role = format!("fl_totp_{}", uuid::Uuid::new_v4().simple());
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
    let reader = owner.clone();
    let tenant = tenants[0].clone();
    let proof=tokio::spawn(async move {
        lifecycle(store.clone()).await;
        otp::lifecycle(store.clone()).await;
        lifecycle_options::lifecycle(store.clone()).await;
        let secret=STANDARD.encode([7u8;32]);
        let backup="ABCDE-FGHIJ";
        let mut tx=store.begin(true).await.unwrap();
        tx.insert(Model::TwoFactor,json!({"id":"private-factor","userId":"private-factor-owner","secret":secret,"backupCodes":[backup],"verified":true})).await.unwrap();
        tx.commit().await.unwrap();
        let ciphertext:String=sqlx::query_scalar("SELECT ciphertext FROM fl_better_auth.records WHERE tenant_id=$1 AND model='twoFactor' AND id='private-factor'").bind(&tenant).fetch_one(&reader).await.unwrap();
        assert!(!ciphertext.contains(&secret));assert!(!ciphertext.contains(backup));
        let mut tx=foreign.begin(false).await.unwrap();assert!(tx.find_unique(Model::TwoFactor,"userId","private-factor-owner").await.unwrap().is_none());tx.commit().await.unwrap();
        let mut tx=store.begin(true).await.unwrap();assert!(tx.insert(Model::TwoFactor,json!({"id":"duplicate-factor","userId":"private-factor-owner"})).await.is_err());drop(tx);
        let audits:i64=sqlx::query_scalar("SELECT count(*) FROM fl_better_auth.audit WHERE tenant_id=$1 AND model='twoFactor'").bind(&tenant).fetch_one(&reader).await.unwrap();assert!(audits>0);
    }).await;
    for table in ["records", "audit"] {
        sqlx::query(&format!(
            "DELETE FROM fl_better_auth.{table} WHERE tenant_id=ANY($1)"
        ))
        .bind(&tenants)
        .execute(&owner)
        .await
        .unwrap();
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
    owner.close().await;
    proof.unwrap();
}
