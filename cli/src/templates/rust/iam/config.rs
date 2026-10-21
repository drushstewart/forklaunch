//! Trusted process configuration, read once at startup. The tenant and every
//! credential come from here, never from request input. Holds secrets, so it
//! deliberately implements neither `Debug` nor `Display`.
use crate::error::Result;
use forklaunch_axum::module_auth::HmacJwksAuth;
use forklaunch_core::auth::jwt::{RemoteJwks, RemoteJwksConfig};
use forklaunch_iam_base::http::DirectoryAuthorizationMode;
use jsonwebtoken::{Algorithm, Validation};
use std::env::{self, VarError};

#[derive(Clone)]
pub struct ServiceConfig {
    pub host: String,
    pub port: u16,
    pub tenant_id: String,
    pub directory_mode: DirectoryAuthorizationMode,
    /// Exact HTTPS origin for browser sessions; `None` leaves them disabled.
    pub browser_origin: Option<String>,
    pub oidc: Option<OidcConfig>,
    pub hmac_secret: String,
    pub jwks_url: String,
    pub jwt_issuer: Option<String>,
    pub jwt_audience: Option<String>,
}

/// The single operator-configured OIDC provider for browser login.
#[derive(Clone)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
}

impl ServiceConfig {
    pub fn from_env() -> Result<Self> {
        let tenant_id = env::var("TENANT_ID").map_err(|_| "TENANT_ID is required")?;
        let directory_mode = match env::var("IAM_DIRECTORY_AUTHORIZATION") {
            Err(VarError::NotPresent) => DirectoryAuthorizationMode::JwtClaims,
            Ok(value) if value == "jwt-claims" => DirectoryAuthorizationMode::JwtClaims,
            Ok(value) if value == "stored-session" => DirectoryAuthorizationMode::StoredSessions,
            _ => return Err("IAM_DIRECTORY_AUTHORIZATION must be jwt-claims or stored-session".into()),
        };
        let oidc_enabled = ["SESSION_OIDC_ISSUER", "SESSION_OIDC_CLIENT_ID", "SESSION_OIDC_CLIENT_SECRET"]
            .iter()
            .any(|name| env::var_os(name).is_some());
        if oidc_enabled && env::var_os("SESSION_COOKIE_ORIGIN").is_none() {
            return Err("OIDC requires SESSION_COOKIE_ORIGIN".into());
        }
        let browser_origin = match env::var("SESSION_COOKIE_ORIGIN") {
            Ok(origin) => Some(origin),
            Err(VarError::NotPresent) => None,
            Err(VarError::NotUnicode(_)) => return Err("Invalid browser session origin".into()),
        };
        let oidc = if oidc_enabled {
            Some(OidcConfig {
                issuer: env::var("SESSION_OIDC_ISSUER").map_err(|_| "SESSION_OIDC_ISSUER is required")?,
                client_id: env::var("SESSION_OIDC_CLIENT_ID").map_err(|_| "SESSION_OIDC_CLIENT_ID is required")?,
                client_secret: match env::var("SESSION_OIDC_CLIENT_SECRET") {
                    Ok(value) => Some(value),
                    Err(VarError::NotPresent) => None,
                    Err(_) => return Err("Invalid OIDC client secret".into()),
                },
            })
        } else {
            None
        };
        Ok(Self {
            host: env::var("HOST").or_else(|_| env::var("SERVICE_HOST")).unwrap_or_else(|_| "127.0.0.1".to_owned()),
            port: env::var("SERVICE_PORT").or_else(|_| env::var("PORT")).unwrap_or_else(|_| "8000".to_owned()).parse()?,
            tenant_id,
            directory_mode,
            browser_origin,
            oidc,
            hmac_secret: env::var("HMAC_SECRET_KEY").map_err(|_| "HMAC_SECRET_KEY is required")?,
            jwks_url: env::var("JWKS_PUBLIC_KEY_URL").map_err(|_| "JWKS_PUBLIC_KEY_URL is required")?,
            jwt_issuer: env::var("JWT_ISSUER").ok(),
            jwt_audience: env::var("JWT_AUDIENCE").ok(),
        })
    }

    #[must_use]
    pub fn bind_addr(&self) -> (&str, u16) {
        (&self.host, self.port)
    }

    /// Internal routes verify request HMAC signatures; protected routes verify
    /// asymmetric JWTs from the trusted JWKS endpoint.
    pub fn module_auth(&self) -> Result<HmacJwksAuth> {
        let jwks = RemoteJwks::new(RemoteJwksConfig::new(self.jwks_url.clone()))?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.algorithms = vec![
            Algorithm::RS256, Algorithm::RS384, Algorithm::RS512,
            Algorithm::PS256, Algorithm::PS384, Algorithm::PS512,
            Algorithm::ES256, Algorithm::ES384, Algorithm::EdDSA,
        ];
        validation.validate_nbf = true;
        validation.leeway = 0;
        validation.validate_aud = false;
        if let Some(issuer) = &self.jwt_issuer {
            validation.set_issuer(&[issuer]);
        }
        if let Some(audience) = &self.jwt_audience {
            validation.validate_aud = true;
            validation.set_audience(&[audience]);
        }
        Ok(HmacJwksAuth::new(self.hmac_secret.clone().into_bytes(), jwks, validation)?)
    }
}
