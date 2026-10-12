use super::*;
use crate::store::memory::MemoryAuthStore;

pub(crate) async fn exercise<S: AuthStore>(store: Arc<S>) {
    let service = StateService::new(store.clone(), vec![7; 32], false).unwrap();
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let input = json!({"callbackURL":"https://auth.fixture.test/complete","errorURL":"https://auth.fixture.test/failure","custom":{"value":true}});
    let grant = service.generate(input.clone(), now).await.unwrap();
    assert_eq!(grant.state.len(), 32);
    assert_eq!(grant.code_verifier.len(), 128);
    assert!(grant.state.bytes().all(|byte| byte.is_ascii_alphanumeric()));
    assert!(
        grant
            .code_verifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric())
    );
    assert!(
        grant
            .set_cookie
            .ends_with("; Max-Age=300; Path=/; HttpOnly; SameSite=Lax")
    );
    let cookie = grant.set_cookie.split(';').next().unwrap();
    for header in [None, Some("better-auth.state=forged")] {
        let error = service
            .consume(&grant.state, header, now)
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, "state_security_mismatch");
        assert_eq!(error.details["errorURL"], input["errorURL"]);
        assert!(!error.details.contains_key("clearCookie"));
    }
    let (one, two) = tokio::join!(
        service.consume(&grant.state, Some(cookie), now),
        service.consume(&grant.state, Some(cookie), now)
    );
    assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
    let data = one.or(two).unwrap();
    assert_eq!(data.data["codeVerifier"], *grant.code_verifier);
    assert_eq!(data.data["oauthState"], grant.state);
    assert_eq!(data.data["custom"], input["custom"]);
    assert_eq!(
        data.clear_cookie,
        "better-auth.state=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax"
    );
    assert_eq!(
        service
            .consume(&grant.state, Some(cookie), now)
            .await
            .err()
            .unwrap()
            .code,
        "state_mismatch"
    );
    for delta in [600_000, 600_001] {
        let grant = service.generate(input.clone(), now).await.unwrap();
        let result = service
            .consume(
                &grant.state,
                Some(grant.set_cookie.split(';').next().unwrap()),
                now + Duration::milliseconds(delta),
            )
            .await;
        if delta == 600_000 {
            assert!(result.is_ok());
        } else {
            let error = result.err().unwrap();
            assert_eq!(error.code, "state_mismatch");
            assert_eq!(error.details["clearCookie"], true);
        }
    }
    let stale = service.generate(input.clone(), now).await.unwrap();
    let result = service
        .consume("unknown-state", None, now + Duration::minutes(11))
        .await;
    assert_eq!(result.err().unwrap().code, "state_mismatch");
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.list(Model::Verification)
            .await
            .unwrap()
            .iter()
            .all(|row| row["identifier"] != stale.state)
    );
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn state_cookie_correlation_concurrent_consumption_expiry_and_cleanup() {
    exercise(Arc::new(MemoryAuthStore::default())).await;
}

#[tokio::test]
async fn secure_names_invalid_shape_and_legacy_link_coercion() {
    let store = Arc::new(MemoryAuthStore::default());
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let service = StateService::new(store.clone(), vec![9; 32], true).unwrap();
    assert!(StateService::new(store.clone(), vec![0; 31], true).is_err());
    assert!(
        service
            .generate(json!({"callbackURL":null}), now)
            .await
            .is_err()
    );
    let grant = service.generate(json!({"callbackURL":"/complete","link":{"email":"fixture@test","userId":42,"strip":true}}), now).await.unwrap();
    assert!(grant.set_cookie.starts_with("__Secure-better-auth.state="));
    assert!(grant.set_cookie.contains("; Secure; SameSite=Lax"));
    let mut tx = store.begin(true).await.unwrap();
    let mut row = tx.list(Model::Verification).await.unwrap().remove(0);
    let mut value = crate::ecmascript::parse(row["value"].as_str().unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("oauthState");
    row["value"] = json!(crate::ecmascript::stringify(&value));
    tx.replace(Model::Verification, row).await.unwrap();
    tx.commit().await.unwrap();
    let consumed = service
        .consume(
            &grant.state,
            Some(grant.set_cookie.split(';').next().unwrap()),
            now,
        )
        .await
        .unwrap();
    assert_eq!(
        consumed.data["link"],
        json!({"email":"fixture@test","userId":"42"})
    );
    assert!(consumed.clear_cookie.contains("; Secure;"));
}
