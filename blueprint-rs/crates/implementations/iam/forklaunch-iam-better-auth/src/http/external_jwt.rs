//! Explicit server-owned external signing for authenticated session tokens.
use super::{AuthHttp, AuthStore, BlueprintError, BlueprintResult, Value};
use crate::keyring::custom::JwtSigner;
use std::{future::Future, pin::Pin, sync::Arc};

type SigningFuture = Pin<Box<dyn Future<Output = BlueprintResult<String>> + Send>>;
type SignCallback = dyn Fn(Value) -> SigningFuture + Send + Sync;

/// External signing configuration. The application publishes the advertised keys;
/// this adapter neither fetches the URL nor exposes locally managed keys.
/// The callback owns claim policy, algorithm enforcement and request deadlines.
pub struct ExternalJwtSigner {
    remote_jwks_url: String,
    algorithm: String,
    sign: Arc<SignCallback>,
}
impl ExternalJwtSigner {
    /// # Errors
    /// Requires an absolute HTTP(S) public-key URL and an explicit asymmetric
    /// signing algorithm. Credentials and fragments are rejected in key URLs.
    pub fn new<T: JwtSigner + 'static>(
        remote_jwks_url: impl Into<String>,
        algorithm: impl Into<String>,
        signer: T,
    ) -> BlueprintResult<Self> {
        let remote_jwks_url = remote_jwks_url.into();
        let algorithm = algorithm.into();
        let valid_url = url::Url::parse(&remote_jwks_url).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none()
        });
        if !valid_url
            || !matches!(
                algorithm.as_str(),
                "EdDSA"
                    | "Ed25519"
                    | "ES256"
                    | "ES384"
                    | "ES512"
                    | "RS256"
                    | "RS384"
                    | "RS512"
                    | "PS256"
                    | "PS384"
                    | "PS512"
            )
        {
            return Err(BlueprintError::new(
                "better_auth_invalid_config",
                "External JWT signing requires a public-key URL and asymmetric algorithm",
            ));
        }
        let signer = Arc::new(signer);
        Ok(Self {
            remote_jwks_url,
            algorithm,
            sign: Arc::new(move |payload| {
                let signer = signer.clone();
                Box::pin(async move { signer.sign(payload).await })
            }),
        })
    }
    #[must_use]
    pub fn remote_jwks_url(&self) -> &str {
        &self.remote_jwks_url
    }
    #[must_use]
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }
}
impl JwtSigner for ExternalJwtSigner {
    fn sign(&self, payload: Value) -> impl Future<Output = BlueprintResult<String>> + Send {
        (self.sign)(payload)
    }
}
impl<S: AuthStore + 'static> AuthHttp<S> {
    pub(super) async fn jwks(&self, now: chrono::DateTime<chrono::Utc>) -> BlueprintResult<Value> {
        if self.external_jwt.is_some() {
            return Err(BlueprintError::new("HTTP_NOT_FOUND", "Not Found"));
        }
        crate::keyring::KeyRing::new(self.store.clone(), self.auth_origin())
            .jwks(now)
            .await
    }

    /// Uses an external signer for `/token` and the `get-session` JWT header.
    /// Session/cookie authorization still precedes signing. The local `/jwks`
    /// route returns 404, matching Better Auth's remote-JWKS configuration.
    /// Configure application JWT verification separately with the external keys.
    #[must_use]
    pub fn with_external_jwt_signer(mut self, signer: ExternalJwtSigner) -> Self {
        self.external_jwt = Some(signer);
        self
    }
}
