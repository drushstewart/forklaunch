use super::*;
use crate::store::memory::MemoryAuthStore;

pub(crate) async fn exercise_key_lifecycle<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let service = KeyRing::new(store.clone(), "https://auth.fixture.test");
    let (first, second) = tokio::join!(service.jwks(now), service.jwks(now));
    let first = first.unwrap();
    assert_eq!(first, second.unwrap());
    assert_eq!(first["keys"].as_array().unwrap().len(), 1);
    let token = service
        .issue("user", "fixture@example.test", None, now)
        .await
        .unwrap();
    let restarted = KeyRing::new(store.clone(), "https://auth.fixture.test");
    assert_eq!(
        token,
        restarted
            .issue("user", "fixture@example.test", None, now)
            .await
            .unwrap()
    );
    let rotate_at = now + Duration::seconds(10);
    let next = restarted.rotate(rotate_at).await.unwrap();
    assert_ne!(next["kid"], first["keys"][0]["kid"]);
    assert_eq!(
        restarted.jwks(rotate_at).await.unwrap()["keys"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        restarted
            .jwks(rotate_at + Duration::days(30))
            .await
            .unwrap(),
        json!({"keys":[next]})
    );
    let mut tx = store.begin(true).await.unwrap();
    for mut row in tx.list(Model::Jwks).await.unwrap() {
        row["expiresAt"] = json!(timestamp(rotate_at));
        tx.replace(Model::Jwks, row).await.unwrap();
    }
    tx.commit().await.unwrap();
    let after_grace = rotate_at + Duration::days(30);
    assert_eq!(
        restarted.jwks(after_grace).await.unwrap(),
        json!({"keys":[]})
    );
    restarted
        .issue("user", "fixture@example.test", None, after_grace)
        .await
        .unwrap();
    assert_eq!(
        restarted.jwks(after_grace).await.unwrap()["keys"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut tx = store.begin(true).await.unwrap();
    let mut live = tx
        .list(Model::Jwks)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.get("expiresAt").is_none())
        .unwrap();
    let mut public: Value = serde_json::from_str(live["publicKey"].as_str().unwrap()).unwrap();
    public["d"] = json!("must-not-be-published");
    live["publicKey"] = json!(public.to_string());
    tx.replace(Model::Jwks, live).await.unwrap();
    tx.commit().await.unwrap();
    assert!(restarted.jwks(after_grace).await.is_err());
    assert!(
        restarted
            .issue("user", "fixture@example.test", None, after_grace)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn concurrent_key_initialization_restart_rotation_grace_and_corruption() {
    exercise_key_lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
