//! Explicit RSA-signed session tokens. Stored token digests remain authoritative
//! for IAM session validation, revocation, expiry and disabled-user checks.
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use forklaunch_iam_interface::User;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::Serialize;

/// Server-owned signing configuration; deliberately does not implement Debug.
/// Publish the matching public key through the application's trusted JWKS service.
/// The private key is never exported by this type. Existing stored sessions are
/// not implicitly revoked when the configuration changes.
pub struct RsaJwtSessionIssuer {
    key: EncodingKey,
    key_id: String,
    issuer: String,
    audience: String,
}

#[derive(Serialize)]
struct Claims<'a> {
    sub: &'a str,
    tenant_id: &'a str,
    roles: &'a [String],
    permissions: &'a [String],
    iss: &'a str,
    aud: &'a str,
    iat: u64,
    exp: u64,
    jti: String,
}

fn invalid() -> BlueprintError {
    BlueprintError::new(
        "iam_jwt_configuration_invalid",
        "Invalid IAM JWT signing configuration",
    )
}

impl RsaJwtSessionIssuer {
    /// Configures RS256 with explicit key ID, issuer and audience. A local signing
    /// probe validates the RSA key before accepting the configuration.
    ///
    /// # Errors
    /// Rejects malformed/unsupported keys, oversized/empty metadata and control
    /// characters. All errors exclude private key material and parser details.
    pub fn new(
        private_key_pem: &[u8],
        key_id: impl Into<String>,
        issuer: impl Into<String>,
        audience: impl Into<String>,
    ) -> BlueprintResult<Self> {
        let key_id = key_id.into();
        let issuer = issuer.into();
        let audience = audience.into();
        if private_key_pem.len() > 64 * 1024
            || [&key_id, &issuer, &audience].iter().any(|value| {
                value.trim().is_empty() || value.len() > 1024 || value.chars().any(char::is_control)
            })
        {
            return Err(invalid());
        }
        let value = Self {
            key: EncodingKey::from_rsa_pem(private_key_pem).map_err(|_| invalid())?,
            key_id,
            issuer,
            audience,
        };
        value.issue(
            &User::new("configuration-probe", "unused"),
            "configuration-probe",
            &[],
            0,
            1,
        )?;
        Ok(value)
    }

    /// Reads an optional, complete signing configuration from process variables.
    /// No variables selects opaque sessions. Partial configuration is an error.
    ///
    /// # Errors
    /// Rejects non-Unicode/partial settings or invalid signing configuration.
    pub fn from_env() -> BlueprintResult<Option<Self>> {
        let names = [
            "SESSION_JWT_PRIVATE_KEY_PEM",
            "SESSION_JWT_KEY_ID",
            "SESSION_JWT_ISSUER",
            "SESSION_JWT_AUDIENCE",
        ];
        let values = names.map(|name| match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(invalid()),
        });
        let [key, key_id, issuer, audience] = values;
        match (key?, key_id?, issuer?, audience?) {
            (None, None, None, None) => Ok(None),
            (Some(key), Some(key_id), Some(issuer), Some(audience)) => {
                Self::new(key.as_bytes(), key_id, issuer, audience).map(Some)
            }
            _ => Err(invalid()),
        }
    }

    pub(crate) fn issue(
        &self,
        user: &User,
        tenant_id: &str,
        permissions: &[String],
        issued_at: u64,
        expires_at: u64,
    ) -> BlueprintResult<String> {
        if user.id.is_empty() || tenant_id.is_empty() || expires_at <= issued_at || user.disabled {
            return Err(invalid());
        }
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(self.key_id.clone());
        let claims = Claims {
            sub: &user.id,
            tenant_id,
            roles: &user.roles,
            permissions,
            iss: &self.issuer,
            aud: &self.audience,
            iat: issued_at,
            exp: expires_at,
            jti: uuid::Uuid::new_v4().to_string(),
        };
        let token = jsonwebtoken::encode(&header, &claims, &self.key).map_err(|_| invalid())?;
        if token.len() > 4096 {
            return Err(invalid());
        }
        Ok(token)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use jsonwebtoken::{DecodingKey, Validation};
    use serde_json::Value;
    // Copied test-only RSA key, never used by production authentication.
    const PRIVATE_KEY: &[u8] = include_bytes!("jwt_sessions/test-key.pem");
    const MODULUS: &str = "kPszNF2ghKj80PvXDNA8pzL2TNEoJKp5g5eIbpPmjHJvyaItOcQOhKcvg5SkW7WVePRqGKIslE-NOg4qMS35oos1g5V9rOnsEunTRciygsCQkaL5WVBNerFLpuUBGypUOYd2W6TpKIsdENXBYO-SDckAYxEZUEdjZshi_AsgWQbfki7axKB67DgTonA0utX3JpG4h9PDVgj6erihubsUdM40Wuslh_j92H1Y3uGQBcvAnwlgvfd_t6WULNeeLAtng-ewemnqEbGSmuZiLPNU2TO9fnwa0C0Cm802BRyYdqDlw3JmLatuqLiO4Ft6PxQDSwlZa1uqy8Y0E3QHhGLp3Q";

    pub(crate) fn issuer() -> RsaJwtSessionIssuer {
        RsaJwtSessionIssuer::new(
            PRIVATE_KEY,
            "fixture-key",
            "https://issuer.fixture",
            "fixture-api",
        )
        .unwrap()
    }

    pub(crate) fn verify(token: &str) -> Value {
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&["https://issuer.fixture"]);
        validation.set_audience(&["fixture-api"]);
        validation.leeway = 0;
        jsonwebtoken::decode::<Value>(
            token,
            &DecodingKey::from_rsa_components(MODULUS, "AQAB").unwrap(),
            &validation,
        )
        .unwrap()
        .claims
    }

    #[test]
    fn signed_sessions_bind_claims_and_have_unique_ids() {
        let issuer = issuer();
        let user = User::new("user-1", "private@example.test").with_role("member");
        let now = u64::try_from(chrono::Utc::now().timestamp()).unwrap();
        let permissions = vec!["users:read".to_owned()];
        let first = issuer
            .issue(&user, "tenant-a", &permissions, now, now + 3600)
            .unwrap();
        let second = issuer
            .issue(&user, "tenant-a", &permissions, now, now + 3600)
            .unwrap();
        assert_ne!(first, second);
        assert!(first.len() <= 4096);
        let header = jsonwebtoken::decode_header(&first).unwrap();
        assert_eq!(header.alg, Algorithm::RS256);
        assert_eq!(header.kid.as_deref(), Some("fixture-key"));
        let claims = verify(&first);
        assert_eq!(claims["sub"], "user-1");
        assert_eq!(claims["tenant_id"], "tenant-a");
        assert_eq!(claims["roles"], serde_json::json!(["member"]));
        assert_eq!(claims["permissions"], serde_json::json!(["users:read"]));
        assert_eq!(claims["iat"], now);
        assert_eq!(claims["exp"], now + 3600);
        assert!(uuid::Uuid::parse_str(claims["jti"].as_str().unwrap()).is_ok());
        assert!(!claims.to_string().contains("private@example.test"));
    }

    #[test]
    fn invalid_keys_metadata_disabled_users_and_oversized_claims_fail_safely() {
        assert!(RsaJwtSessionIssuer::new(PRIVATE_KEY, "", "issuer", "audience").is_err());
        assert!(RsaJwtSessionIssuer::new(PRIVATE_KEY, "key", "issuer\n", "audience").is_err());
        let error = RsaJwtSessionIssuer::new(b"PRIVATE-SENTINEL", "key", "issuer", "audience")
            .err()
            .unwrap();
        assert!(!error.to_string().contains("PRIVATE-SENTINEL"));
        let issuer = issuer();
        let mut user = User::new("user", "unused");
        assert!(issuer.issue(&user, "tenant", &[], 1, 1).is_err());
        user.disabled = true;
        assert!(issuer.issue(&user, "tenant", &[], 1, 2).is_err());
        user.disabled = false;
        user.roles.push("x".repeat(6000));
        assert!(issuer.issue(&user, "tenant", &[], 1, 2).is_err());
    }
}
