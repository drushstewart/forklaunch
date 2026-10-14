use super::*;
use crate::{
    jwt::SigningKey,
    keyring::{
        KeyRing,
        adapter::{
            JwksAdapter, JwksKeyMaterial, JwksKeyRecord, JwksPublicKeyRecord, JwksReadRecord,
        },
    },
    store::{
        AuthStore, AuthTransaction, Model,
        memory::{MemoryAuthStore, MemoryAuthTransaction},
    },
};
use chrono::{DateTime, Utc};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
fn now() -> DateTime<Utc> {
    DateTime::from_timestamp(1_800_000_000, 0).unwrap()
}
fn body() -> Value {
    json!({"payload":{"sub":"owner"}})
}
fn config() -> KeyPairConfig {
    KeyPairConfig::default()
        .with_additional_profiles(vec![KeyPairConfig::new("ES256", 2048).unwrap()])
        .unwrap()
}
fn header(signed: &Value) -> Value {
    crate::keyring::verify::compact(signed["token"].as_str().unwrap())
        .unwrap()
        .0
}
#[tokio::test]
async fn additional_algorithm_mints_once_reuses_existing_and_never_falls_back() {
    let store = Arc::new(MemoryAuthStore::default());
    let default = KeyRing::new(store.clone(), "https://owned.test");
    assert!(
        default
            .sign_jwt_with_algorithm(Some(&body()), "ES256", now())
            .await
            .is_err()
    );
    let ring = KeyRing::new(store.clone(), "https://owned.test").with_key_pair_config(config());
    let first = ring
        .sign_jwt_with_algorithm(Some(&body()), "ES256", now())
        .await
        .unwrap();
    assert_eq!(header(&first)["alg"], "ES256");
    let second = default
        .sign_jwt_with_algorithm(Some(&body()), "ES256", now())
        .await
        .unwrap();
    assert_eq!(header(&first), header(&second));
    assert!(
        ring.sign_jwt_with_algorithm(Some(&body()), "RS256", now())
            .await
            .is_err()
    );
    assert!(
        ring.sign_jwt_with_key_id(
            Some(&body()),
            header(&first)["kid"].as_str().unwrap(),
            Some("EdDSA"),
            now()
        )
        .await
        .is_err()
    );
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(tx.list(Model::Jwks).await.unwrap().len(), 1);
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn expired_advertised_extra_replaces_but_empty_jwks_override_clears_advertisement() {
    let store = Arc::new(MemoryAuthStore::default());
    let profile = config()
        .with_rotation_interval(std::time::Duration::from_millis(1))
        .unwrap();
    let ring = KeyRing::new(store, "https://owned.test").with_key_pair_config(profile);
    let first = ring
        .sign_jwt_with_algorithm(Some(&body()), "ES256", now())
        .await
        .unwrap();
    let second = ring
        .sign_jwt_with_algorithm(
            Some(&body()),
            "ES256",
            now() + chrono::Duration::milliseconds(1),
        )
        .await
        .unwrap();
    assert_ne!(header(&first)["kid"], header(&second)["kid"]);
    let fresh = KeyRing::new(Arc::new(MemoryAuthStore::default()), "https://owned.test")
        .with_key_pair_config(config());
    assert!(
        fresh
            .sign_jwt_with_algorithm(
                Some(&json!({"payload":{},"overrideOptions":{"jwks":{}}})),
                "ES256",
                now()
            )
            .await
            .is_err()
    );
}
struct UnavailableStore;
impl AuthStore for UnavailableStore {
    type Transaction<'a> = MemoryAuthTransaction;
    async fn begin(&self, _: bool) -> BlueprintResult<Self::Transaction<'_>> {
        panic!("custom hooks/invalid constraints must not access default store")
    }
}
#[tokio::test]
async fn typed_profiles_reject_invalid_authority_before_storage() {
    assert!(
        KeyPairConfig::default()
            .with_additional_profiles(vec![KeyPairConfig::default()])
            .is_err()
    );
    let es = KeyPairConfig::new("ES256", 2048).unwrap();
    assert!(
        KeyPairConfig::default()
            .with_additional_profiles(vec![es.clone(), es.clone()])
            .is_err()
    );
    assert!(
        KeyPairConfig::default()
            .with_additional_profiles(vec![es.clone(); 17])
            .is_err()
    );
    assert!(
        KeyPairConfig::new("RS256", 2048)
            .unwrap()
            .with_additional_profiles(vec![config()])
            .is_err()
    );
    assert!(
        KeyPairConfig::default()
            .with_additional_profiles(vec![
                es.with_rotation_interval(std::time::Duration::from_secs(1))
                    .unwrap()
            ])
            .is_err()
    );
    let ring = KeyRing::new(Arc::new(UnavailableStore), "https://owned.test");
    assert!(
        ring.sign_jwt_with_algorithm(Some(&body()), "HS256", now())
            .await
            .is_err()
    );
    assert!(
        ring.sign_jwt_with_algorithm(
            Some(&json!({"payload":{},"overrideOptions":{"jwks":{"keyPairConfigs":[]}}})),
            "ES256",
            now()
        )
        .await
        .is_err()
    );
}
#[tokio::test]
async fn custom_algorithm_creation_is_one_read_and_returned_key_is_authoritative() {
    let reads = Arc::new(AtomicUsize::new(0));
    let count = reads.clone();
    let key = SigningKey::generate_config("returned", "EdDSA", 2048).unwrap();
    let record = JwksKeyRecord::new(
        "returned",
        JwksKeyMaterial::new("EdDSA", now(), None, key.private_pkcs8().to_vec()).unwrap(),
    )
    .unwrap();
    let adapter = JwksAdapter::new("tenant")
        .unwrap()
        .with_get_jwks(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            async { Ok(Some(vec![])) }
        })
        .with_create_jwk(move |material, _| {
            assert_eq!(material.algorithm(), "ES256");
            let record = record.clone();
            async move { Ok(record) }
        });
    let ring = KeyRing::new(Arc::new(UnavailableStore), "https://owned.test")
        .with_key_pair_config(config())
        .with_jwks_adapter(adapter);
    let signed = ring
        .sign_jwt_with_algorithm(Some(&body()), "ES256", now())
        .await
        .unwrap();
    assert_eq!(header(&signed)["alg"], "EdDSA");
    assert_eq!(header(&signed)["kid"], "returned");
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn public_only_matching_algorithm_fails_without_mint() {
    let key = SigningKey::generate_config("public", "ES256", 2048).unwrap();
    let record =
        JwksPublicKeyRecord::new("public", "ES256", now(), None, &key.public_jwk()).unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let count = reads.clone();
    let adapter = JwksAdapter::new("tenant")
        .unwrap()
        .with_get_jwks_records(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            let record = record.clone();
            async move { Ok(Some(vec![JwksReadRecord::Public(record)])) }
        })
        .with_create_jwk(|_, _| async { panic!("selected public key must not cause replacement") });
    let ring = KeyRing::new(Arc::new(UnavailableStore), "https://owned.test")
        .with_key_pair_config(config())
        .with_jwks_adapter(adapter);
    assert!(
        ring.sign_jwt_with_algorithm(Some(&body()), "ES256", now())
            .await
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}
