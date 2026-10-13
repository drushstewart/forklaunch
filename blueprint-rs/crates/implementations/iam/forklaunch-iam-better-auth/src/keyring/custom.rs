//! Trusted external JWT signing without local key selection or persistence.
use super::{AuthStore, BlueprintError, BlueprintResult, DateTime, KeyRing, Utc, Value, json};
use std::future::Future;

/// Server-owned signer integration (for example, an external key-management service).
/// The implementation owns claim policy, signing algorithm and key publication.
pub trait JwtSigner: Send + Sync {
    /// # Errors
    /// Returns a signing failure. Public callers receive a sanitized error.
    fn sign(&self, payload: Value) -> impl Future<Output = BlueprintResult<String>> + Send;
}
impl<S: AuthStore> KeyRing<S> {
    /// Delegates a trusted payload to an explicitly supplied custom signer.
    /// Serialized options configure claims; they cannot replace the typed callback.
    /// Mirrors the custom-sign branch's defaults without local JOSE claim setters,
    /// key generation or database access. This does not configure public HTTP/JWKS
    /// endpoints; applications must publish/verify the external signer's keys.
    /// # Errors
    /// Rejects malformed input, invalid duration/non-finite JSON claims, and returns
    /// sanitized callback errors. No local key is persisted on either outcome.
    pub async fn sign_jwt_with_signer<T: JwtSigner>(
        &self,
        body: Option<&Value>,
        now: DateTime<Utc>,
        signer: &T,
    ) -> BlueprintResult<Value> {
        let payload = super::sign::custom_payload(body, now, &self.base_url)?;
        let token = signer.sign(payload).await.map_err(|_| {
            BlueprintError::new(
                "better_auth_custom_signer_failed",
                "Custom JWT signing failed",
            )
        })?;
        Ok(json!({"token":token}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::memory::MemoryAuthTransaction;
    use std::{
        future::pending,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    struct UnavailableStore;
    impl AuthStore for UnavailableStore {
        type Transaction<'a> = MemoryAuthTransaction;
        async fn begin(&self, _: bool) -> BlueprintResult<Self::Transaction<'_>> {
            panic!("Custom signing must not access local storage")
        }
    }
    struct Signer {
        calls: AtomicUsize,
        fail: bool,
    }
    impl JwtSigner for Signer {
        async fn sign(&self, payload: Value) -> BlueprintResult<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(payload["iss"], 42);
            assert_eq!(payload["exp"], "raw-expiry");
            if self.fail {
                return Err(BlueprintError::new("private", "private signing key"));
            }
            Ok("externally-signed-token".into())
        }
    }
    #[tokio::test]
    async fn custom_signing_bypasses_storage_claim_setters_and_redacts_failures() {
        let ring = KeyRing::new(Arc::new(UnavailableStore), "https://auth.fixture.test");
        let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        let body = json!({"payload":{"iss":42,"exp":"raw-expiry"}});
        for fail in [false, true] {
            let signer = Signer {
                calls: AtomicUsize::new(0),
                fail,
            };
            let result = ring.sign_jwt_with_signer(Some(&body), now, &signer).await;
            if fail {
                let error = result.unwrap_err();
                assert_eq!(error.code, "better_auth_custom_signer_failed");
                assert!(!error.message.contains("private"));
            } else {
                assert_eq!(result.unwrap(), json!({"token":"externally-signed-token"}));
            }
            assert_eq!(signer.calls.load(Ordering::SeqCst), 1);
            assert!(ring.sign_jwt_with_signer(None, now, &signer).await.is_err());
            assert_eq!(signer.calls.load(Ordering::SeqCst), 1);
        }
    }
    struct WaitingSigner;
    impl JwtSigner for WaitingSigner {
        async fn sign(&self, _: Value) -> BlueprintResult<String> {
            pending().await
        }
    }
    #[tokio::test]
    async fn caller_can_bound_external_signer_wait_without_holding_database_locks() {
        let ring = KeyRing::new(Arc::new(UnavailableStore), "https://auth.fixture.test");
        let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        let body = json!({"payload":{}});
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(10),
                ring.sign_jwt_with_signer(Some(&body), now, &WaitingSigner)
            )
            .await
            .is_err()
        );
    }
}
