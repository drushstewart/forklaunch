use super::*;
use forklaunch_core::persistence::encryption::FieldEncryptor;
use forklaunch_iam_base::browser_sessions::PostgresCookieStore;
use tower_sessions::{
    SessionStore,
    cookie::time::{Duration, OffsetDateTime},
    session::{Id, Record},
};

async fn store(
    fixture: &Fixture,
    tenant: &str,
    namespace: &str,
    key: FieldEncryptor,
) -> PostgresCookieStore {
    PostgresCookieStore::new(
        fixture.pool.clone(),
        TenantContext::new(format!("{}-{tenant}", fixture.role)).unwrap(),
        namespace,
        Arc::new(key),
    )
    .await
    .unwrap()
}
fn key() -> FieldEncryptor {
    FieldEncryptor::new("cookie-fixture-key-only").unwrap()
}
fn record() -> Record {
    Record {
        id: Id::default(),
        expiry_date: OffsetDateTime::now_utc() + Duration::minutes(30),
        data: [("credential".to_owned(), json!("private-bearer-sentinel"))].into(),
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // Exercise one isolated store through corruption and recovery boundaries.
#[ignore = "requires FORKLAUNCH_TEST_POSTGRES_URL; exercised in parity CI"]
async fn postgres_iam_http_encrypted_cookie_store_isolation_rotation_and_stale_writes() {
    let fixture = Fixture::new().await;
    let tenant = TenantContext::new(format!("{}-a", fixture.role)).unwrap();
    assert!(
        PostgresCookieStore::new(
            fixture.owner.clone(),
            tenant.clone(),
            "browser",
            Arc::new(key())
        )
        .await
        .is_err()
    );
    assert!(
        PostgresCookieStore::new(fixture.pool.clone(), tenant.clone(), "", Arc::new(key()))
            .await
            .is_err()
    );
    let first = store(&fixture, "a", "browser", key()).await;
    let other = store(&fixture, "b", "browser", key()).await;
    let namespace = store(&fixture, "a", "different", key()).await;
    let mut original = record();
    first.create(&mut original).await.unwrap();
    assert_eq!(
        first.load(&original.id).await.unwrap(),
        Some(original.clone())
    );
    assert!(other.load(&original.id).await.unwrap().is_none());
    assert!(namespace.load(&original.id).await.unwrap().is_none());
    assert!(format!("{first:?}").contains("PostgresCookieStore"));
    assert!(!format!("{first:?}").contains("cookie-fixture-key"));
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id_hash,encrypted_record FROM fl_iam_browser_sessions WHERE tenant_id=$1",
    )
    .bind(tenant.id())
    .fetch_all(&fixture.owner)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0.len(), 64);
    assert!(!rows[0].0.contains(&original.id.to_string()));
    assert!(!rows[0].1.contains("private-bearer-sentinel"));
    assert!(!rows[0].1.contains(&original.id.to_string()));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM fl_iam_browser_sessions")
            .fetch_one(&fixture.pool)
            .await
            .unwrap(),
        0
    );
    let restarted = store(&fixture, "a", "browser", key()).await;
    assert_eq!(
        restarted.load(&original.id).await.unwrap(),
        Some(original.clone())
    );
    // A caller-supplied ID collision cannot overwrite the existing credential.
    let mut collision = original.clone();
    collision
        .data
        .insert("credential".to_owned(), json!("different-sentinel"));
    first.create(&mut collision).await.unwrap();
    assert_ne!(collision.id, original.id);
    assert_eq!(
        first.load(&original.id).await.unwrap(),
        Some(original.clone())
    );
    // Ciphertext is bound to the record ID, namespace, tenant and indexed expiry.
    sqlx::query(
        "UPDATE fl_iam_browser_sessions SET encrypted_record=$2 WHERE tenant_id=$1 AND id_hash<>$3",
    )
    .bind(tenant.id())
    .bind(&rows[0].1)
    .bind(&rows[0].0)
    .execute(&fixture.owner)
    .await
    .unwrap();
    let error = first.load(&collision.id).await.unwrap_err();
    assert!(!error.to_string().contains("sentinel"));
    first.delete(&collision.id).await.unwrap();
    let mut separate = original.clone();
    namespace.create(&mut separate).await.unwrap();
    sqlx::query("UPDATE fl_iam_browser_sessions SET encrypted_record=$2 WHERE tenant_id=$1 AND namespace='different'")
        .bind(tenant.id()).bind(&rows[0].1).execute(&fixture.owner).await.unwrap();
    assert!(namespace.load(&original.id).await.is_err());
    namespace.delete(&original.id).await.unwrap();
    assert!(first.load(&original.id).await.unwrap().is_some());
    sqlx::query("UPDATE fl_iam_browser_sessions SET expires_at=expires_at+1 WHERE tenant_id=$1")
        .bind(tenant.id())
        .execute(&fixture.owner)
        .await
        .unwrap();
    assert!(first.load(&original.id).await.is_err());
    sqlx::query("UPDATE fl_iam_browser_sessions SET expires_at=expires_at-1 WHERE tenant_id=$1")
        .bind(tenant.id())
        .execute(&fixture.owner)
        .await
        .unwrap();
    let rotated = store(
        &fixture,
        "a",
        "browser",
        FieldEncryptor::new("new-cookie-key")
            .unwrap()
            .with_previous_keys(&["cookie-fixture-key-only".to_owned()])
            .unwrap(),
    )
    .await;
    assert!(rotated.load(&original.id).await.unwrap().is_some());
    original.data.insert("updated".to_owned(), json!(true));
    rotated.save(&original).await.unwrap();
    assert!(first.load(&original.id).await.is_err());
    assert_eq!(
        rotated.load(&original.id).await.unwrap(),
        Some(original.clone())
    );
    rotated.delete(&original.id).await.unwrap();
    assert!(rotated.save(&original).await.is_err());
    assert!(rotated.load(&original.id).await.unwrap().is_none());
    let mut expired = record();
    expired.expiry_date = OffsetDateTime::now_utc() - Duration::seconds(1);
    first.create(&mut expired).await.unwrap();
    assert!(first.load(&expired.id).await.unwrap().is_none());
    expired.expiry_date = OffsetDateTime::now_utc() + Duration::minutes(30);
    assert!(first.save(&expired).await.is_err());
    assert_eq!(first.purge_expired().await.unwrap(), 1);
    let mut oversized = record();
    oversized
        .data
        .insert("oversized".to_owned(), json!("x".repeat(70 * 1024)));
    assert!(first.create(&mut oversized).await.is_err());
    assert!(first.load(&oversized.id).await.unwrap().is_none());
    drop((first, other, namespace, restarted, rotated));
    fixture.close().await;
}
