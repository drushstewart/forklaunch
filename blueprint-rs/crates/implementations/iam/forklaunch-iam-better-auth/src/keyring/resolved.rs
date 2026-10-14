//! Trusted caller-owned signing capabilities; no persistence or revocation checks.
use super::{
    AuthStore, BlueprintResult, DateTime, KeyRing, SigningKey, Utc, Value, adapter::JwksKeyRecord,
    json,
};

/// An imported private signing key. Intentionally neither Debug nor serializable.
/// Construction and reuse do not check record expiry or revocation. The trusted
/// caller owns capability lifetime; deleting its source record does not revoke it.
/// Public-only records and arbitrary JSON cannot construct this capability.
pub struct ResolvedSigningKey(SigningKey);
impl ResolvedSigningKey {
    /// Imports validated private material once, without consulting any repository.
    /// # Errors
    /// Returns a sanitized error for invalid private material or algorithm.
    pub fn from_record(record: &JwksKeyRecord) -> BlueprintResult<Self> {
        SigningKey::from_pkcs8_profile(
            record.id(),
            record.material().private_pkcs8(),
            record.material().algorithm(),
        )
        .map(Self)
    }
    /// Public verification metadata; never includes private material.
    #[must_use]
    pub fn public_jwk(&self) -> Value {
        self.0.public_jwk()
    }
}
impl<S: AuthStore> KeyRing<S> {
    /// Reuses an explicitly trusted capability without reads, callbacks or minting.
    /// Record expiry/removal is not rechecked. Key ID/algorithm come solely from
    /// the capability; the existing strict ID lookup API is unchanged.
    /// This local entrypoint does not configure or replace an HTTP signer.
    /// Callers with an external signer must choose the existing external signing
    /// entrypoint first, as `AuthHttp` does; raw resolved-key JSON is unsupported.
    /// # Errors
    /// Returns existing claim validation or sanitized signing errors.
    #[allow(clippy::unused_async)] // Matches the async signing API; capability reuse performs no I/O.
    pub async fn sign_jwt_with_resolved_key(
        &self,
        body: Option<&Value>,
        now: DateTime<Utc>,
        key: &ResolvedSigningKey,
    ) -> BlueprintResult<Value> {
        let payload = super::sign::resolved_payload(body, now, &self.base_url)?;
        Ok(json!({"token":key.0.sign_payload(&payload)?}))
    }
}

#[cfg(test)]
mod tests;
