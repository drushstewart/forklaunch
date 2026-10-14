use super::*;
use crate::{
    keyring::KeyRing,
    store::{
        AuthStore, AuthTransaction, Model,
        memory::{MemoryAuthStore, MemoryAuthTransaction},
    },
};
use serde_json::json;
use std::sync::Mutex;
fn time() -> DateTime<Utc> {
    DateTime::from_timestamp(1_800_000_000, 0).unwrap()
}
fn material() -> JwksKeyMaterial {
    let key = SigningKey::generate_config("fixture", "EdDSA", 2048).unwrap();
    JwksKeyMaterial::new("EdDSA", time(), None, key.private_pkcs8().to_vec()).unwrap()
}
struct UnavailableStore;
impl AuthStore for UnavailableStore {
    type Transaction<'a> = MemoryAuthTransaction;
    async fn begin(&self, _: bool) -> BlueprintResult<Self::Transaction<'_>> {
        panic!("both callbacks must bypass the default store")
    }
}
#[tokio::test]
async fn both_hooks_share_returned_authority_publication_verification_and_fixed_context() {
    let records = Arc::new(Mutex::new(Vec::<JwksKeyRecord>::new()));
    let read = records.clone();
    let write = records.clone();
    let replacement = material();
    let adapter = JwksAdapter::new("tenant-a")
        .unwrap()
        .with_get_jwks(move |ctx| {
            assert_eq!(ctx.tenant, "tenant-a");
            assert_eq!(ctx.base_url, "https://owned.test");
            assert_eq!(ctx.now, time());
            let read = read.clone();
            async move { Ok(Some(read.lock().unwrap().clone())) }
        })
        .with_create_jwk(move |generated, ctx| {
            assert_eq!(ctx.tenant, "tenant-a");
            assert_eq!(
                generated.expires_at(),
                Some(time() + chrono::Duration::seconds(2))
            );
            let record = JwksKeyRecord::new("returned-authority", replacement.clone()).unwrap();
            let write = write.clone();
            async move {
                write.lock().unwrap().push(record.clone());
                Ok(record)
            }
        });
    let ring =
        KeyRing::new(Arc::new(UnavailableStore), "https://owned.test").with_jwks_adapter(adapter);
    let signed=ring.sign_jwt(Some(&json!({"payload":{"sub":"owner"},"overrideOptions":{"jwks":{"rotationInterval":2}}})),time()).await.unwrap();
    assert_eq!(
        ring.jwks(time()).await.unwrap()["keys"][0]["kid"],
        "returned-authority"
    );
    assert_eq!(
        ring.verify_jwt(Some(&signed), time()).await.unwrap()["payload"]["sub"],
        "owner"
    );
    assert!(ring.rotate(time()).await.is_err());
    assert_eq!(records.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn independent_create_hook_runs_without_default_lock_and_can_persist_returned_record() {
    let store = Arc::new(MemoryAuthStore::default());
    let callback_store = store.clone();
    let adapter = JwksAdapter::new("tenant-a")
        .unwrap()
        .with_create_jwk(move |material, _| {
            let store = callback_store.clone();
            async move {
                let record = JwksKeyRecord::new("callback-created", material)?;
                let mut tx = store.begin(true).await?;
                tx.insert(Model::Jwks, record.row()?).await?;
                tx.commit().await?;
                Ok(record)
            }
        });
    let ring = KeyRing::new(store.clone(), "https://owned.test").with_jwks_adapter(adapter);
    let signed = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ring.sign_jwt(Some(&json!({"payload":{"sub":"owner"}})), time()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        ring.jwks(time()).await.unwrap()["keys"][0]["kid"],
        "callback-created"
    );
    assert_eq!(
        ring.verify_jwt(Some(&signed), time()).await.unwrap()["payload"]["sub"],
        "owner"
    );
    assert!(ring.rotate(time()).await.is_err());
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(tx.list(Model::Jwks).await.unwrap().len(), 1);
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn read_only_hook_owns_reads_and_does_not_hide_independent_default_creation() {
    let store = Arc::new(MemoryAuthStore::default());
    let adapter = JwksAdapter::new("tenant-b")
        .unwrap()
        .with_get_jwks(|ctx| async move {
            assert_eq!(ctx.tenant, "tenant-b");
            Ok(None)
        });
    let ring = KeyRing::new(store.clone(), "https://owned.test").with_jwks_adapter(adapter);
    assert!(ring.jwks(time()).await.is_err());
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(tx.list(Model::Jwks).await.unwrap().len(), 1);
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn callback_failures_are_sanitized_and_oversized_custom_windows_reject() {
    for read in [true, false] {
        let adapter = JwksAdapter::new("tenant").unwrap();
        let adapter = if read {
            adapter.with_get_jwks(|_| async {
                Err(BlueprintError::new("private-code", "private key details"))
            })
        } else {
            adapter.with_create_jwk(|_, _| async {
                Err(BlueprintError::new("private-code", "private key details"))
            })
        };
        let ring = KeyRing::new(Arc::new(MemoryAuthStore::default()), "https://owned.test")
            .with_jwks_adapter(adapter);
        let error = ring.jwks(time()).await.unwrap_err();
        assert_eq!(error.code, "better_auth_jwks_adapter_failed");
        assert!(!error.message.contains("private"));
    }
    let record = JwksKeyRecord::new("record", material()).unwrap();
    let adapter = JwksAdapter::new("tenant").unwrap().with_get_jwks(move |_| {
        let record = record.clone();
        async move { Ok(Some(vec![record; 4097])) }
    });
    assert!(
        KeyRing::new(Arc::new(UnavailableStore), "https://owned.test")
            .with_jwks_adapter(adapter)
            .jwks(time())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn custom_reads_are_not_truncated_to_default_window_and_select_live_primary() {
    let key = material();
    let expired = JwksKeyMaterial::new(
        "EdDSA",
        time() - chrono::Duration::days(40),
        Some(time() - chrono::Duration::days(31)),
        key.private_pkcs8().to_vec(),
    )
    .unwrap();
    let mut records = (0..100)
        .map(|index| JwksKeyRecord::new(format!("expired-{index}"), expired.clone()).unwrap())
        .collect::<Vec<_>>();
    records.push(JwksKeyRecord::new("live-primary", key).unwrap());
    let ec = SigningKey::generate_config("ec", "ES256", 2048).unwrap();
    records.push(
        JwksKeyRecord::new(
            "newer-other-algorithm",
            JwksKeyMaterial::new(
                "ES256",
                time() + chrono::Duration::seconds(1),
                None,
                ec.private_pkcs8().to_vec(),
            )
            .unwrap(),
        )
        .unwrap(),
    );

    let adapter = JwksAdapter::new("tenant").unwrap().with_get_jwks(move |_| {
        let records = records.clone();
        async move { Ok(Some(records)) }
    });
    let ring =
        KeyRing::new(Arc::new(UnavailableStore), "https://owned.test").with_jwks_adapter(adapter);
    let token = ring
        .issue("owner", "owner@example.test", None, time())
        .await
        .unwrap();
    assert_eq!(
        ring.verify_jwt(Some(&json!({"token":token})), time())
            .await
            .unwrap()["payload"]["sub"],
        "owner"
    );
    assert_eq!(
        ring.jwks(time()).await.unwrap()["keys"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        ring.jwks(time()).await.unwrap()["keys"][0]["kid"],
        "live-primary"
    );
    let signed=ring.sign_jwt(Some(&json!({"payload":{"sub":"ec-owner"},"overrideOptions":{"jwks":{"keyPairConfig":{"alg":"ES256"}}}})),time()).await.unwrap();
    assert_eq!(
        ring.verify_jwt(Some(&signed), time()).await.unwrap()["payload"]["sub"],
        "ec-owner"
    );
    let header: Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(signed["token"].as_str().unwrap().split('.').next().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(header["kid"], "newer-other-algorithm");
}

#[tokio::test]
async fn http_token_header_and_jwks_share_fixed_custom_repository() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let store = Arc::new(MemoryAuthStore::default());
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"owner","email":"owner@example.test"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let session = crate::sessions::SessionService::new(store.clone())
        .create("owner", "", "", time())
        .await
        .unwrap();
    let cookie = crate::cookies::SessionCookie { secure: false }.serialize(
        session["token"].as_str().unwrap(),
        &[42; 32],
        true,
    );
    let record = JwksKeyRecord::new("http-custom", material()).unwrap();
    let adapter = JwksAdapter::new("fixed-http-tenant")
        .unwrap()
        .with_get_jwks(move |ctx| {
            assert_eq!(ctx.tenant, "fixed-http-tenant");
            let record = record.clone();
            async move { Ok(Some(vec![record])) }
        });
    let router = crate::http::AuthHttp::new(
        store.clone(),
        "http://localhost:3000".into(),
        vec![42; 32],
        vec![],
    )
    .unwrap()
    .with_clock(time)
    .with_jwks_adapter(adapter)
    .router();
    for path in ["/token", "/get-session", "/jwks"] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("cookie", cookie.split(';').next().unwrap())
                    .header("x-tenant-id", "attacker-tenant")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let header = response
            .headers()
            .get("set-auth-jwt")
            .map(|value| value.to_str().unwrap().to_owned());
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        if path == "/jwks" {
            assert_eq!(body["keys"][0]["kid"], "http-custom");
        } else {
            let token = header.unwrap_or_else(|| body["token"].as_str().unwrap().into());
            let decoded: Value = serde_json::from_slice(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(token.split('.').next().unwrap())
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(decoded["kid"], "http-custom");
        }
    }
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.list(Model::Jwks).await.unwrap().is_empty());
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn custom_latest_fallback_reads_again_and_uses_second_snapshot() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let reads = Arc::new(AtomicUsize::new(0));
    let callback_reads = reads.clone();
    let record = JwksKeyRecord::new("second-snapshot", material()).unwrap();
    let adapter = JwksAdapter::new("tenant").unwrap().with_get_jwks(move |_| {
        let count = callback_reads.fetch_add(1, Ordering::SeqCst);
        let record = record.clone();
        async move { Ok(Some(if count == 0 { vec![] } else { vec![record] })) }
    });
    let ring =
        KeyRing::new(Arc::new(UnavailableStore), "https://owned.test").with_jwks_adapter(adapter);
    let signed = ring
        .sign_jwt(Some(&json!({"payload":{"sub":"fallback"}})), time())
        .await
        .unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    assert_eq!(
        ring.verify_jwt(Some(&signed), time()).await.unwrap()["payload"]["sub"],
        "fallback"
    );
}

#[tokio::test]
async fn public_only_snapshot_publishes_verifies_and_never_falls_back_when_signing() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let key = SigningKey::generate_config("public-only", "EdDSA", 2048).unwrap();
    let private = JwksKeyRecord::new(
        "public-only",
        JwksKeyMaterial::new("EdDSA", time(), None, key.private_pkcs8().to_vec()).unwrap(),
    )
    .unwrap();
    let signer = KeyRing::new(Arc::new(UnavailableStore), "https://owned.test").with_jwks_adapter(
        JwksAdapter::new("tenant").unwrap().with_get_jwks(move |_| {
            let private = private.clone();
            async move { Ok(Some(vec![private])) }
        }),
    );
    let token = signer
        .sign_jwt(Some(&json!({"payload":{"sub":"owner"}})), time())
        .await
        .unwrap();
    let public =
        JwksPublicKeyRecord::new("public-only", "EdDSA", time(), None, &key.public_jwk()).unwrap();
    let adapter = JwksAdapter::new("tenant")
        .unwrap()
        .with_get_jwks_records(move |_| {
            let public = public.clone();
            async move { Ok(Some(vec![JwksReadRecord::Public(public)])) }
        });
    let ring = KeyRing::new(Arc::new(UnavailableStore), "https://owned.test")
        .with_jwks_adapter(adapter.clone());
    let store = Arc::new(MemoryAuthStore::default());
    let app = crate::http::AuthHttp::new(
        store.clone(),
        "http://localhost:3000".into(),
        vec![42; 32],
        vec![],
    )
    .unwrap()
    .with_clock(time)
    .with_jwks_adapter(adapter);
    let response = app
        .router()
        .oneshot(Request::builder().uri("/jwks").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let published: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(published["keys"][0], key.public_jwk());
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.list(Model::Jwks).await.unwrap().is_empty());
    tx.commit().await.unwrap();
    assert_eq!(
        ring.jwks(time()).await.unwrap()["keys"][0],
        key.public_jwk()
    );
    assert_eq!(
        ring.verify_jwt(Some(&token), time()).await.unwrap()["payload"]["sub"],
        "owner"
    );
    assert!(
        ring.sign_jwt(Some(&json!({"payload":{"sub":"owner"}})), time())
            .await
            .is_err()
    );
}

#[test]
fn public_only_rejects_private_unknown_mismatched_and_unbounded_jwk_fields() {
    let key = SigningKey::generate_config("public-only", "EdDSA", 2048).unwrap();
    for (field, value) in [
        ("d", json!("secret")),
        ("extra", json!(true)),
        ("kid", json!("other")),
        ("x", json!("a".repeat(2049))),
    ] {
        let mut public = key.public_jwk();
        public[field] = value;
        assert!(JwksPublicKeyRecord::new("public-only", "EdDSA", time(), None, &public).is_err());
    }
}
