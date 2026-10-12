use super::*;
pub(super) async fn exercise<S: AuthStore>(
    store: &Arc<S>,
    service: &KeyRing<S>,
    token: &str,
    now: DateTime<Utc>,
) {
    let request = json!({"token":token});
    let verified = service.verify_jwt(Some(&request), now).await.unwrap();
    assert_eq!(verified["payload"]["sub"], "user");
    assert_eq!(verified["payload"]["aud"], "https://auth.fixture.test");
    assert_eq!(
        service
            .verify_jwt(Some(&request), now + Duration::seconds(900))
            .await
            .unwrap(),
        json!({"payload":null})
    );
    assert_eq!(
        service
            .verify_jwt(Some(&json!({"token":token,"issuer":"different"})), now)
            .await
            .unwrap(),
        json!({"payload":null})
    );
    assert_eq!(
        service.verify_jwt(None, now).await.unwrap_err().code,
        "VALIDATION_ERROR"
    );
    let mut tx = store.begin(true).await.unwrap();
    let original = tx.list(Model::Jwks).await.unwrap()[0].clone();
    let mut retired = original.clone();
    retired["expiresAt"] = json!(timestamp(now - Duration::days(40)));
    retired["privateKey"] = json!("corrupt-private-key");
    tx.replace(Model::Jwks, retired.clone()).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        service.verify_jwt(Some(&request), now).await.unwrap(),
        verified
    );
    let mut tx = store.begin(true).await.unwrap();
    assert_eq!(tx.list(Model::Jwks).await.unwrap(), vec![retired]);
    tx.replace(Model::Jwks, original).await.unwrap();
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn empty_verification_does_not_mint_keys() {
    let store = Arc::new(MemoryAuthStore::default());
    let service = KeyRing::new(store.clone(), "https://auth.fixture.test");
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for token in [
        "",
        "malformed",
        "eyJhbGciOiJFZERTQSIsImtpZCI6ImFic2VudCJ9.e30.YQ",
    ] {
        assert_eq!(
            service
                .verify_jwt(Some(&json!({"token":token})), now)
                .await
                .unwrap(),
            json!({"payload":null})
        );
    }
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.list(Model::Jwks).await.unwrap().is_empty());
    tx.commit().await.unwrap();
}
