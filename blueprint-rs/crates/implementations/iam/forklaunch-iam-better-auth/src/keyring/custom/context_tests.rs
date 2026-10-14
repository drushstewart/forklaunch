use super::*;
use crate::store::memory::MemoryAuthTransaction;
use std::sync::{Arc, Mutex};
struct UnavailableStore;
impl AuthStore for UnavailableStore {
    type Transaction<'a> = MemoryAuthTransaction;
    async fn begin(&self, _: bool) -> BlueprintResult<Self::Transaction<'_>> {
        panic!("external metadata must not access local keys")
    }
}
struct CapturingSigner(Arc<Mutex<Vec<JwtSigningContext>>>);
impl JwtSigner for CapturingSigner {
    async fn sign(&self, _: Value) -> BlueprintResult<String> {
        panic!("context-aware dispatch required")
    }
    async fn sign_with_context(
        &self,
        payload: Value,
        context: JwtSigningContext,
    ) -> BlueprintResult<String> {
        assert_eq!(payload["sub"], "identity");
        self.0.lock().unwrap().push(context);
        Ok("opaque-external-token".into())
    }
}
#[tokio::test]
async fn external_context_preserves_headers_hints_and_wrapper_without_store_access() {
    let captures = Arc::new(Mutex::new(Vec::new()));
    let signer = CapturingSigner(captures.clone());
    let ring = KeyRing::new(Arc::new(UnavailableStore), "https://auth.test");
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let body = json!({"payload":{"sub":"identity"}});
    let header = json!({"kid":"header-id","alg":"header-alg","typ":null,"extra":[true,1]});
    let context = JwtSigningContext::new(
        header.as_object().cloned(),
        Some("missing".into()),
        Some("unrecognized-algorithm".into()),
    )
    .unwrap();
    let result = ring
        .sign_jwt_with_signer_context(Some(&body), now, &signer, context.clone())
        .await
        .unwrap();
    assert_eq!(result["token"], "opaque-external-token");
    ring.sign_jwt_with_signer(Some(&body), now, &signer)
        .await
        .unwrap();
    #[cfg(feature = "http")]
    {
        let external =
            crate::http::ExternalJwtSigner::new("https://keys.test/jwks", "ES256", signer).unwrap();
        ring.sign_jwt_with_signer_context(Some(&body), now, &external, context)
            .await
            .unwrap();
    }
    let observations = captures.lock().unwrap();
    assert_eq!(observations[0].header(), header.as_object());
    assert_eq!(observations[0].signing_key_id(), Some("missing"));
    assert_eq!(
        observations[0].signing_algorithm(),
        Some("unrecognized-algorithm")
    );
    assert!(observations[1].header().is_none());
    assert!(observations[1].signing_key_id().is_none());
    assert!(observations[1].signing_algorithm().is_none());
    #[cfg(feature = "http")]
    assert_eq!(observations[2].header(), header.as_object());
}
struct LegacySigner;
impl JwtSigner for LegacySigner {
    async fn sign(&self, _: Value) -> BlueprintResult<String> {
        Ok("legacy".into())
    }
}
struct Failing;
impl JwtSigner for Failing {
    async fn sign(&self, _: Value) -> BlueprintResult<String> {
        panic!("must use context callback")
    }
    async fn sign_with_context(&self, _: Value, _: JwtSigningContext) -> BlueprintResult<String> {
        Err(BlueprintError::new("secret", "private key details"))
    }
}
#[tokio::test]
async fn legacy_signers_keep_default_dispatch_and_callback_errors_are_sanitized() {
    let ring = KeyRing::new(Arc::new(UnavailableStore), "https://auth.test");
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let context = JwtSigningContext::new(
        Some(serde_json::Map::default()),
        Some(String::new()),
        Some(String::new()),
    )
    .unwrap();
    assert_eq!(
        ring.sign_jwt_with_signer_context(
            Some(&json!({"payload":{}})),
            now,
            &LegacySigner,
            context
        )
        .await
        .unwrap()["token"],
        "legacy"
    );
    let error = ring
        .sign_jwt_with_signer(Some(&json!({"payload":{}})), now, &Failing)
        .await
        .unwrap_err();
    assert_eq!(error.code, "better_auth_custom_signer_failed");
    assert!(!error.message.contains("private"));
}
#[test]
fn trusted_context_bounds_preserve_empty_and_explicit_null() {
    assert!(JwtSigningContext::new(None, Some("x".repeat(513)), None).is_err());
    assert!(JwtSigningContext::new(None, None, Some("x".repeat(129))).is_err());
    for value in [
        json!({"large":"x".repeat(16385)}),
        json!({"wide":vec![Value::Null;1025]}),
        json!({"escaped":"\0".repeat(16380)}),
    ] {
        assert!(JwtSigningContext::new(value.as_object().cloned(), None, None).is_err());
    }
    let mut nested = Value::Null;
    for _ in 0..34 {
        nested = json!([nested]);
    }
    assert!(
        JwtSigningContext::new(json!({"nested":nested}).as_object().cloned(), None, None).is_err()
    );
    assert!(
        JwtSigningContext::new(Some(serde_json::Map::default()), None, None)
            .unwrap()
            .header()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        JwtSigningContext::new(json!({"typ":null}).as_object().cloned(), None, None)
            .unwrap()
            .header()
            .unwrap()["typ"],
        Value::Null
    );
}
