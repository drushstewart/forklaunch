use super::*;
use crate::{
    keyring::adapter::{JwksAdapter, JwksKeyMaterial},
    store::memory::MemoryAuthTransaction,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey};
use std::sync::Arc;
struct NoStore;
impl AuthStore for NoStore {
    type Transaction<'a> = MemoryAuthTransaction;
    async fn begin(&self, _: bool) -> BlueprintResult<Self::Transaction<'_>> {
        panic!("Resolved signing must never access storage")
    }
}
fn record(now: DateTime<Utc>) -> JwksKeyRecord {
    let key = SigningKey::generate_config("source", "ES256", 2048).unwrap();
    JwksKeyRecord::new(
        "resolved",
        JwksKeyMaterial::new(
            "ES256",
            now,
            Some(now - chrono::Duration::seconds(1)),
            key.private_pkcs8().to_vec(),
        )
        .unwrap(),
    )
    .unwrap()
}
#[tokio::test]
async fn expired_removed_capability_reuses_without_repository_or_callbacks() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let source = record(now);
    let key = ResolvedSigningKey::from_record(&source).unwrap();
    let public = key.public_jwk();
    assert_eq!(public["kid"], "resolved");
    assert_eq!(public["alg"], "ES256");
    assert!(public.get("d").is_none());
    drop(source);
    let adapter = JwksAdapter::new("tenant")
        .unwrap()
        .with_get_jwks(|_| async { panic!("No read callback") })
        .with_create_jwk(|_, _| async { panic!("No create callback") });
    let ring = KeyRing::new(Arc::new(NoStore), "https://auth.test").with_jwks_adapter(adapter);
    for _ in 0..3 {
        let result = ring
            .sign_jwt_with_resolved_key(
                Some(&json!({"payload":{"sub":"owner"},
            "overrideOptions":{"jwks":{"keyPairConfig":{"alg":"EdDSA"}}}})),
                now,
                &key,
            )
            .await
            .unwrap();
        let token = result["token"].as_str().unwrap();
        let parts: Vec<_> = token.split('.').collect();
        let header: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header, json!({"alg":"ES256","kid":"resolved"}));
        // Verify independently with the public elliptic-curve implementation.
        let x = URL_SAFE_NO_PAD
            .decode(public["x"].as_str().unwrap())
            .unwrap();
        let y = URL_SAFE_NO_PAD
            .decode(public["y"].as_str().unwrap())
            .unwrap();
        let point = [vec![4], x, y].concat();
        let verifier = UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &point);
        verifier
            .verify(
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &URL_SAFE_NO_PAD.decode(parts[2]).unwrap(),
            )
            .unwrap();
    }
    for body in [
        json!({"payload":{"iat":"invalid"}}),
        json!({"payload":[]}),
        json!({"payload":{"iss":42}}),
    ] {
        assert!(
            ring.sign_jwt_with_resolved_key(Some(&body), now, &key)
                .await
                .is_err()
        );
    }
    assert!(
        ring.sign_jwt_with_resolved_key(None, now, &key)
            .await
            .is_err()
    );
}
#[test]
fn invalid_private_material_cannot_become_a_capability() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for algorithm in ["ES256", "unknown"] {
        let result = JwksKeyMaterial::new(algorithm, now, None, b"private-secret".to_vec());
        let error = result.err().unwrap();
        assert!(!error.message.contains("private-secret"));
    }
}
