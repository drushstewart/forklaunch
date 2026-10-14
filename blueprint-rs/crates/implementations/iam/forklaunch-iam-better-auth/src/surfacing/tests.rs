use super::*;
use serde_json::json;
pub(crate) const USER: &str = "a1000000-0000-4000-8000-000000000001";
const ORGANIZATION: &str = "a2000000-0000-4000-8000-000000000001";
const OTHER_ORGANIZATION: &str = "a2000000-0000-4000-8000-000000000002";
const SESSION: &str = "a3000000-0000-4000-8000-000000000001";
pub(crate) fn scope() -> AuthorizationScope {
    AuthorizationScope {
        user_id: USER.into(),
        organization_id: ORGANIZATION.into(),
        session_id: SESSION.into(),
    }
}
#[tokio::test]
async fn exact_authenticated_session_scopes_current_roles_and_permissions() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}
#[allow(clippy::too_many_lines)] // One shared memory/PostgreSQL lifecycle proves current scoped authorization.
pub(crate) async fn exercise<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::parse_from_rfc3339("2027-01-15T08:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let session = json!({"id":SESSION,"userId":USER,"activeOrganizationId":ORGANIZATION,"createdAt":"2027-01-15T07:00:00Z","expiresAt":"2027-01-16T08:00:00Z"});
    let member =
        json!({"id":"surface-member","userId":USER,"organizationId":ORGANIZATION,"role":"custom"});
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Session, session.clone()).await.unwrap();
    tx.insert(Model::Session,json!({"id":"a3000000-0000-4000-8000-000000000002","userId":USER,"activeOrganizationId":OTHER_ORGANIZATION,"createdAt":"2027-01-15T07:59:00Z","expiresAt":"2027-01-16T08:00:00Z"})).await.unwrap();
    tx.insert(Model::Member, member.clone()).await.unwrap();
    tx.insert(Model::Member,json!({"id":"surface-other-member","userId":USER,"organizationId":OTHER_ORGANIZATION,"role":"owner"})).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"surface-grant","organizationId":ORGANIZATION,"role":"custom","permission":"opaque-custom-grant"})).await.unwrap();
    tx.commit().await.unwrap();
    let service = SurfacingService::new(store.clone());
    let scope = scope();
    assert_eq!(
        service.surface_role_scoped(&scope, now).await.unwrap(),
        "custom"
    );
    assert_eq!(
        service
            .surface_permissions_scoped(&scope, now)
            .await
            .unwrap(),
        vec![json!("opaque-custom-grant")]
    );
    assert!(service.surface_role(USER, now).await.unwrap().is_null());
    assert!(
        service
            .surface_permissions(USER, now)
            .await
            .unwrap()
            .is_empty()
    );
    let uppercase_session = AuthorizationScope {
        session_id: scope.session_id.to_ascii_uppercase(),
        ..scope.clone()
    };
    assert_eq!(
        service
            .surface_role_scoped(&uppercase_session, now)
            .await
            .unwrap(),
        "custom"
    );
    let uppercase_org = AuthorizationScope {
        organization_id: scope.organization_id.to_ascii_uppercase(),
        ..scope.clone()
    };
    assert!(
        service
            .surface_role_scoped(&uppercase_org, now)
            .await
            .unwrap()
            .is_null()
    );
    for invalid in [
        AuthorizationScope {
            organization_id: OTHER_ORGANIZATION.into(),
            ..scope.clone()
        },
        AuthorizationScope {
            user_id: "a1000000-0000-4000-8000-000000000002".into(),
            ..scope.clone()
        },
        AuthorizationScope {
            session_id: "not-a-uuid".into(),
            ..scope.clone()
        },
    ] {
        assert!(
            service
                .surface_role_scoped(&invalid, now)
                .await
                .unwrap()
                .is_null()
        );
        assert!(
            service
                .surface_permissions_scoped(&invalid, now)
                .await
                .unwrap()
                .is_empty()
        );
    }
    for (role, expected) in [
        ("owner", vec!["platform:read", "platform:write"]),
        ("admin", vec!["platform:read", "platform:write"]),
        ("editor", vec!["platform:read", "platform:write"]),
        ("system", vec!["platform:read", "platform:write"]),
        ("viewer", vec!["platform:read"]),
    ] {
        let mut tx = store.begin(true).await.unwrap();
        let mut changed = member.clone();
        changed["role"] = json!(role);
        tx.replace(Model::Member, changed).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(
            service.surface_role_scoped(&scope, now).await.unwrap(),
            role
        );
        assert_eq!(
            service
                .surface_permissions_scoped(&scope, now)
                .await
                .unwrap(),
            expected.into_iter().map(Value::from).collect::<Vec<_>>()
        );
    }
    for change in [
        json!({"activeOrganizationId":null}),
        json!({"activeOrganizationId":OTHER_ORGANIZATION}),
        json!({"expiresAt":"2027-01-15T08:00:00Z"}),
    ] {
        let mut changed = session.clone();
        changed
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        let mut tx = store.begin(true).await.unwrap();
        tx.replace(Model::Session, changed).await.unwrap();
        tx.commit().await.unwrap();
        assert!(
            service
                .surface_role_scoped(&scope, now)
                .await
                .unwrap()
                .is_null()
        );
    }
    let mut tx = store.begin(true).await.unwrap();
    tx.replace(Model::Session, session.clone()).await.unwrap();
    tx.delete(Model::Member, "surface-member").await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        service
            .surface_role_scoped(&scope, now)
            .await
            .unwrap()
            .is_null()
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Member, member).await.unwrap();
    tx.delete(Model::Session, SESSION).await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        service
            .surface_role_scoped(&scope, now)
            .await
            .unwrap()
            .is_null()
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Session, session).await.unwrap();
    tx.commit().await.unwrap();
}
#[test]
fn verified_payload_requires_all_three_uuid_components() {
    let payload = json!({"sub":USER,"activeOrganizationId":ORGANIZATION,"sessionId":SESSION});
    assert_eq!(AuthorizationScope::from_payload(&payload), Some(scope()));
    for field in ["sub", "activeOrganizationId", "sessionId"] {
        for invalid in [Value::Null, json!(42), json!(""), json!("not-a-uuid")] {
            let mut changed = payload.clone();
            changed[field] = invalid;
            assert!(AuthorizationScope::from_payload(&changed).is_none());
        }
    }
}
