use super::*;
use crate::{
    keyring::{KeyRing, generate_key_record},
    store::{AuthStore, AuthTransaction, Model, memory::MemoryAuthStore},
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
fn now() -> DateTime<Utc> {
    DateTime::from_timestamp(1_800_000_000, 0).unwrap()
}
#[tokio::test]
async fn legacy_native_metadata_signs_publishes_verifies_without_rewriting() {
    for algorithm in ["EdDSA", "ES256", "RS256"] {
        for null in [false, true] {
            let store = Arc::new(MemoryAuthStore::default());
            let mut row = generate_key_record(now(), algorithm, 2048, None)
                .await
                .unwrap()
                .0;
            let id = row["id"].as_str().unwrap().to_owned();
            if null {
                row["alg"] = Value::Null;
                row["crv"] = Value::Null;
            } else {
                row.as_object_mut().unwrap().remove("alg");
                row.as_object_mut().unwrap().remove("crv");
            }
            let mut public: Value =
                serde_json::from_str(row["publicKey"].as_str().unwrap()).unwrap();
            public.as_object_mut().unwrap().remove("alg");
            public.as_object_mut().unwrap().remove("kid");
            row["publicKey"] = json!(public.to_string());
            let mut tx = store.begin(true).await.unwrap();
            tx.insert(Model::Jwks, row.clone()).await.unwrap();
            tx.commit().await.unwrap();
            let ring = KeyRing::new(store.clone(), "https://owned.test")
                .with_key_pair_config(KeyPairConfig::new(algorithm, 2048).unwrap());
            let body = json!({"payload":{"sub":"owner"}});
            let signed = ring
                .sign_jwt_with_key_id(Some(&body), &id, Some(algorithm), now())
                .await
                .unwrap();
            assert_eq!(
                ring.verify_jwt(Some(&signed), now()).await.unwrap()["payload"]["sub"],
                "owner"
            );
            assert_eq!(ring.jwks(now()).await.unwrap()["keys"][0]["alg"], algorithm);
            let session = ring
                .issue("owner", "owner@example.test", None, now())
                .await
                .unwrap();
            assert_eq!(
                ring.verify_jwt(Some(&json!({"token":session})), now())
                    .await
                    .unwrap()["payload"]["sub"],
                "owner"
            );
            let mut tx = store.begin(false).await.unwrap();
            assert_eq!(tx.get(Model::Jwks, &id).await.unwrap().unwrap(), row);
            assert_eq!(tx.list(Model::Jwks).await.unwrap().len(), 1);
            tx.commit().await.unwrap();
        }
    }
}
#[tokio::test]
async fn legacy_conflicts_reject_without_mint_and_modern_rows_stay_strict() {
    let base = generate_key_record(now(), "EdDSA", 2048, None)
        .await
        .unwrap()
        .0;
    for field in ["alg", "kid", "d"] {
        let store = Arc::new(MemoryAuthStore::default());
        let mut row = base.clone();
        row["alg"] = Value::Null;
        let mut public: Value = serde_json::from_str(row["publicKey"].as_str().unwrap()).unwrap();
        public[field] = json!("conflict");
        row["publicKey"] = json!(public.to_string());
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(Model::Jwks, row).await.unwrap();
        tx.commit().await.unwrap();
        let ring = KeyRing::new(store.clone(), "https://owned.test");
        assert!(ring.jwks(now()).await.is_err());
        assert!(
            ring.sign_jwt(Some(&json!({"payload":{"sub":"owner"}})), now())
                .await
                .is_err()
        );
        let mut tx = store.begin(false).await.unwrap();
        assert_eq!(tx.list(Model::Jwks).await.unwrap().len(), 1);
        tx.commit().await.unwrap();
    }
    let mut modern = base.clone();
    let mut jwk: Value = serde_json::from_str(modern["publicKey"].as_str().unwrap()).unwrap();
    jwk.as_object_mut().unwrap().remove("kid");
    modern["publicKey"] = json!(jwk.to_string());
    assert!(
        super::public(&modern, "EdDSA").is_err(),
        "explicit metadata must not enable legacy repair"
    );
    assert!(KeyPairConfig::new("HS256", 2048).is_err());
    assert!(KeyPairConfig::new("RS256", 1024).is_err());
}
#[tokio::test]
async fn configured_http_publication_and_rotation_use_the_same_algorithm() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let store = Arc::new(MemoryAuthStore::default());
    let config = KeyPairConfig::new("ES256", 2048).unwrap();
    let app = crate::http::AuthHttp::new(
        store.clone(),
        "http://localhost:3000".into(),
        vec![42; 32],
        vec![],
    )
    .unwrap()
    .with_key_pair_config(config.clone());
    let response = app
        .router()
        .oneshot(Request::builder().uri("/jwks").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let published: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(published["keys"][0]["alg"], "ES256");
    let ring = KeyRing::new(store, "http://localhost:3000").with_key_pair_config(config);
    let rotated = ring.rotate(Utc::now()).await.unwrap();
    assert_eq!(rotated["alg"], "ES256");
}

#[tokio::test]
async fn malformed_explicit_algorithm_and_legacy_public_json_return_null_verification() {
    let store = Arc::new(MemoryAuthStore::default());
    let ring = KeyRing::new(store.clone(), "https://owned.test");
    let signed = ring
        .sign_jwt(Some(&json!({"payload":{"sub":"owner"}})), now())
        .await
        .unwrap();
    let mut tx = store.begin(false).await.unwrap();
    let row = tx.list(Model::Jwks).await.unwrap().remove(0);
    tx.commit().await.unwrap();
    for malformed_public in [false, true] {
        let mut broken = row.clone();
        broken["alg"] = if malformed_public {
            Value::Null
        } else {
            json!(42)
        };
        if malformed_public {
            broken["publicKey"] = json!("invalid JSON");
        }
        let mut tx = store.begin(true).await.unwrap();
        tx.replace(Model::Jwks, broken).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(
            ring.verify_jwt(Some(&signed), now()).await.unwrap(),
            json!({"payload":null})
        );
    }
}

#[tokio::test]
async fn signing_jwks_override_replaces_rather_than_merges_server_configuration() {
    for (body, expected) in [
        (json!({"payload":{"sub":"owner"}}), "ES256"),
        (
            json!({"payload":{"sub":"owner"},"overrideOptions":{"jwks":{}}}),
            "EdDSA",
        ),
    ] {
        let ring = KeyRing::new(Arc::new(MemoryAuthStore::default()), "https://owned.test")
            .with_key_pair_config(KeyPairConfig::new("ES256", 2048).unwrap());
        let signed = ring.sign_jwt(Some(&body), now()).await.unwrap();
        assert_eq!(
            crate::keyring::verify::compact(signed["token"].as_str().unwrap())
                .unwrap()
                .0["alg"],
            expected
        );
    }
}
