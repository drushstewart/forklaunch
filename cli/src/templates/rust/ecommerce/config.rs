//! Trusted process configuration, read once at startup. The tenant and every
//! credential come from here, never from request input. Holds secrets, so it
//! deliberately implements neither `Debug` nor `Display`.
use crate::error::Result;
use forklaunch_axum::module_auth::HmacJwksAuth;
use forklaunch_core::auth::jwt::{RemoteJwks, RemoteJwksConfig};
use forklaunch_worker_database::{http::HttpConfig, runner::RunnerConfig};
use forklaunch_worker_interface::RetryPolicy;
use jsonwebtoken::{Algorithm, Validation};
use std::{
    env::{self, VarError},
    time::Duration,
};

/// The tenant and order-event queue policy shared by the HTTP service and the inventory worker.
#[derive(Clone)]
pub struct StoreConfig {
    pub tenant_id: String,
    pub retry: RetryPolicy,
}

impl StoreConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            tenant_id: env::var("TENANT_ID").map_err(|_| "TENANT_ID is required")?,
            retry: RetryPolicy::new(u32::try_from(number("WORKER_MAX_ATTEMPTS", 3)?)?, number("WORKER_BACKOFF_SECONDS", 30)?),
        })
    }
}

#[derive(Clone)]
pub struct ServiceConfig {
    pub store: StoreConfig,
    pub host: String,
    pub port: u16,
    pub hmac_secret: String,
    pub jwks_url: String,
    pub jwt_issuer: Option<String>,
    pub jwt_audience: Option<String>,
    pub stripe: StripeConfig,
    pub paypal: Option<PaypalConfig>,
}

#[derive(Clone)]
pub struct StripeConfig {
    pub api_key: String,
    pub api_base: String,
    pub webhook_secret: String,
    /// Direct Connect charges to this account, with an optional platform fee.
    pub connect: Option<(String, u16)>,
}

#[derive(Clone)]
pub struct PaypalConfig {
    pub client_id: String,
    pub client_secret: String,
    pub base_url: String,
    pub webhook_id: String,
}

impl ServiceConfig {
    pub fn from_env(store: StoreConfig) -> Result<Self> {
        let hmac_secret = env::var("HMAC_SECRET_KEY").map_err(|_| "HMAC_SECRET_KEY is required")?;
        let jwks_url = env::var("JWKS_PUBLIC_KEY_URL").map_err(|_| "JWKS_PUBLIC_KEY_URL is required")?;
        let stripe = StripeConfig {
            api_base: env::var("STRIPE_API_BASE").unwrap_or_else(|_| "https://api.stripe.com".to_owned()),
            api_key: env::var("STRIPE_API_KEY").map_err(|_| "STRIPE_API_KEY is required")?,
            webhook_secret: env::var("STRIPE_WEBHOOK_SECRET").map_err(|_| "STRIPE_WEBHOOK_SECRET is required")?,
            connect: match env::var("STRIPE_CONNECTED_ACCOUNT_ID") {
                Ok(account) => Some((account, u16::try_from(number("STRIPE_PLATFORM_FEE_BPS", 0)?)?)),
                Err(_) if env::var_os("STRIPE_PLATFORM_FEE_BPS").is_some() => {
                    return Err("STRIPE_PLATFORM_FEE_BPS requires STRIPE_CONNECTED_ACCOUNT_ID".into());
                }
                Err(_) => None,
            },
        };
        let paypal = match (env::var("PAYPAL_CLIENT_ID"), env::var("PAYPAL_CLIENT_SECRET")) {
            (Ok(client_id), Ok(client_secret)) => Some(PaypalConfig {
                client_id,
                client_secret,
                base_url: env::var("PAYPAL_BASE_URL").map_err(|_| "PAYPAL_BASE_URL is required with PayPal credentials")?,
                webhook_id: env::var("PAYPAL_WEBHOOK_ID").map_err(|_| "PAYPAL_WEBHOOK_ID is required with PayPal credentials")?,
            }),
            (Err(VarError::NotPresent), Err(VarError::NotPresent)) => None,
            _ => return Err("Both PAYPAL_CLIENT_ID and PAYPAL_CLIENT_SECRET are required to enable PayPal".into()),
        };
        Ok(Self {
            store,
            host: env::var("HOST").or_else(|_| env::var("SERVICE_HOST")).unwrap_or_else(|_| "127.0.0.1".to_owned()),
            port: env::var("SERVICE_PORT").or_else(|_| env::var("PORT")).unwrap_or_else(|_| "8000".to_owned()).parse()?,
            hmac_secret,
            jwks_url,
            jwt_issuer: env::var("JWT_ISSUER").ok(),
            jwt_audience: env::var("JWT_AUDIENCE").ok(),
            stripe,
            paypal,
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

/// The inventory worker's queue runner and optional readiness/control listener.
pub struct WorkerConfig {
    pub store: StoreConfig,
    pub runner: RunnerConfig,
    pub http: Option<HttpConfig>,
}

impl WorkerConfig {
    pub fn from_env(store: StoreConfig) -> Result<Self> {
        let runner = RunnerConfig {
            queue: "order-events".to_owned(),
            concurrency: usize::try_from(number("WORKER_CONCURRENCY", 1)?)?,
            batch_size: usize::try_from(number("WORKER_BATCH_SIZE", 1)?)?,
            poll_interval: Duration::from_millis(number("WORKER_POLL_MS", 250)?),
            lease_duration: Duration::from_millis(number("WORKER_LEASE_MS", 30000)?),
            renew_interval: Duration::from_millis(number("WORKER_RENEW_MS", 10000)?),
            drain_timeout: Duration::from_millis(number("WORKER_DRAIN_MS", 30000)?),
        };
        runner.validate()?;
        Ok(Self { store, runner, http: http_config()? })
    }
}

fn number(key: &str, default: u64) -> Result<u64> {
    match env::var(key) {
        Ok(value) => Ok(value.parse()?),
        Err(VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

fn http_config() -> Result<Option<HttpConfig>> {
    let token = match env::var("WORKER_CONTROL_TOKEN") {
        Ok(value) => Some(value),
        Err(VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    let address = match env::var("WORKER_HTTP_ADDR") {
        Ok(value) => value.parse()?,
        Err(VarError::NotPresent) => {
            if token.is_some() {
                return Err("WORKER_CONTROL_TOKEN requires WORKER_HTTP_ADDR".into());
            }
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let config = HttpConfig { address, control_token: token };
    config.validate()?;
    Ok(Some(config))
}
