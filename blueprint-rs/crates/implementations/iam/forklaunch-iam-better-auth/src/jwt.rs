//! Persistent asymmetric JWT signing keys and public JWK profiles.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use p521::{
    ecdsa::signature::RandomizedSigner,
    pkcs8::{DecodePrivateKey, EncodePrivateKey},
};
use ring::{
    rand::SystemRandom,
    signature::{
        ECDSA_P256_SHA256_FIXED_SIGNING, ECDSA_P384_SHA384_FIXED_SIGNING, EcdsaKeyPair,
        Ed25519KeyPair, KeyPair,
    },
};
use serde_json::{Value, json};
use zeroize::Zeroizing;

mod rsa_key;

/// An asymmetric signing key with a persistent identifier.
/// Deliberately has no Debug/Serialize implementation exposing private material.
pub struct SigningKey {
    id: String,
    private: Zeroizing<Vec<u8>>,
    pair: Material,
    algorithm: String,
}

enum Material {
    Ed25519(Ed25519KeyPair),
    Ecdsa(EcdsaKeyPair),
    P521(p521::ecdsa::SigningKey),
    Rsa(Box<rsa_key::RsaKey>),
}

impl SigningKey {
    /// Generates a fresh Ed25519 key. The caller assigns its persisted key ID.
    /// # Errors
    /// Returns a safe error on invalid IDs or secure randomness failure.
    pub fn generate(id: impl Into<String>) -> BlueprintResult<Self> {
        Self::generate_profile(id, "EdDSA")
    }

    pub(crate) fn generate_profile(
        id: impl Into<String>,
        algorithm: &str,
    ) -> BlueprintResult<Self> {
        Self::generate_config(id, algorithm, 2048)
    }
    pub(crate) fn generate_config(
        id: impl Into<String>,
        algorithm: &str,
        bits: usize,
    ) -> BlueprintResult<Self> {
        if matches!(
            algorithm,
            "RS256" | "RS384" | "RS512" | "PS256" | "PS384" | "PS512"
        ) {
            let encoded = rsa_key::RsaKey::generate(bits)?;
            return Self::from_pkcs8_profile(id, &encoded, algorithm);
        }
        if algorithm == "ES512" {
            let secret = p521::SecretKey::random(&mut rand_core::OsRng);
            let encoded = secret.to_pkcs8_der().map_err(|_| key_error())?;
            return Self::from_pkcs8_profile(id, encoded.as_bytes(), algorithm);
        }
        let random = SystemRandom::new();
        let encoded = match algorithm {
            "EdDSA" | "Ed25519" => Ed25519KeyPair::generate_pkcs8(&random),
            "ES256" => EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &random),
            "ES384" => EcdsaKeyPair::generate_pkcs8(&ECDSA_P384_SHA384_FIXED_SIGNING, &random),
            _ => return Err(key_error()),
        }
        .map_err(|_| key_error())?;
        Self::from_pkcs8_profile(id, encoded.as_ref(), algorithm)
    }

    /// Restores a previously stored Ed25519 signing key without changing its key ID.
    /// # Errors
    /// Rejects malformed private keys and missing/oversized key IDs.
    pub fn from_pkcs8(id: impl Into<String>, encoded: &[u8]) -> BlueprintResult<Self> {
        Self::from_pkcs8_profile(id, encoded, "EdDSA")
    }

    pub(crate) fn from_pkcs8_profile(
        id: impl Into<String>,
        encoded: &[u8],
        algorithm: &str,
    ) -> BlueprintResult<Self> {
        let id = id.into();
        if id.is_empty() || id.len() > 256 || encoded.len() > 4096 {
            return Err(key_error());
        }
        let random = SystemRandom::new();
        let pair = match algorithm {
            "EdDSA" | "Ed25519" => {
                Material::Ed25519(Ed25519KeyPair::from_pkcs8(encoded).map_err(|_| key_error())?)
            }
            "ES256" => Material::Ecdsa(
                EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, encoded, &random)
                    .map_err(|_| key_error())?,
            ),
            "ES384" => Material::Ecdsa(
                EcdsaKeyPair::from_pkcs8(&ECDSA_P384_SHA384_FIXED_SIGNING, encoded, &random)
                    .map_err(|_| key_error())?,
            ),
            "ES512" => {
                let secret = p521::SecretKey::from_pkcs8_der(encoded).map_err(|_| key_error())?;
                let bytes = Zeroizing::new(secret.to_bytes());
                Material::P521(
                    p521::ecdsa::SigningKey::from_bytes(&bytes).map_err(|_| key_error())?,
                )
            }
            "RS256" | "RS384" | "RS512" | "PS256" | "PS384" | "PS512" => {
                Material::Rsa(Box::new(rsa_key::RsaKey::restore(encoded)?))
            }
            _ => return Err(key_error()),
        };
        Ok(Self {
            id,
            private: Zeroizing::new(encoded.to_vec()),
            pair,
            algorithm: algorithm.to_string(),
        })
    }

    /// Private persistence representation. Encrypt it with tenant-bound storage
    /// before writing it to a database; never include it in HTTP/JWKS responses.
    #[must_use]
    pub fn private_pkcs8(&self) -> &[u8] {
        &self.private
    }

    /// Public-only key, matching the configured Better Auth `/jwks` response.
    #[must_use]
    pub fn public_jwk(&self) -> Value {
        match &self.pair {
            Material::Rsa(key) => key.public(&self.id, &self.algorithm),
            Material::Ed25519(pair) => json!({
                "alg":self.algorithm,"kty":"OKP","crv":"Ed25519",
                "x":URL_SAFE_NO_PAD.encode(pair.public_key().as_ref()),"kid":self.id
            }),
            Material::P521(pair) => {
                let point = p521::ecdsa::VerifyingKey::from(pair).to_encoded_point(false);
                let bytes = point.as_bytes();
                json!({"alg":self.algorithm,"kty":"EC","crv":"P-521",
                    "x":URL_SAFE_NO_PAD.encode(&bytes[1..67]),"y":URL_SAFE_NO_PAD.encode(&bytes[67..]),"kid":self.id})
            }
            Material::Ecdsa(pair) => {
                let bytes = pair.public_key().as_ref();
                let width = (bytes.len() - 1) / 2;
                json!({"alg":self.algorithm,"kty":"EC","crv":if width == 32 {"P-256"} else {"P-384"},
                    "x":URL_SAFE_NO_PAD.encode(&bytes[1..=width]),"y":URL_SAFE_NO_PAD.encode(&bytes[width+1..]),"kid":self.id})
            }
        }
    }

    /// Issues the actual configured payload with a 15-minute lifetime.
    /// Identity, organization and issuance time must come from an authenticated
    /// persisted session and the server clock, not caller-controlled JSON.
    /// # Errors
    /// Rejects timestamp overflow or JSON serialization failures.
    pub fn issue(
        &self,
        base_url: &str,
        user_id: &str,
        email: &str,
        active_organization_id: Option<&str>,
        issued_at: u64,
    ) -> BlueprintResult<String> {
        self.issue_session(
            base_url,
            user_id,
            email,
            active_organization_id,
            None,
            issued_at,
        )
    }
    /// Issue configured claims including the authenticated session identifier.
    /// An absent session ID intentionally grants no scoped application access.
    /// # Errors
    /// Rejects timestamp overflow or serialization failures.
    pub fn issue_session(
        &self,
        base_url: &str,
        user_id: &str,
        email: &str,
        active_organization_id: Option<&str>,
        session_id: Option<&str>,
        issued_at: u64,
    ) -> BlueprintResult<String> {
        let expires_at = issued_at.checked_add(900).ok_or_else(key_error)?;
        let mut payload = json!({
            "iat": issued_at, "exp": expires_at, "iss": base_url, "aud": base_url,
            "sub": user_id, "email": email, "activeOrganizationId": active_organization_id
        });
        if let Some(session_id) = session_id { payload["sessionId"] = Value::from(session_id); }
        self.sign_payload(&payload)
    }
    /// Signs an already validated server-owned claims object.
    /// # Errors
    /// Returns a sanitized error when the claims are not an object.
    pub(crate) fn sign_payload(&self, payload: &Value) -> BlueprintResult<String> {
        if !payload.is_object() {
            return Err(key_error());
        }
        let header = json!({"alg":self.algorithm,"kid":self.id});
        let content = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(crate::ecmascript::stringify(&header)),
            URL_SAFE_NO_PAD.encode(crate::ecmascript::stringify(payload))
        );
        let signature = match &self.pair {
            Material::Rsa(key) => key.sign(content.as_bytes(), &self.algorithm)?,
            Material::Ed25519(pair) => pair.sign(content.as_bytes()).as_ref().to_vec(),
            Material::Ecdsa(pair) => pair
                .sign(&SystemRandom::new(), content.as_bytes())
                .map_err(|_| key_error())?
                .as_ref()
                .to_vec(),
            Material::P521(pair) => {
                let signature: p521::ecdsa::Signature = pair
                    .try_sign_with_rng(&mut rand_core::OsRng, content.as_bytes())
                    .map_err(|_| key_error())?;
                signature.to_bytes().to_vec()
            }
        };
        Ok(format!("{content}.{}", URL_SAFE_NO_PAD.encode(&signature)))
    }
}

fn key_error() -> BlueprintError {
    BlueprintError::new(
        "better_auth_key_error",
        "Unable to use authentication signing key",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_keys_preserve_identity_and_signatures_and_reject_invalid_material() {
        let key = SigningKey::generate("persisted-id").unwrap();
        let restored = SigningKey::from_pkcs8("persisted-id", key.private_pkcs8()).unwrap();
        assert_eq!(key.public_jwk(), restored.public_jwk());
        assert_eq!(
            key.issue("https://service.test", "user", "mail@test", None, 100)
                .unwrap(),
            restored
                .issue("https://service.test", "user", "mail@test", None, 100)
                .unwrap()
        );
        assert!(SigningKey::from_pkcs8("persisted-id", b"malformed").is_err());
        assert!(SigningKey::generate("").is_err());
        assert!(
            key.issue("https://service.test", "user", "mail@test", None, u64::MAX)
                .is_err()
        );
        assert!(key.public_jwk().get("d").is_none());
    }
}
