mod sign;
mod verify;
use super::*;
use crate::store::memory::MemoryAuthStore;
use chrono::Duration;

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
    sign::exercise(&service, now).await;
    verify::exercise(&store, &service, &token, now).await;
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
    sign::profiles(&store, now).await;
}

#[tokio::test]
async fn concurrent_key_initialization_restart_rotation_grace_and_corruption() {
    exercise_key_lifecycle(Arc::new(MemoryAuthStore::default())).await;
}

pub(crate) async fn exercise_large<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let ring = KeyRing::new(store.clone(), "https://auth.fixture.test");
    let original = ring.jwks(now).await.unwrap();
    let mut tx = store.begin(true).await.unwrap();
    let template = tx
        .get(Model::Jwks, original["keys"][0]["kid"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    for index in 0..4_100 {
        let mut row = template.clone();
        row["id"] = json!(format!("large-key-{index}"));
        let mut public: Value = serde_json::from_str(row["publicKey"].as_str().unwrap()).unwrap();
        public["kid"] = row["id"].clone();
        row["publicKey"] = json!(public.to_string());
        tx.insert(Model::Jwks, row).await.unwrap();
    }
    tx.commit().await.unwrap();
    assert_eq!(
        ring.jwks(now).await.unwrap()["keys"]
            .as_array()
            .unwrap()
            .len(),
        100
    );
    let token = ring
        .issue("user", "fixture@example.test", None, now)
        .await
        .unwrap();
    assert_eq!(
        ring.verify_jwt(Some(&json!({"token":token})), now)
            .await
            .unwrap()["payload"]["sub"],
        "user"
    );
    let rotated = ring.rotate(now + Duration::seconds(1)).await.unwrap();
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::Jwks, "large-key-4099")
            .await
            .unwrap()
            .unwrap()["expiresAt"],
        timestamp(now + Duration::seconds(1))
    );
    assert!(
        tx.get(Model::Jwks, rotated["kid"].as_str().unwrap())
            .await
            .unwrap()
            .unwrap()
            .get("expiresAt")
            .is_none()
    );
    tx.commit().await.unwrap();
    // The configured first-100 window excludes the newly appended key.
    assert_eq!(
        ring.jwks(now + Duration::days(31)).await.unwrap(),
        json!({"keys":[]})
    );
    // Leave no key-window exhaustion behind for the subsequent HTTP fixture.
    let mut tx = store.begin(true).await.unwrap();
    let mut after = None;
    loop {
        let page = tx.list_page(Model::Jwks, after, 256).await.unwrap();
        for row in page.records {
            tx.delete(Model::Jwks, row["id"].as_str().unwrap())
                .await
                .unwrap();
        }
        after = page.next;
        if after.is_none() {
            break;
        }
    }
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn configured_grace_period_uses_strict_millisecond_boundaries() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let store = Arc::new(MemoryAuthStore::default());
    let ring = KeyRing::new(store.clone(), "https://auth.example.test");
    ring.jwks(now).await.unwrap();
    let mut tx = store.begin(true).await.unwrap();
    let source = tx.list(Model::Jwks).await.unwrap().remove(0);
    tx.delete(Model::Jwks, source["id"].as_str().unwrap())
        .await
        .unwrap();
    let offsets = [
        None,
        Some(-2_592_000_001),
        Some(-2_592_000_000),
        Some(-1001),
        Some(-1000),
        Some(-1),
        Some(0),
        Some(1),
        Some(1000),
        Some(1001),
    ];
    for (index, offset) in offsets.iter().enumerate() {
        let mut row = source.clone();
        row["id"] = json!(format!("key-{index}"));
        let mut public: Value = serde_json::from_str(row["publicKey"].as_str().unwrap()).unwrap();
        public["kid"] = row["id"].clone();
        row["publicKey"] = json!(public.to_string());
        row["expiresAt"] = offset.map_or(Value::Null, |offset| {
            json!(timestamp(now + Duration::milliseconds(offset)))
        });
        tx.insert(Model::Jwks, row).await.unwrap();
    }
    tx.commit().await.unwrap();
    for millis in [2_592_000_000, 0, 1, 1000] {
        let configured = KeyRing::new(store.clone(), "https://auth.example.test")
            .with_jwks_grace_period(std::time::Duration::from_millis(millis))
            .unwrap();
        let actual = configured.jwks(now).await.unwrap();
        let expected: Vec<_> = offsets
            .iter()
            .enumerate()
            .filter(|(_, offset)| {
                offset.is_none_or(|offset| i128::from(offset) + i128::from(millis) > 0)
            })
            .map(|(index, _)| json!(format!("key-{index}")))
            .collect();
        assert_eq!(
            actual["keys"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| key["kid"].clone())
                .collect::<Vec<_>>(),
            expected
        );
        if millis == 2_592_000_000 {
            assert_eq!(actual, ring.jwks(now).await.unwrap());
        }
    }
    for invalid in [std::time::Duration::from_nanos(1), std::time::Duration::MAX] {
        assert!(
            KeyRing::new(store.clone(), "https://auth.example.test")
                .with_jwks_grace_period(invalid)
                .is_err()
        );
    }
}
