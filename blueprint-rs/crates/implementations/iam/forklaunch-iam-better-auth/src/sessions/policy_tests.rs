use super::*;
use crate::store::memory::MemoryAuthStore;

#[allow(clippy::too_many_lines)]
pub(crate) async fn exercise_policy<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let policy = SessionPolicy::new(120, 30, 60, false).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"session-policy-test-user","email":"session-policy-domain@example.test","emailVerified":true}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    for (mode, elapsed, expected_refresh, missing) in [
        ("before", 29999, false, false),
        ("equal", 30000, true, false),
        ("after", 30001, true, false),
        ("disabled", 30001, false, false),
        ("query", 30001, false, false),
        ("transient", 30001, false, false),
        ("expiry-equal", 120_000, true, false),
        ("expiry-after", 120_001, false, true),
        ("zero-update", 0, true, false),
        ("update-after-expiry", 120_000, false, false),
    ] {
        let selected = match mode {
            "disabled" => SessionPolicy::new(120, 30, 60, true).unwrap(),
            "zero-update" => SessionPolicy::new(120, 0, 60, false).unwrap(),
            "update-after-expiry" => SessionPolicy::new(120, 121, 60, false).unwrap(),
            _ => policy,
        };
        let service = SessionService::new(store.clone()).with_session_policy(selected);
        let row = service
            .create("session-policy-test-user", "", "", now)
            .await
            .unwrap();
        assert_eq!(
            field_time(&row, "expiresAt").unwrap(),
            now + Duration::seconds(120)
        );
        let result = service
            .get(
                row["token"].as_str().unwrap(),
                mode == "transient",
                mode == "query",
                now + Duration::milliseconds(elapsed),
            )
            .await
            .unwrap();
        assert_eq!(result.is_none(), missing, "{mode}");
        if let Some(result) = result {
            assert_eq!(result.refreshed, expected_refresh, "{mode}");
            let expiry = if expected_refresh {
                now + Duration::milliseconds(elapsed) + Duration::seconds(120)
            } else {
                now + Duration::seconds(120)
            };
            assert_eq!(
                field_time(&result.session, "expiresAt").unwrap(),
                expiry,
                "{mode}"
            );
        }
        service
            .sign_out(row["token"].as_str().unwrap())
            .await
            .unwrap();
    }
    let service = SessionService::new(store.clone()).with_session_policy(policy);
    let row = service
        .create("session-policy-test-user", "", "", now)
        .await
        .unwrap();
    let token = row["token"].as_str().unwrap();
    assert!(
        service
            .list(token, false, now + Duration::seconds(59))
            .await
            .is_ok()
    );
    assert_eq!(
        service
            .list(token, false, now + Duration::seconds(60))
            .await
            .unwrap_err()
            .code,
        "SESSION_NOT_FRESH"
    );
    assert!(
        SessionService::new(store.clone())
            .with_session_policy(SessionPolicy::new(120, 30, 0, false).unwrap())
            .list(token, false, now + Duration::seconds(60))
            .await
            .is_ok()
    );
    // Current policy applies to old rows; authoritative stored expiry is not rewritten until refresh.
    let changed = SessionService::new(store.clone())
        .with_session_policy(SessionPolicy::new(300, 0, 0, true).unwrap());
    let before = changed
        .get(token, false, false, now + Duration::seconds(60))
        .await
        .unwrap()
        .unwrap();
    assert!(!before.refreshed);
    let changed = SessionService::new(store.clone())
        .with_session_policy(SessionPolicy::new(300, 0, 0, false).unwrap());
    let after = changed
        .get(token, false, false, now + Duration::seconds(60))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        field_time(&after.session, "expiresAt").unwrap(),
        now + Duration::seconds(360)
    );
    service.sign_out(token).await.unwrap();
    let transient =
        new_session_with_policy("session-policy-test-user", "", "", now, policy, false).unwrap();
    assert_eq!(
        field_time(&transient, "expiresAt").unwrap(),
        now + Duration::days(1)
    );
}

#[tokio::test]
async fn session_policy_refresh_freshness_expiry_and_current_policy() {
    exercise_policy(Arc::new(MemoryAuthStore::default())).await;
}
#[test]
fn session_policy_bounds_and_cookie_compatibility() {
    assert!(SessionPolicy::new(0, 0, 0, false).is_err());
    assert!(SessionPolicy::new(u32::MAX, u32::MAX, u32::MAX, true).is_ok());
    let policy = SessionPolicy::new(120, 30, 0, false).unwrap();
    let cookie = crate::cookies::SessionCookie { secure: false };
    assert!(
        cookie
            .serialize("token", b"owned", true)
            .contains("Max-Age=86400")
    );
    assert!(
        cookie
            .serialize_with_policy("token", b"owned", true, policy)
            .contains("Max-Age=120")
    );
    assert!(
        !cookie
            .serialize_with_policy("token", b"owned", false, policy)
            .contains("Max-Age=")
    );
    assert!(new_session_with_policy("u", "", "", DateTime::<Utc>::MAX_UTC, policy, true).is_err());
}
