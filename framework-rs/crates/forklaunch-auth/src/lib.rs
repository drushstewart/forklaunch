#![forbid(unsafe_code)]

//! Authentication and authorization primitives for the Rust framework.
//!
//! Covers the credential mechanisms `ForkLaunch` services rely on — HMAC, JWT
//! (HS256 signature key), and Basic — together with the RBAC model
//! (roles, permissions, scopes with hierarchy, feature entitlements, and
//! subscription gating). All failures surface as [`AuthError`], which converts
//! into the framework's standard [`ErrorResponse`].

use base64::{Engine as _, engine::general_purpose::STANDARD};
use forklaunch_contract::RbacConfig;
use forklaunch_http::{ErrorKind, ErrorResponse};
use hmac::{Hmac, Mac};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;

pub mod jwt;

/// TypeScript-compatible service-to-service request signatures.
pub mod request_hmac;

type HmacSha256 = Hmac<Sha256>;

/// An authenticated caller, enriched from a verified credential.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    /// Subject identifier (e.g. user or service id).
    pub subject: String,
    /// Granted roles.
    pub roles: Vec<String>,
    /// Granted fine-grained permissions.
    pub permissions: Vec<String>,
    /// Granted scopes.
    pub scopes: Vec<String>,
    /// Enabled feature entitlements.
    pub features: Vec<String>,
    /// Whether the caller has an active subscription.
    pub active_subscription: bool,
}

impl Principal {
    /// Creates a principal for the given subject with no grants.
    #[must_use]
    pub fn new(subject: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            ..Self::default()
        }
    }

    /// Enriches an authenticated principal from verified claims.
    #[must_use]
    pub fn from_claims(claims: JwtClaims) -> Self {
        Self {
            subject: claims.sub,
            roles: claims.roles,
            permissions: claims.permissions,
            scopes: claims.scopes,
            features: claims.features,
            active_subscription: claims.active_subscription,
        }
    }
}

/// Reasons authentication or authorization can fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// No credential was presented.
    MissingCredentials,
    /// A credential was presented but could not be verified.
    InvalidCredentials,
    /// The caller authenticated but is not permitted to access the resource.
    Forbidden(String),
}

impl AuthError {
    /// Converts the failure into a safe framework error response.
    #[must_use]
    pub fn to_error_response(&self) -> ErrorResponse {
        match self {
            Self::MissingCredentials => {
                ErrorResponse::new(ErrorKind::Unauthorized, "missing credentials")
            }
            Self::InvalidCredentials => {
                ErrorResponse::new(ErrorKind::Unauthorized, "invalid credentials")
            }
            Self::Forbidden(reason) => ErrorResponse::new(ErrorKind::Forbidden, reason.clone()),
        }
    }
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCredentials => write!(formatter, "missing credentials"),
            Self::InvalidCredentials => write!(formatter, "invalid credentials"),
            Self::Forbidden(reason) => write!(formatter, "forbidden: {reason}"),
        }
    }
}

impl std::error::Error for AuthError {}

/// JWT claims understood by the framework, including RBAC grants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    /// Subject identifier.
    pub sub: String,
    /// Expiry as a UNIX timestamp (seconds).
    pub exp: usize,
    /// Granted roles.
    #[serde(default)]
    pub roles: Vec<String>,
    /// Granted permissions.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Granted scopes.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Enabled feature entitlements.
    #[serde(default)]
    pub features: Vec<String>,
    /// Whether the caller has an active subscription.
    #[serde(default)]
    pub active_subscription: bool,
}

/// Extracts a bearer token from an `Authorization` header value.
#[must_use]
pub fn parse_bearer(header: &str) -> Option<&str> {
    header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

/// Parses a `Basic` `Authorization` header value into `(username, password)`.
#[must_use]
pub fn parse_basic(header: &str) -> Option<(String, String)> {
    let encoded = header
        .strip_prefix("Basic ")
        .or_else(|| header.strip_prefix("basic "))?;
    let decoded = STANDARD.decode(encoded.trim()).ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (username, password) = text.split_once(':')?;
    Some((username.to_owned(), password.to_owned()))
}

/// Verifies a HS256 JWT against a shared signature key and returns a principal.
///
/// # Errors
///
/// Returns [`AuthError::InvalidCredentials`] when the token is malformed,
/// expired, or signed with a different key.
pub fn verify_jwt_hs256(token: &str, signature_key: &[u8]) -> Result<Principal, AuthError> {
    jwt::check_header_policy(token)?;
    let validation = Validation::new(Algorithm::HS256);
    decode::<JwtClaims>(token, &DecodingKey::from_secret(signature_key), &validation)
        .map(|data| Principal::from_claims(data.claims))
        .map_err(|_| AuthError::InvalidCredentials)
}

/// Issues an HS256 JWT from server-owned claims and a nonempty signing key.
///
/// The caller supplies the subject, expiry (UNIX seconds) and grants explicitly.
/// No claims are copied from a request or added implicitly. Use a securely
/// generated secret; key distribution and rotation belong to the application.
/// This does not replace the Better Auth adapter's provider-specific JWT format.
///
/// # Errors
/// Returns [`AuthError::InvalidCredentials`] for an empty key/subject or when
/// serialization/signing fails. The caller owns expiry policy; an already
/// expired token can be issued but will fail verification.
pub fn issue_jwt_hs256(claims: &JwtClaims, signature_key: &[u8]) -> Result<String, AuthError> {
    if signature_key.is_empty() || claims.sub.is_empty() {
        return Err(AuthError::InvalidCredentials);
    }
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(Algorithm::HS256),
        claims,
        &jsonwebtoken::EncodingKey::from_secret(signature_key),
    )
    .map_err(|_| AuthError::InvalidCredentials)
}

/// Computes the base64-encoded HMAC-SHA256 signature for `payload`.
/// # Panics
///
/// Panics if the HMAC implementation rejects the key; SHA-256 accepts any key size.
#[must_use]
pub fn hmac_sign(secret: &[u8], payload: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts keys of any size");
    mac.update(payload);
    STANDARD.encode(mac.finalize().into_bytes())
}

/// Verifies a base64-encoded HMAC-SHA256 signature in constant time.
/// # Panics
///
/// Panics if the HMAC implementation rejects the key; SHA-256 accepts any key size.
#[must_use]
pub fn hmac_verify(secret: &[u8], payload: &[u8], signature: &str) -> bool {
    let Ok(provided) = STANDARD.decode(signature) else {
        return false;
    };

    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts keys of any size");
    mac.update(payload);
    let expected = mac.finalize().into_bytes();

    expected.len() == provided.len() && bool::from(expected.as_slice().ct_eq(provided.as_slice()))
}

/// Enforces an [`RbacConfig`] against a principal.
///
/// # Errors
///
/// Returns [`AuthError::Forbidden`] describing the first failed rule.
pub fn authorize(principal: &Principal, rbac: &RbacConfig) -> Result<(), AuthError> {
    if let Some(permission) = rbac
        .forbidden_permissions
        .iter()
        .find(|permission| principal.permissions.contains(permission))
    {
        return Err(AuthError::Forbidden(format!(
            "permission `{permission}` is forbidden"
        )));
    }

    if !rbac.allowed_permissions.is_empty()
        && !rbac
            .allowed_permissions
            .iter()
            .any(|permission| principal.permissions.contains(permission))
    {
        return Err(AuthError::Forbidden(
            "missing a required permission".to_owned(),
        ));
    }

    if let Some(role) = rbac
        .forbidden_roles
        .iter()
        .find(|role| principal.roles.contains(role))
    {
        return Err(AuthError::Forbidden(format!("role `{role}` is forbidden")));
    }

    if !rbac.allowed_roles.is_empty()
        && !rbac
            .allowed_roles
            .iter()
            .any(|role| principal.roles.contains(role))
    {
        return Err(AuthError::Forbidden("missing a required role".to_owned()));
    }

    if let Some(required_scope) = &rbac.required_scope
        && !scope_satisfied(principal, required_scope, &rbac.scope_hierarchy)
    {
        return Err(AuthError::Forbidden(format!(
            "missing required scope `{required_scope}`"
        )));
    }

    if let Some(feature) = rbac
        .required_features
        .iter()
        .find(|feature| !principal.features.contains(feature))
    {
        return Err(AuthError::Forbidden(format!(
            "missing required feature `{feature}`"
        )));
    }

    if rbac.require_active_subscription && !principal.active_subscription {
        return Err(AuthError::Forbidden(
            "an active subscription is required".to_owned(),
        ));
    }

    Ok(())
}

/// Returns whether a principal satisfies `required_scope`, honoring a hierarchy
/// ordered from most to least privileged.
#[must_use]
pub fn scope_satisfied(principal: &Principal, required_scope: &str, hierarchy: &[String]) -> bool {
    if principal.scopes.iter().any(|scope| scope == required_scope) {
        return true;
    }

    let Some(required_rank) = hierarchy.iter().position(|scope| scope == required_scope) else {
        return false;
    };

    principal.scopes.iter().any(|scope| {
        hierarchy
            .iter()
            .position(|candidate| candidate == scope)
            .is_some_and(|rank| rank <= required_rank)
    })
}

#[cfg(test)]
mod tests {
    use jsonwebtoken::{EncodingKey, Header, encode};

    use super::*;

    fn future_exp() -> usize {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_secs();
        usize::try_from(now).unwrap_or(0) + 3600
    }

    #[test]
    fn typed_jwt_issuance_preserves_grants_and_rejects_invalid_inputs() {
        let mut claims = JwtClaims {
            sub: "issuer-test".into(),
            exp: 4_102_444_800,
            roles: vec!["admin".into()],
            permissions: vec!["users:read".into()],
            scopes: vec!["profile".into()],
            features: vec!["reports".into()],
            active_subscription: true,
        };
        let token = issue_jwt_hs256(&claims, b"test-only-signing-key").unwrap();
        let principal = verify_jwt_hs256(&token, b"test-only-signing-key").unwrap();
        assert_eq!(principal, Principal::from_claims(claims.clone()));
        assert!(verify_jwt_hs256(&token, b"different-key").is_err());
        claims.exp = 1;
        let expired = issue_jwt_hs256(&claims, b"test-only-signing-key").unwrap();
        assert!(verify_jwt_hs256(&expired, b"test-only-signing-key").is_err());
        assert!(issue_jwt_hs256(&claims, b"").is_err());
        claims.sub.clear();
        assert!(issue_jwt_hs256(&claims, b"test-only-signing-key").is_err());
    }

    #[test]
    fn hmac_round_trips_and_rejects_tampering() {
        let secret = b"super-secret";
        let signature = hmac_sign(secret, b"payload");

        assert!(hmac_verify(secret, b"payload", &signature));
        assert!(!hmac_verify(secret, b"tampered", &signature));
        assert!(!hmac_verify(b"wrong-secret", b"payload", &signature));
        assert!(!hmac_verify(secret, b"payload", "not-base64!!"));
    }

    #[test]
    fn jwt_verifies_valid_tokens_and_rejects_wrong_keys() {
        let secret = b"jwt-signing-key";
        let claims = JwtClaims {
            sub: "user-1".to_owned(),
            exp: future_exp(),
            roles: vec!["admin".to_owned()],
            permissions: vec!["users:read".to_owned()],
            scopes: vec!["org".to_owned()],
            features: vec!["beta".to_owned()],
            active_subscription: true,
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret),
        )
        .expect("token should encode");

        let principal = verify_jwt_hs256(&token, secret).expect("token should verify");
        assert_eq!(principal.subject, "user-1");
        assert_eq!(principal.roles, vec!["admin".to_owned()]);
        assert!(principal.active_subscription);

        assert_eq!(
            verify_jwt_hs256(&token, b"other-key"),
            Err(AuthError::InvalidCredentials)
        );
    }

    #[test]
    fn parses_authorization_headers() {
        assert_eq!(parse_bearer("Bearer abc.def"), Some("abc.def"));
        assert_eq!(parse_bearer("bearer  token "), Some("token"));
        assert_eq!(parse_bearer("Basic xyz"), None);

        // base64 of "user:pass"
        assert_eq!(
            parse_basic("Basic dXNlcjpwYXNz"),
            Some(("user".to_owned(), "pass".to_owned()))
        );
    }

    #[test]
    fn rbac_enforces_permissions_roles_and_subscription() {
        let principal = Principal {
            subject: "user-1".to_owned(),
            roles: vec!["member".to_owned()],
            permissions: vec!["users:read".to_owned()],
            scopes: vec!["org".to_owned()],
            features: vec!["beta".to_owned()],
            active_subscription: true,
        };

        let allow = RbacConfig {
            allowed_permissions: vec!["users:read".to_owned()],
            allowed_roles: vec!["member".to_owned()],
            required_features: vec!["beta".to_owned()],
            require_active_subscription: true,
            ..RbacConfig::default()
        };
        assert!(authorize(&principal, &allow).is_ok());

        let forbidden_permission = RbacConfig {
            forbidden_permissions: vec!["users:read".to_owned()],
            ..RbacConfig::default()
        };
        assert!(authorize(&principal, &forbidden_permission).is_err());

        let missing_feature = RbacConfig {
            required_features: vec!["enterprise".to_owned()],
            ..RbacConfig::default()
        };
        assert!(authorize(&principal, &missing_feature).is_err());

        let needs_subscription = RbacConfig {
            require_active_subscription: true,
            ..RbacConfig::default()
        };
        let mut unsubscribed = principal.clone();
        unsubscribed.active_subscription = false;
        assert!(authorize(&unsubscribed, &needs_subscription).is_err());
    }

    #[test]
    fn scope_hierarchy_allows_higher_privilege_scopes() {
        let hierarchy = vec!["admin".to_owned(), "org".to_owned(), "team".to_owned()];

        let admin = Principal {
            scopes: vec!["admin".to_owned()],
            ..Principal::new("admin-user")
        };
        let team = Principal {
            scopes: vec!["team".to_owned()],
            ..Principal::new("team-user")
        };

        assert!(scope_satisfied(&admin, "team", &hierarchy));
        assert!(!scope_satisfied(&team, "org", &hierarchy));
        assert!(scope_satisfied(&team, "team", &hierarchy));
    }
}
