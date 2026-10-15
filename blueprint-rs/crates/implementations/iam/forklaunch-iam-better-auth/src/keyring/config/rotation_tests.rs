use super::*;
use crate::{
    keyring::KeyRing,
    store::{
        AuthStore, AuthTransaction, Model,
        memory::{MemoryAuthStore, MemoryAuthTransaction},
    },
};
use chrono::{DateTime, Utc};
use std::{sync::Arc, time::Duration};
fn now() -> DateTime<Utc> {
    DateTime::from_timestamp_millis(1_800_000_000_000).unwrap()
}
fn config() -> KeyPairConfig {
    KeyPairConfig::default()
        .with_rotation_interval(Duration::from_millis(2))
        .unwrap()
}
#[tokio::test]
async fn rotation_expiry_pinning_grace_and_operator_mint_boundaries() {
    let store = Arc::new(MemoryAuthStore::default());
    let ring = KeyRing::new(store.clone(), "https://owned.test")
        .with_key_pair_config(config())
        .with_jwks_grace_period(Duration::from_millis(2))
        .unwrap();
    let first = ring.jwks(now()).await.unwrap()["keys"][0]["kid"]
        .as_str()
        .unwrap()
        .to_owned();
    let boundary = now() + chrono::Duration::milliseconds(2);
    let body = json!({"payload":{"sub":"owner"}});
    assert!(
        ring.sign_jwt_with_key_id(Some(&body), &first, None, boundary)
            .await
            .is_ok()
    );
    ring.issue("owner", "owner@test", None, boundary)
        .await
        .unwrap();
    assert!(
        ring.sign_jwt_with_key_id(
            Some(&body),
            &first,
            None,
            boundary + chrono::Duration::milliseconds(1)
        )
        .await
        .is_err()
    );
    assert_eq!(
        ring.jwks(boundary + chrono::Duration::milliseconds(1))
            .await
            .unwrap()["keys"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let published = ring
        .jwks(boundary + chrono::Duration::milliseconds(2))
        .await
        .unwrap();
    assert!(
        !published["keys"]
            .as_array()
            .unwrap()
            .iter()
            .any(|key| key["kid"] == first)
    );
    let rotation_time = boundary + chrono::Duration::milliseconds(3);
    let rotated = ring.rotate(rotation_time).await.unwrap();
    let mut tx = store.begin(false).await.unwrap();
    let row = tx
        .get(Model::Jwks, rotated["kid"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        super::super::optional_time(&row, "expiresAt").unwrap(),
        Some(rotation_time + chrono::Duration::milliseconds(2))
    );
    assert_eq!(tx.list(Model::Jwks).await.unwrap().len(), 3);
    tx.commit().await.unwrap();
}
struct UnavailableStore;
impl AuthStore for UnavailableStore {
    type Transaction<'a> = MemoryAuthTransaction;
    async fn begin(&self, _: bool) -> BlueprintResult<Self::Transaction<'_>> {
        panic!("invalid expiration must fail before I/O")
    }
}
#[tokio::test]
async fn invalid_rotation_and_clock_overflow_fail_before_storage() {
    assert!(
        KeyPairConfig::default()
            .with_rotation_interval(Duration::from_nanos(1))
            .is_err()
    );
    assert!(
        KeyPairConfig::default()
            .with_rotation_interval(Duration::MAX)
            .is_err()
    );
    let ring = KeyRing::new(Arc::new(UnavailableStore), "https://owned.test")
        .with_key_pair_config(config());
    let max = DateTime::<Utc>::MAX_UTC;
    assert!(ring.jwks(max).await.is_err());
    assert!(ring.rotate(max).await.is_err());
    assert!(ring.issue("owner", "owner@test", None, max).await.is_err());
    assert!(
        ring.sign_jwt(Some(&json!({"payload":{"sub":"owner"}})), max)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn absent_zero_and_shallow_overrides_disable_new_key_expiration() {
    for (config, body) in [
        (KeyPairConfig::default(), json!({"payload":{}})),
        (
            config().with_rotation_interval(Duration::ZERO).unwrap(),
            json!({"payload":{}}),
        ),
        (
            config(),
            json!({"payload":{},"overrideOptions":{"jwks":{}}}),
        ),
    ] {
        let store = Arc::new(MemoryAuthStore::default());
        let ring = KeyRing::new(store.clone(), "https://owned.test").with_key_pair_config(config);
        ring.sign_jwt(Some(&body), now()).await.unwrap();
        let mut tx = store.begin(false).await.unwrap();
        assert!(
            tx.list(Model::Jwks).await.unwrap()[0]
                .get("expiresAt")
                .is_none()
        );
        tx.commit().await.unwrap();
    }
}

#[cfg(feature = "http")]
#[tokio::test]
async fn http_token_and_session_header_keep_configured_algorithm_and_rotation_with_hooks() {
    use axum::{body::Body, http::Request};
    use std::sync::atomic::{AtomicI64, Ordering};
    use tower::ServiceExt;
    for payload_hook in [false, true] {
        for lifetime in [false, true] {
            let store = Arc::new(MemoryAuthStore::default());
            let mut tx = store.begin(true).await.unwrap();
            tx.insert(Model::User, json!({"id":"owner","email":"owner@test"}))
                .await
                .unwrap();
            tx.commit().await.unwrap();
            let session = crate::sessions::SessionService::new(store.clone())
                .create("owner", "", "", now())
                .await
                .unwrap();
            let cookie = crate::cookies::SessionCookie { secure: false }.serialize(
                session["token"].as_str().unwrap(),
                &[42; 32],
                true,
            );
            let clock = Arc::new(AtomicI64::new(now().timestamp_millis()));
            let clock_source = clock.clone();
            let profile = KeyPairConfig::new("ES256", 2048)
                .unwrap()
                .with_rotation_interval(Duration::from_millis(2))
                .unwrap();
            let mut app = crate::http::AuthHttp::new(
                store.clone(),
                "http://localhost:3000".into(),
                vec![42; 32],
                vec![],
            )
            .unwrap()
            .with_clock(move || {
                DateTime::from_timestamp_millis(clock_source.load(Ordering::SeqCst)).unwrap()
            })
            .with_key_pair_config(profile.clone());
            if payload_hook {
                app = app.with_jwt_payload_resolver(|_| async {
                    Ok(Some(serde_json::Map::from_iter([(
                        "custom".into(),
                        json!(true),
                    )])))
                });
            }
            if lifetime {
                app = app.with_jwt_lifetime(Duration::from_secs(60)).unwrap();
            }
            let router = app.router();
            let mut previous = None;
            for path in ["/token", "/get-session"] {
                let response = router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri(path)
                            .header("cookie", cookie.split(';').next().unwrap())
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
                let token = response_token(response).await;
                let (header, claims, _, _) = crate::keyring::verify::compact(&token).unwrap();
                assert_eq!(header["alg"], "ES256");
                assert_ne!(previous.as_ref(), Some(&header["kid"]));
                previous = Some(header["kid"].clone());
                assert_eq!(
                    claims["exp"],
                    json!(now().timestamp() + if lifetime { 60 } else { 900 })
                );
                assert_eq!(claims.get("custom").is_some(), payload_hook);
                let ring = KeyRing::new(store.clone(), "http://localhost:3000")
                    .with_key_pair_config(profile.clone());
                assert_eq!(
                    ring.verify_jwt(Some(&json!({"token":token})), now())
                        .await
                        .unwrap()["payload"]["sub"],
                    "owner"
                );
                let mut tx = store.begin(false).await.unwrap();
                let row = tx
                    .get(Model::Jwks, header["kid"].as_str().unwrap())
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    super::super::optional_time(&row, "expiresAt")
                        .unwrap()
                        .unwrap()
                        .timestamp_millis(),
                    clock.load(Ordering::SeqCst) + 2
                );
                tx.commit().await.unwrap();
                clock.fetch_add(2, Ordering::SeqCst);
            }
        }
    }
}

#[cfg(feature = "http")]
async fn response_token(response: axum::response::Response) -> String {
    let header_token = response
        .headers()
        .get("set-auth-jwt")
        .map(|value| value.to_str().unwrap().to_owned());
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap(),
    )
    .unwrap();
    header_token.unwrap_or_else(|| body["token"].as_str().unwrap().into())
}
