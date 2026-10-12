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

// Exercise the same persistent profile lifecycle through memory and encrypted PostgreSQL.
pub(super) async fn profiles<S: AuthStore>(store: &Arc<S>, now: DateTime<Utc>) {
    for (algorithm, curve) in [
        ("Ed25519", "Ed25519"),
        ("ES256", "P-256"),
        ("ES384", "P-384"),
        ("ES512", "P-521"),
    ] {
        let mut tx = store.begin(true).await.unwrap();
        for row in tx.list(Model::Jwks).await.unwrap() {
            tx.delete(Model::Jwks, row["id"].as_str().unwrap())
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
        let service = KeyRing::new(store.clone(), "https://auth.fixture.test");
        let body = json!({"payload":{"sub":"profile-user"},"overrideOptions":{"jwks":{"keyPairConfig":{"alg":algorithm}}}});
        let (first, second) = tokio::join!(
            service.sign_jwt(Some(&body), now),
            service.sign_jwt(Some(&body), now)
        );
        let first = first.unwrap();
        let second = second.unwrap();
        let retired_token = first.clone();
        let keys = service.jwks(now).await.unwrap();
        assert_eq!(keys["keys"].as_array().unwrap().len(), 1);
        assert_eq!(keys["keys"][0]["alg"], algorithm);
        assert_eq!(keys["keys"][0]["crv"], curve);
        let key = &keys["keys"][0];
        assert!(key.get("d").is_none());
        assert!(key.get("privateKey").is_none());
        let restarted = KeyRing::new(store.clone(), "https://auth.fixture.test");
        for token in [
            first,
            second,
            restarted.sign_jwt(Some(&body), now).await.unwrap(),
        ] {
            assert_eq!(
                restarted.verify_jwt(Some(&token), now).await.unwrap()["payload"]["sub"],
                "profile-user"
            );
        }
        // Ordinary session token issuance can reuse the latest asymmetric key.
        let issued = restarted
            .issue("session-user", "profile@example.test", None, now)
            .await
            .unwrap();
        assert_eq!(
            restarted
                .verify_jwt(Some(&json!({"token":issued})), now)
                .await
                .unwrap()["payload"]["sub"],
            "session-user"
        );
        assert_eq!(restarted.jwks(now).await.unwrap(), keys);
        let mut tx = store.begin(true).await.unwrap();
        let mut row = tx.list(Model::Jwks).await.unwrap().remove(0);
        row["expiresAt"] = json!(timestamp(now - Duration::seconds(1)));
        tx.replace(Model::Jwks, row).await.unwrap();
        tx.commit().await.unwrap();
        let minted = restarted.sign_jwt(Some(&body), now).await.unwrap();
        assert_eq!(
            restarted.verify_jwt(Some(&minted), now).await.unwrap()["payload"]["sub"],
            "profile-user"
        );
        assert_eq!(
            restarted
                .verify_jwt(Some(&retired_token), now)
                .await
                .unwrap()["payload"]["sub"],
            "profile-user"
        );
        let mut tx = store.begin(true).await.unwrap();
        assert_eq!(tx.list(Model::Jwks).await.unwrap().len(), 2);
        for row in tx.list(Model::Jwks).await.unwrap() {
            tx.delete(Model::Jwks, row["id"].as_str().unwrap())
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
        assert_eq!(
            restarted
                .verify_jwt(Some(&retired_token), now)
                .await
                .unwrap()["payload"],
            Value::Null
        );
    }
}
#[tokio::test]
async fn asymmetric_profiles_initialize_once_restore_reuse_and_rotate() {
    profiles(
        &Arc::new(MemoryAuthStore::default()),
        DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap(),
    )
    .await;
}
