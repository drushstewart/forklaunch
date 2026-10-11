use super::*;
use crate::store::memory::MemoryAuthStore;

#[allow(clippy::too_many_lines)]
pub(crate) async fn exercise_sessions<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    for (id, email) in [
        ("session-user", "sessions@fixture.test"),
        ("session-other", "other-sessions@fixture.test"),
    ] {
        tx.insert(Model::User,json!({"id":id,"email":email,"name":"Person","firstName":"First","lastName":"Last","emailVerified":false,
            "createdAt":timestamp(now),"updatedAt":timestamp(now),"providerFields":{"private":"hidden"}})).await.unwrap();
    }
    tx.commit().await.unwrap();
    let service = SessionService::new(store.clone());
    assert!(service.create("missing-user", "", "", now).await.is_err());
    let first = service
        .create("session-user", "private-ip", "private-agent", now)
        .await
        .unwrap();
    let peer = service
        .create("session-user", "", "", now + Duration::seconds(1))
        .await
        .unwrap();
    let other = service
        .create("session-other", "", "", now + Duration::seconds(2))
        .await
        .unwrap();
    let token = first["token"].as_str().unwrap();
    assert_eq!(token.len(), 32);
    assert!(token.bytes().all(|byte| byte.is_ascii_alphanumeric()));
    assert_ne!(first["token"], peer["token"]);
    assert!(service.get("", false, false, now).await.unwrap().is_none());
    let fresh = service
        .get(
            token,
            false,
            false,
            now + Duration::hours(1) - Duration::milliseconds(1),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(!fresh.refreshed);
    assert!(fresh.user.get("providerFields").is_none());
    let refreshed = service
        .get(token, false, false, now + Duration::hours(1))
        .await
        .unwrap()
        .unwrap();
    assert!(refreshed.refreshed);
    assert_eq!(refreshed.session["token"], first["token"]);
    assert_eq!(refreshed.session["createdAt"], first["createdAt"]);
    let skipped = service
        .get(token, false, true, now + Duration::hours(2))
        .await
        .unwrap()
        .unwrap();
    assert!(!skipped.refreshed);
    assert_eq!(skipped.session["expiresAt"], refreshed.session["expiresAt"]);
    assert_eq!(
        service
            .list(token, false, now + Duration::days(1))
            .await
            .unwrap_err()
            .code,
        "SESSION_NOT_FRESH"
    );
    let extended = service
        .get(token, false, true, now + Duration::days(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        field_time(&extended.session, "updatedAt").unwrap(),
        now + Duration::days(1)
    );
    service
        .revoke_one(
            token,
            other["token"].as_str().unwrap(),
            false,
            now + Duration::days(1),
        )
        .await
        .unwrap();
    assert!(
        service
            .get(
                other["token"].as_str().unwrap(),
                true,
                false,
                now + Duration::days(1)
            )
            .await
            .unwrap()
            .is_some()
    );
    service
        .revoke_others(token, false, now + Duration::days(1))
        .await
        .unwrap();
    assert!(
        service
            .get(
                peer["token"].as_str().unwrap(),
                false,
                false,
                now + Duration::days(1)
            )
            .await
            .unwrap()
            .is_none()
    );
    service
        .revoke_all(token, false, now + Duration::days(1))
        .await
        .unwrap();
    assert!(
        service
            .get(token, false, false, now + Duration::days(1))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        service
            .revoke_all(token, false, now + Duration::days(1))
            .await
            .unwrap_err()
            .code,
        "UNAUTHORIZED"
    );
    service
        .sign_out(other["token"].as_str().unwrap())
        .await
        .unwrap();
    assert!(
        service
            .get(other["token"].as_str().unwrap(), false, false, now)
            .await
            .unwrap()
            .is_none()
    );
    let transient = service.create("session-user", "", "", now).await.unwrap();
    let transient_token = transient["token"].as_str().unwrap();
    let view = service
        .get(transient_token, true, false, now + Duration::hours(1))
        .await
        .unwrap()
        .unwrap();
    assert!(!view.refreshed);
    assert!(
        service
            .get(transient_token, true, false, now + Duration::days(1))
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        service
            .get(
                transient_token,
                true,
                false,
                now + Duration::days(1) + Duration::milliseconds(1)
            )
            .await
            .unwrap()
            .is_none()
    );
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.get(Model::Session, transient["id"].as_str().unwrap())
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn native_refresh_freshness_scoped_revocation_transient_expiry_and_replay() {
    exercise_sessions(Arc::new(MemoryAuthStore::default())).await;
}
