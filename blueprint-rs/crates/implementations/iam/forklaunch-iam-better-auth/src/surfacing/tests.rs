use super::*;
use serde_json::json;

#[tokio::test]
async fn newest_active_session_scopes_opaque_roles_and_permissions() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}
pub(crate) async fn exercise<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::parse_from_rfc3339("2027-01-15T08:00:00.000Z")
        .unwrap()
        .with_timezone(&Utc);
    let mut tx = store.begin(true).await.unwrap();
    for row in [
        json!({"id":"surface-old","userId":"surface-owner","activeOrganizationId":"surface-one","createdAt":"2027-01-15T07:00:00Z","expiresAt":"2027-01-16T08:00:00Z"}),
        json!({"id":"surface-expired","userId":"surface-owner","activeOrganizationId":"surface-two","createdAt":"2027-01-15T07:30:00Z","expiresAt":"2027-01-15T08:00:00Z"}),
        json!({"id":"surface-unscoped","userId":"surface-owner","activeOrganizationId":null,"createdAt":"2027-01-15T07:40:00Z","expiresAt":"2027-01-16T08:00:00Z"}),
        json!({"id":"surface-foreign","userId":"surface-other","activeOrganizationId":"surface-two","createdAt":"2027-01-15T07:50:00Z","expiresAt":"2027-01-16T08:00:00Z"}),
    ] {
        tx.insert(Model::Session, row).await.unwrap();
    }
    for row in [
        json!({"id":"surface-member-one","userId":"surface-owner","organizationId":"surface-one","role":"owner,admin"}),
        json!({"id":"surface-member-two","userId":"surface-owner","organizationId":"surface-two","role":"member"}),
    ] {
        tx.insert(Model::Member, row).await.unwrap();
    }
    for row in [
        json!({"id":"surface-role-one","organizationId":"surface-one","role":"owner,admin","permission":"opaque-not-expanded"}),
        json!({"id":"surface-role-two","organizationId":"surface-one","role":"owner,admin","permission":"{\"platform\":[\"platform:read\"]}"}),
        json!({"id":"surface-role-split","organizationId":"surface-one","role":"owner","permission":"wrong-split-role"}),
        json!({"id":"surface-role-foreign","organizationId":"surface-two","role":"owner,admin","permission":"wrong-organization"}),
    ] {
        tx.insert(Model::OrganizationRole, row).await.unwrap();
    }
    tx.commit().await.unwrap();
    let service = SurfacingService::new(store.clone());
    assert_eq!(
        service.surface_role("surface-owner", now).await.unwrap(),
        json!("owner,admin")
    );
    assert_eq!(
        service
            .surface_permissions("surface-owner", now)
            .await
            .unwrap(),
        vec![
            json!("opaque-not-expanded"),
            json!("{\"platform\":[\"platform:read\"]}")
        ]
    );
    assert_eq!(
        service.surface_role("surface-missing", now).await.unwrap(),
        Value::Null
    );
    assert!(
        service
            .surface_permissions("surface-missing", now)
            .await
            .unwrap()
            .is_empty()
    );
    // A new live session changes the scoped organization; it does not reuse an
    // older membership or union its permission grants with the selected scope.
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Session,json!({"id":"surface-new","userId":"surface-owner","activeOrganizationId":"surface-two","createdAt":"2027-01-15T07:55:00Z","expiresAt":"2027-01-16T08:00:00Z"})).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        service.surface_role("surface-owner", now).await.unwrap(),
        json!("member")
    );
    assert!(
        service
            .surface_permissions("surface-owner", now)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn legacy_non_string_relationship_equality_survives_indexing() {
    let store = Arc::new(crate::store::memory::MemoryAuthStore::default());
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Session, json!({"id":"legacy-session","userId":"legacy-user","activeOrganizationId":42,"createdAt":"2027-01-15T07:00:00Z","expiresAt":"2027-01-16T08:00:00Z"})).await.unwrap();
    tx.insert(
        Model::Member,
        json!({"id":"legacy-member","userId":"legacy-user","organizationId":42,"role":"owner"}),
    )
    .await
    .unwrap();
    tx.insert(
        Model::OrganizationRole,
        json!({"id":"legacy-role","organizationId":42,"role":"owner","permission":"opaque"}),
    )
    .await
    .unwrap();
    tx.insert(
        Model::OrganizationRole,
        json!({"id":"string-role","organizationId":"42","role":"owner","permission":"wrong"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let now = DateTime::parse_from_rfc3339("2027-01-15T08:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    assert_eq!(
        SurfacingService::new(store)
            .surface_permissions("legacy-user", now)
            .await
            .unwrap(),
        vec![json!("opaque")]
    );
}
