//! Trusted process configuration, read once at startup. The tenant and every
//! credential come from here, never from request input. Holds secrets, so it
//! deliberately implements neither `Debug` nor `Display`.
use crate::error::Result;
use forklaunch_axum::module_auth::HmacJwksAuth;
use forklaunch_billing_stripe::http::{BillingRequestDateProfile, profiles};
use forklaunch_core::auth::jwt::{RemoteJwks, RemoteJwksConfig};
use jsonwebtoken::{Algorithm, Validation};
use std::env;

#[derive(Clone)]
pub struct ServiceConfig {
    pub host: String,
    pub port: u16,
    pub tenant_id: String,
    pub hmac_secret: String,
    pub jwks_url: String,
    pub jwt_issuer: Option<String>,
    pub jwt_audience: Option<String>,
{{#stripe}}    pub stripe: StripeConfig,
{{/stripe}}    pub database_backup: bool,
    pub request_dates: BillingRequestDateProfile,
}
{{#stripe}}
/// Your own Stripe key takes precedence; the managed gateway is the fallback
/// when no key is configured.
#[derive(Clone)]
pub struct StripeConfig {
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    pub webhook_secret: Option<String>,
    pub gateway_url: Option<String>,
    pub instance_id: Option<String>,
    pub instance_key: Option<String>,
}

fn non_empty(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}
{{/stripe}}
impl ServiceConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            host: env::var("HOST").or_else(|_| env::var("SERVICE_HOST")).unwrap_or_else(|_| "127.0.0.1".to_owned()),
            port: env::var("SERVICE_PORT").or_else(|_| env::var("PORT")).unwrap_or_else(|_| "8000".to_owned()).parse()?,
            tenant_id: env::var("TENANT_ID").map_err(|_| "TENANT_ID is required")?,
            hmac_secret: env::var("HMAC_SECRET_KEY").map_err(|_| "HMAC_SECRET_KEY is required")?,
            jwks_url: env::var("JWKS_PUBLIC_KEY_URL").map_err(|_| "JWKS_PUBLIC_KEY_URL is required")?,
            jwt_issuer: env::var("JWT_ISSUER").ok(),
            jwt_audience: env::var("JWT_AUDIENCE").ok(),
{{#stripe}}            stripe: StripeConfig {
                api_key: non_empty("STRIPE_API_KEY").or_else(|| non_empty("STRIPE_SECRET_KEY")),
                api_base: env::var("STRIPE_API_BASE").ok(),
                webhook_secret: non_empty("STRIPE_WEBHOOK_SECRET"),
                gateway_url: non_empty("PLATFORM_GATEWAY_URL"),
                instance_id: non_empty("INSTANCE_ID"),
                instance_key: non_empty("INSTANCE_HMAC_KEY"),
            },
{{/stripe}}            database_backup: env::var("BILLING_DATABASE_BACKUP").unwrap_or_else(|_| "false".into()).parse()?,
            request_dates: profiles::request_date_profile_from_env()?,
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
