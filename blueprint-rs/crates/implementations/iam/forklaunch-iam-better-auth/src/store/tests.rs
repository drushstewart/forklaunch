use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::json;

#[allow(clippy::too_many_lines)]
async fn exercise_transactions<S: AuthStore>(store: &S) {
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"user-1","email":"private@fixture.test","subscription":null}),
    )
    .await
    .unwrap();
    tx.insert(Model::Account, json!({"id":"account-1","userId":"user-1","password":"private-hash","accessToken":"private-provider-token"})).await.unwrap();
    tx.insert(
        Model::Session,
        json!({"id":"session-1","userId":"user-1","token":"private-session-token"}),
    )
    .await
    .unwrap();
    let signing_key = crate::jwt::SigningKey::generate("persisted-key-id").unwrap();
    tx.insert(Model::Jwks, json!({"id":"persisted-key-id","privateKey":STANDARD.encode(signing_key.private_pkcs8()),"publicKey":signing_key.public_jwk()})).await.unwrap();
    tx.commit().await.unwrap();
    {
        let mut tx = store.begin(false).await.unwrap();
        let row = tx
            .get(Model::Jwks, "persisted-key-id")
            .await
            .unwrap()
            .unwrap();
        let restored = crate::jwt::SigningKey::from_pkcs8(
            "persisted-key-id",
            &STANDARD
                .decode(row["privateKey"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(restored.public_jwk(), signing_key.public_jwk());
        tx.commit().await.unwrap();
    }
    {
        let mut tx = store.begin(true).await.unwrap();
        assert!(
            tx.insert(Model::User, json!({"id":"invalid-type","email":42}))
                .await
                .is_err()
        );
        assert!(
            tx.replace(Model::User, json!({"id":"user-1","email":42}))
                .await
                .is_err()
        );
        tx.commit().await.unwrap();
    }
    {
        let mut dropped = store.begin(true).await.unwrap();
        dropped
            .insert(
                Model::User,
                json!({"id":"dropped-user","email":"dropped@fixture.test"}),
            )
            .await
            .unwrap();
        dropped.delete(Model::Session, "session-1").await.unwrap();
    }
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.get(Model::User, "dropped-user").await.unwrap().is_none());
    assert!(tx.get(Model::User, "invalid-type").await.unwrap().is_none());
    assert!(tx.get(Model::Session, "session-1").await.unwrap().is_some());
    assert!(tx.delete(Model::User, "user-1").await.is_err());
    assert!(
        tx.replace(
            Model::User,
            json!({"id":"user-1","email":"changed@fixture.test"})
        )
        .await
        .is_err()
    );
    assert!(
        tx.insert(Model::User, json!({"id":"read-only-user"}))
            .await
            .is_err()
    );
    tx.commit().await.unwrap();
    {
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(
            Model::User,
            json!({"id":"rolled-back","email":"other@fixture.test"}),
        )
        .await
        .unwrap();
        assert!(
            tx.insert(
                Model::User,
                json!({"id":"conflicting","email":"private@fixture.test"})
            )
            .await
            .is_err()
        );
    }
    let mut tx = store.begin(false).await.unwrap();
    let rows = tx.list(Model::User).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["email"], "private@fixture.test");
    assert!(tx.get(Model::User, "rolled-back").await.unwrap().is_none());
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn memory_auth_transactions_are_atomic_unique_and_read_only() {
    exercise_transactions(&memory::MemoryAuthStore::default()).await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "requires FORKLAUNCH_TEST_POSTGRES_URL"]
#[allow(clippy::too_many_lines)]
async fn postgres_auth_encryption_rls_rotation_audits_and_atomicity() {
    use forklaunch_blueprint_core::AuditContext;
    use forklaunch_core::persistence::{TenantContext, encryption::FieldEncryptor};
    use postgres::PostgresAuthStore;
    use sqlx::postgres::PgPoolOptions;
    use std::sync::Arc;
    let url = std::env::var("FORKLAUNCH_TEST_POSTGRES_URL").unwrap();
    let owner = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    PostgresAuthStore::migrate(&owner).await.unwrap();
    PostgresAuthStore::migrate(&owner).await.unwrap();
    let role = format!("fl_auth_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!(
        "CREATE ROLE {role} NOLOGIN NOSUPERUSER NOBYPASSRLS"
    ))
    .execute(&owner)
    .await
    .unwrap();
    sqlx::query(&format!("GRANT USAGE ON SCHEMA fl_better_auth TO {role}"))
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query(&format!("GRANT SELECT,INSERT,UPDATE,DELETE ON fl_better_auth.records,fl_better_auth.unique_claims,fl_better_auth.audit TO {role}")).execute(&owner).await.unwrap();
    sqlx::query(&format!(
        "GRANT USAGE ON ALL SEQUENCES IN SCHEMA fl_better_auth TO {role}"
    ))
    .execute(&owner)
    .await
    .unwrap();
    let set_role = format!("SET ROLE {role}");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |connection, _| {
            let set_role = set_role.clone();
            Box::pin(async move {
                sqlx::query(&set_role).execute(connection).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    let tenant = TenantContext::new(format!("{role}-a")).unwrap();
    let other = TenantContext::new(format!("{role}-b")).unwrap();
    let key = Arc::new(FieldEncryptor::new("auth-fixture-old-key").unwrap());
    assert!(
        PostgresAuthStore::new(
            owner.clone(),
            tenant.clone(),
            key.clone(),
            AuditContext::system("reject-owner")
        )
        .await
        .is_err()
    );
    let store = PostgresAuthStore::new(
        pool.clone(),
        tenant.clone(),
        key.clone(),
        AuditContext::actor("operator", "auth-fixture"),
    )
    .await
    .unwrap();
    assert!(store.with_audit_context(AuditContext::system("")).is_err());
    exercise_transactions(&store).await;
    let other_store = PostgresAuthStore::new(
        pool.clone(),
        other.clone(),
        key.clone(),
        AuditContext::system("other-fixture"),
    )
    .await
    .unwrap();
    let mut tx = other_store.begin(true).await.unwrap();
    assert!(tx.get(Model::User, "user-1").await.unwrap().is_none());
    tx.insert(
        Model::User,
        json!({"id":"user-1","email":"private@fixture.test"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    crate::surfacing::tests::exercise(Arc::new(other_store.clone())).await;
    let isolated_surfacing = crate::surfacing::SurfacingService::new(Arc::new(store.clone()));
    assert_eq!(
        isolated_surfacing
            .surface_role("surface-owner", chrono::Utc::now())
            .await
            .unwrap(),
        Value::Null
    );
    assert!(
        isolated_surfacing
            .surface_permissions("surface-owner", chrono::Utc::now())
            .await
            .unwrap()
            .is_empty()
    );
    crate::keyring::tests::exercise_key_lifecycle(Arc::new(other_store.clone())).await;
    crate::oauth::state::tests::exercise(Arc::new(other_store.clone())).await;
    crate::oauth::google_accounts::tests::exercise(Arc::new(other_store.clone())).await;
    #[cfg(feature = "http")]
    crate::http::tests::tokens::exercise(Arc::new(other_store.clone())).await;
    let google_ciphertexts: Vec<String> =
        sqlx::query_scalar("SELECT ciphertext FROM fl_better_auth.records WHERE tenant_id=$1")
            .bind(other.id())
            .fetch_all(&owner)
            .await
            .unwrap();
    assert!(!google_ciphertexts.is_empty());
    for ciphertext in google_ciphertexts {
        for secret in [
            "native-google@fixture.test",
            "private-google-id-token",
            "private-google-refresh-token",
            "updated-private-access",
            "native-updated-access",
            "opaque-not-expanded",
            "native-rotated-refresh",
        ] {
            assert!(!ciphertext.contains(secret));
        }
    }

    let encrypted: Vec<String> =
        sqlx::query_scalar("SELECT ciphertext FROM fl_better_auth.records WHERE tenant_id=$1")
            .bind(tenant.id())
            .fetch_all(&owner)
            .await
            .unwrap();
    assert_eq!(encrypted.len(), 4);
    for value in encrypted {
        for secret in [
            "private@fixture.test",
            "private-hash",
            "private-provider-token",
            "private-session-token",
            "privateKey",
        ] {
            assert!(!value.contains(secret));
        }
    }
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM fl_better_auth.audit WHERE tenant_id=$1 AND actor_id='operator'",
    )
    .bind(tenant.id())
    .fetch_one(&owner)
    .await
    .unwrap();
    assert_eq!(
        audit, 4,
        "rolled-back transactions must not leak audit records"
    );
    let unscoped: i64 = sqlx::query_scalar("SELECT count(*) FROM fl_better_auth.records")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        unscoped, 0,
        "tenant context must not leak between pooled transactions"
    );
    let rotated = PostgresAuthStore::new(
        pool.clone(),
        tenant.clone(),
        Arc::new(
            FieldEncryptor::new("auth-fixture-new-key")
                .unwrap()
                .with_previous_keys(&["auth-fixture-old-key".into()])
                .unwrap(),
        ),
        AuditContext::system("rotation-fixture"),
    )
    .await
    .unwrap();
    crate::sessions::tests::exercise_sessions(Arc::new(rotated.clone())).await;
    crate::accounts::tests::exercise_credentials(Arc::new(rotated.clone())).await;
    crate::organizations::tests::exercise_organizations(Arc::new(rotated.clone())).await;
    let cleanup_tenants = vec![tenant.id().to_owned(), other.id().to_owned()];
    #[cfg(feature = "http")]
    let cleanup_tenants = {
        // Raw storage fixtures intentionally contain incomplete session/JWKS
        // records. Give the live auth service its own complete tenant graph.
        let http_tenant = TenantContext::new(format!("{role}-http")).unwrap();
        let http_store = PostgresAuthStore::new(
            pool.clone(),
            http_tenant.clone(),
            key.clone(),
            AuditContext::system("http-fixture"),
        )
        .await
        .unwrap();
        crate::http::tests::exercise_http(Arc::new(http_store)).await;
        let mut tenants = cleanup_tenants;
        tenants.push(http_tenant.id().to_owned());
        tenants
    };
    let mut tx = rotated.begin(true).await.unwrap();
    assert_eq!(
        tx.get(Model::Account, "account-1").await.unwrap().unwrap()["password"],
        "private-hash"
    );
    assert!(
        tx.insert(
            Model::User,
            json!({"id":"duplicate-after-rotation","email":"private@fixture.test"})
        )
        .await
        .is_err()
    );
    drop(tx);
    // Valid ciphertext cannot be transplanted between records or model families.
    sqlx::query("UPDATE fl_better_auth.records SET ciphertext=(SELECT ciphertext FROM fl_better_auth.records WHERE tenant_id=$1 AND model='account' AND id='account-1') WHERE tenant_id=$1 AND model='session' AND id='session-1'").bind(tenant.id()).execute(&owner).await.unwrap();
    let mut tx = rotated.begin(false).await.unwrap();
    assert!(tx.get(Model::Session, "session-1").await.is_err());
    tx.commit().await.unwrap();
    for table in ["fl_better_auth.records", "fl_better_auth.audit"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE tenant_id=ANY($1)"))
            .bind(&cleanup_tenants)
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
}
