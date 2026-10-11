use forklaunch_auth::jwt::{RemoteJwks, RemoteJwksConfig};
use forklaunch_axum::module_auth::HmacJwksAuth;
use forklaunch_blueprint_core::AuditContext;
use forklaunch_blueprint_core::provider_http::ProviderHttpClient;
use forklaunch_core::persistence::{TenantContext, encryption::FieldEncryptor};
use forklaunch_core::telemetry::otlp::TelemetryRuntime;
use forklaunch_ecommerce_axum::{EcommerceBackend, ecommerce_router};
use forklaunch_ecommerce_base::{events::OrderInventoryHandler, postgres::PostgresEcommerceStore};
use forklaunch_ecommerce_paypal::PaypalClient;
use forklaunch_ecommerce_stripe::{StripeClient, StripeConnect};
use forklaunch_worker_database::{
    http::HttpConfig, postgres::PostgresWorkerQueue, runner::RunnerConfig,
};
use forklaunch_worker_interface::RetryPolicy;
use {{crate_module}}::{service_info, write_contract_artifacts};
use jsonwebtoken::{Algorithm, Validation};
use sqlx::postgres::PgPoolOptions;
use std::{env, sync::Arc, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if matches!(
        env::var("FORKLAUNCH_MODE").as_deref(),
        Ok("openapi" | "contract")
    ) {
        write_contract_artifacts(
            env::var("FORKLAUNCH_CONTRACT_OUTPUT")
                .unwrap_or_else(|_| ".forklaunch/contracts/contract.json".to_owned()),
            env::var("FORKLAUNCH_OPENAPI_OUTPUT")
                .unwrap_or_else(|_| ".forklaunch/openapi/openapi.json".to_owned()),
        )?;
        return Ok(());
    }
    let telemetry =
        TelemetryRuntime::start({{crate_module}}::SERVICE_NAME, env!("CARGO_PKG_VERSION")).await?;
    let result = run().await;
    let flushed = telemetry.shutdown().await;
    result?;
    flushed?;
    Ok(())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if !args.is_empty() && args != ["--migrate"] && args != ["--worker"] {
        return Err("Expected no arguments, --migrate or --worker".into());
    }
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?)
        .await
        .map_err(|_| "Unable to connect to PostgreSQL")?;
    if args == ["--migrate"] {
        PostgresEcommerceStore::migrate(&pool).await?;
        PostgresWorkerQueue::migrate(&pool).await?;
        pool.close().await;
        return Ok(());
    }
    let tenant = TenantContext::new(env::var("TENANT_ID").map_err(|_| "TENANT_ID is required")?)?;
    let store = PostgresEcommerceStore::new(
        pool.clone(),
        tenant.clone(),
        Arc::new(FieldEncryptor::from_env()?),
        AuditContext::system("ecommerce-startup"),
    )
    .await?;
    let queue = PostgresWorkerQueue::new(
        pool.clone(),
        tenant,
        RetryPolicy::new(
            u32::try_from(number("WORKER_MAX_ATTEMPTS", 3)?)?,
            number("WORKER_BACKOFF_SECONDS", 30)?,
        ),
    )
    .await?;
    if args == ["--worker"] {
        run_inventory_worker(&queue, store).await?;
        pool.close().await;
        return Ok(());
    }
    let secret = env::var("HMAC_SECRET_KEY").map_err(|_| "HMAC_SECRET_KEY is required")?;
    let jwks = RemoteJwks::new(RemoteJwksConfig::new(
        env::var("JWKS_PUBLIC_KEY_URL").map_err(|_| "JWKS_PUBLIC_KEY_URL is required")?,
    ))?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.algorithms = vec![
        Algorithm::RS256,
        Algorithm::RS384,
        Algorithm::RS512,
        Algorithm::PS256,
        Algorithm::PS384,
        Algorithm::PS512,
        Algorithm::ES256,
        Algorithm::ES384,
        Algorithm::EdDSA,
    ];
    validation.validate_nbf = true;
    validation.leeway = 0;
    validation.validate_aud = false;
    if let Ok(issuer) = env::var("JWT_ISSUER") {
        validation.set_issuer(&[issuer]);
    }
    if let Ok(audience) = env::var("JWT_AUDIENCE") {
        validation.validate_aud = true;
        validation.set_audience(&[audience]);
    }
    let auth = HmacJwksAuth::new(secret.into_bytes(), jwks, validation)?;
    let stripe_http = ProviderHttpClient::new(
        &env::var("STRIPE_API_BASE").unwrap_or_else(|_| "https://api.stripe.com".to_owned()),
        Duration::from_secs(15),
        1024 * 1024,
    )?;
    let stripe = StripeClient::with_transport(
        env::var("STRIPE_API_KEY").map_err(|_| "STRIPE_API_KEY is required")?,
        stripe_http,
    )?;
    let mut backend = EcommerceBackend::new(store, queue, stripe);
    if let Ok(account) = env::var("STRIPE_CONNECTED_ACCOUNT_ID") {
        backend = backend.with_connect(StripeConnect::new(
            account,
            u16::try_from(number("STRIPE_PLATFORM_FEE_BPS", 0)?)?,
        )?);
    } else if env::var_os("STRIPE_PLATFORM_FEE_BPS").is_some() {
        return Err("STRIPE_PLATFORM_FEE_BPS requires STRIPE_CONNECTED_ACCOUNT_ID".into());
    }
    match (
        env::var("PAYPAL_CLIENT_ID"),
        env::var("PAYPAL_CLIENT_SECRET"),
    ) {
        (Ok(id), Ok(secret)) => {
            backend = backend.with_paypal(PaypalClient::new(
                id,
                secret,
                &env::var("PAYPAL_BASE_URL")
                    .map_err(|_| "PAYPAL_BASE_URL is required with PayPal credentials")?,
            )?)
        }
        (Err(env::VarError::NotPresent), Err(env::VarError::NotPresent)) => {}
        _ => {
            return Err(
                "Both PAYPAL_CLIENT_ID and PAYPAL_CLIENT_SECRET are required to enable PayPal"
                    .into(),
            );
        }
    }
    let host = env::var("HOST")
        .or_else(|_| env::var("SERVICE_HOST"))
        .unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port: u16 = env::var("SERVICE_PORT")
        .or_else(|_| env::var("PORT"))
        .unwrap_or_else(|_| "8000".to_owned())
        .parse()?;
    let listener = tokio::net::TcpListener::bind((host.as_str(), port)).await?;
    println!(
        "{} listening on http://{}",
        {{crate_module}}::SERVICE_NAME,
        listener.local_addr()?
    );
    axum::serve(
        listener,
        {{crate_module}}::routes::register_router(ecommerce_router(backend, auth, service_info()))
            .into_router_with_default_mcp()?,
    )
    .with_graceful_shutdown(shutdown())
    .await?;
    pool.close().await;
    Ok(())
}

async fn run_inventory_worker(
    queue: &PostgresWorkerQueue,
    store: PostgresEcommerceStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = RunnerConfig {
        queue: "order-events".to_owned(),
        concurrency: usize::try_from(number("WORKER_CONCURRENCY", 1)?)?,
        batch_size: usize::try_from(number("WORKER_BATCH_SIZE", 1)?)?,
        poll_interval: Duration::from_millis(number("WORKER_POLL_MS", 250)?),
        lease_duration: Duration::from_millis(number("WORKER_LEASE_MS", 30000)?),
        renew_interval: Duration::from_millis(number("WORKER_RENEW_MS", 10000)?),
        drain_timeout: Duration::from_millis(number("WORKER_DRAIN_MS", 30000)?),
    };
    config.validate()?;
    let http = http_config()?;
    println!("{} inventory worker ready", {{crate_module}}::SERVICE_NAME);
    let report = forklaunch_worker_database::http::run(
        queue,
        &OrderInventoryHandler::new(Arc::new(store), "order-events"),
        &config,
        None,
        http.as_ref(),
        shutdown(),
    )
    .await?;
    println!(
        "inventory worker stopped: completed={} retried={} dead_lettered={}",
        report.worker.completed, report.worker.retried, report.worker.dead_lettered
    );
    Ok(())
}
fn number(key: &str, default: u64) -> Result<u64, Box<dyn std::error::Error>> {
    match env::var(key) {
        Ok(value) => Ok(value.parse()?),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}
fn http_config() -> Result<Option<HttpConfig>, Box<dyn std::error::Error>> {
    let token = match env::var("WORKER_CONTROL_TOKEN") {
        Ok(value) => Some(value),
        Err(env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    let address = match env::var("WORKER_HTTP_ADDR") {
        Ok(value) => value.parse()?,
        Err(env::VarError::NotPresent) => {
            if token.is_some() {
                return Err("WORKER_CONTROL_TOKEN requires WORKER_HTTP_ADDR".into());
            }
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let config = HttpConfig {
        address,
        control_token: token,
    };
    config.validate()?;
    Ok(Some(config))
}

async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = signal.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
