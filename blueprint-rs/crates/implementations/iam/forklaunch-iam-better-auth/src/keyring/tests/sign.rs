use super::*;
pub(super) async fn exercise<S: AuthStore>(service: &KeyRing<S>, now: DateTime<Utc>) {
    let signed = service
        .sign_jwt(
            Some(&json!({"payload":{"sub":"server-user","custom":{"flag":true}}})),
            now,
        )
        .await
        .unwrap();
    let verified = service.verify_jwt(Some(&signed), now).await.unwrap();
    assert_eq!(verified["payload"]["sub"], "server-user");
    assert_eq!(verified["payload"]["custom"], json!({"flag":true}));
    assert!(verified["payload"].get("iat").is_none());
    assert_eq!(verified["payload"]["exp"], now.timestamp() + 900);
    assert_eq!(
        service
            .sign_jwt(Some(&json!({"payload":{"sub":1}})), now)
            .await
            .unwrap_err()
            .message,
        "\"sub\" claim must be a string"
    );
    let overridden = service.sign_jwt(Some(&json!({"payload":{"sub":"server-user"},"overrideOptions":{"jwt":{"issuer":"custom","expirationTime":123}}})),now).await.unwrap();
    let token = overridden["token"].as_str().unwrap();
    let content = URL_SAFE_NO_PAD
        .decode(token.split('.').nth(1).unwrap())
        .unwrap();
    let claims: Value = serde_json::from_slice(&content).unwrap();
    assert_eq!(claims["iss"], "custom");
    assert_eq!(claims["exp"], 123);
}
#[tokio::test]
async fn schema_and_duration_precede_key_mint_but_claim_failure_retains_key() {
    let store = Arc::new(MemoryAuthStore::default());
    let service = KeyRing::new(store.clone(), "https://auth.fixture.test");
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    assert_eq!(
        service.sign_jwt(None, now).await.unwrap_err().code,
        "VALIDATION_ERROR"
    );
    service
        .sign_jwt(
            Some(&json!({"payload":{},"overrideOptions":{"jwt":{"expirationTime":"bad"}}})),
            now,
        )
        .await
        .unwrap_err();
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.list(Model::Jwks).await.unwrap().is_empty());
    tx.commit().await.unwrap();
    service
        .sign_jwt(Some(&json!({"payload":{"exp":"bad"}})), now)
        .await
        .unwrap_err();
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(tx.list(Model::Jwks).await.unwrap().len(), 1);
    tx.commit().await.unwrap();
    exercise(&service, now).await;
}
