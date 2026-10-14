//! Trusted native key-repository callbacks. Private material is plaintext PKCS#8
//! inside the callback boundary; callback owners must protect their persistence.
use super::{
    BlueprintError, BlueprintResult, DateTime, STANDARD, SigningKey, Utc, Value, key_error,
    timestamp,
};
use base64::Engine;
use std::{future::Future, pin::Pin, sync::Arc};
use zeroize::Zeroizing;

/// Fixed server-owned namespace and invocation metadata; no request fields.
#[derive(Clone, Debug)]
pub struct JwksAdapterContext {
    pub tenant: String,
    pub base_url: String,
    pub now: DateTime<Utc>,
}
/// Secret-bearing native key material. No Debug or serialization implementation.
/// Cloning copies secret bytes; each owned byte buffer is zeroized on drop.
#[derive(Clone)]
pub struct JwksKeyMaterial {
    algorithm: String,
    created_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
    private_pkcs8: Zeroizing<Vec<u8>>,
}
impl JwksKeyMaterial {
    /// Imports native plaintext PKCS#8 material, not Better Auth ciphertext/JWK JSON.
    /// # Errors
    /// Rejects unsupported algorithms or invalid key material.
    pub fn new(
        algorithm: impl Into<String>,
        created_at: DateTime<Utc>,
        expires_at: Option<DateTime<Utc>>,
        private_pkcs8: Vec<u8>,
    ) -> BlueprintResult<Self> {
        let algorithm = algorithm.into();
        let private_pkcs8 = Zeroizing::new(private_pkcs8);
        if private_pkcs8.len() > 6144
            || !created_at
                .timestamp_subsec_nanos()
                .is_multiple_of(1_000_000)
            || expires_at
                .is_some_and(|value| !value.timestamp_subsec_nanos().is_multiple_of(1_000_000))
        {
            return Err(key_error());
        }

        SigningKey::from_pkcs8_profile("validate", &private_pkcs8, &algorithm)?;
        Ok(Self {
            algorithm,
            created_at,
            expires_at,
            private_pkcs8,
        })
    }
    #[must_use]
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    #[must_use]
    pub const fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.expires_at
    }
    /// Trusted persistence only: never log or return these bytes in HTTP responses.
    #[must_use]
    pub fn private_pkcs8(&self) -> &[u8] {
        &self.private_pkcs8
    }
    pub(super) fn from_row(row: &Value) -> BlueprintResult<Self> {
        let key = super::restore(row)?;
        Self::new(
            row["alg"].as_str().unwrap_or("EdDSA"),
            super::required_time(row, "createdAt")?,
            super::optional_time(row, "expiresAt")?,
            key.private_pkcs8().to_vec(),
        )
    }
}
/// A callback-owned record with an authoritative ID and validated private material.
#[derive(Clone)]
pub struct JwksKeyRecord {
    id: String,
    material: JwksKeyMaterial,
}
impl JwksKeyRecord {
    /// # Errors
    /// Rejects empty/oversized IDs. The callback owns persistence before returning.
    pub fn new(id: impl Into<String>, material: JwksKeyMaterial) -> BlueprintResult<Self> {
        let id = id.into();
        if id.is_empty() || id.len() > 512 {
            return Err(key_error());
        }
        Ok(Self { id, material })
    }
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
    #[must_use]
    pub const fn material(&self) -> &JwksKeyMaterial {
        &self.material
    }
    pub(super) fn row(&self) -> BlueprintResult<Value> {
        let key = SigningKey::from_pkcs8_profile(
            &self.id,
            &self.material.private_pkcs8,
            &self.material.algorithm,
        )?;
        let public = key.public_jwk();
        let mut row = serde_json::json!({"id":self.id,"alg":self.material.algorithm,"createdAt":timestamp(self.material.created_at),"privateKey":STANDARD.encode(&*self.material.private_pkcs8),"publicKey":serde_json::to_string(&public).map_err(|_|key_error())?});
        if let Some(curve) = public.get("crv") {
            row["crv"] = curve.clone();
        }
        if let Some(expiry) = self.material.expires_at {
            row["expiresAt"] = serde_json::json!(timestamp(expiry));
        }
        Ok(row)
    }
}
type CallbackFuture<T> = Pin<Box<dyn Future<Output = BlueprintResult<T>> + Send>>;
type Read =
    Arc<dyn Fn(JwksAdapterContext) -> CallbackFuture<Option<Vec<JwksKeyRecord>>> + Send + Sync>;
type Create =
    Arc<dyn Fn(JwksKeyMaterial, JwksAdapterContext) -> CallbackFuture<JwksKeyRecord> + Send + Sync>;
/// Independent trusted read/create hooks. Bind this adapter to the same tenant as
/// the `AuthStore`; the fixed label is not a substitute for backend isolation.
#[derive(Clone)]
pub struct JwksAdapter {
    pub(super) tenant: String,
    pub(super) read: Option<Read>,
    pub(super) create: Option<Create>,
}
impl JwksAdapter {
    /// # Errors
    /// Rejects empty/oversized server-owned tenant labels.
    pub fn new(tenant: impl Into<String>) -> BlueprintResult<Self> {
        let tenant = tenant.into();
        if tenant.trim().is_empty() || tenant.len() > 512 {
            return Err(adapter_error());
        }
        Ok(Self {
            tenant,
            read: None,
            create: None,
        })
    }
    #[must_use]
    pub fn with_get_jwks<F, Fut>(mut self, callback: F) -> Self
    where
        F: Fn(JwksAdapterContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = BlueprintResult<Option<Vec<JwksKeyRecord>>>> + Send + 'static,
    {
        self.read = Some(Arc::new(move |context| Box::pin(callback(context))));
        self
    }
    #[must_use]
    pub fn with_create_jwk<F, Fut>(mut self, callback: F) -> Self
    where
        F: Fn(JwksKeyMaterial, JwksAdapterContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = BlueprintResult<JwksKeyRecord>> + Send + 'static,
    {
        self.create = Some(Arc::new(move |material, context| {
            Box::pin(callback(material, context))
        }));
        self
    }
    pub(super) fn active(&self) -> bool {
        self.read.is_some() || self.create.is_some()
    }
}
pub(super) fn adapter_error() -> BlueprintError {
    BlueprintError::new(
        "better_auth_jwks_adapter_failed",
        "Custom key repository failed",
    )
}

#[cfg(test)]
mod tests;
