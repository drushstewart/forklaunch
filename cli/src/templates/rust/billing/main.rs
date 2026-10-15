use std::{env, sync::Arc};
use forklaunch_auth::jwt::{RemoteJwks, RemoteJwksConfig};
use forklaunch_axum::module_auth::HmacJwksAuth;
use forklaunch_blueprint_core::AuditContext;
use forklaunch_core::persistence::{TenantContext,encryption::FieldEncryptor};
use forklaunch_billing_base::services::postgres::PostgresBillingStore;
use forklaunch_billing_axum::{BillingBackend,BoundCheckoutV1,BoundBillingV1,billing_router};
{{#stripe}}use forklaunch_billing_stripe::client::StripeBillingClient;
use forklaunch_blueprint_core::provider_http::ProviderHttpClient;
use std::time::Duration;{{/stripe}}
use sqlx::postgres::PgPoolOptions;
use forklaunch_core::telemetry::otlp::TelemetryRuntime;
use jsonwebtoken::{Algorithm, Validation};
use {{crate_module}}::{service_info, write_contract_artifacts, request_date_profile_from_env, bound_checkout_config_from_env, bound_billing_config_from_env};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if matches!(env::var("FORKLAUNCH_MODE").as_deref(), Ok("openapi" | "contract")) {
        write_contract_artifacts(
            env::var("FORKLAUNCH_CONTRACT_OUTPUT").unwrap_or_else(|_| ".forklaunch/contracts/contract.json".to_owned()),
            env::var("FORKLAUNCH_OPENAPI_OUTPUT").unwrap_or_else(|_| ".forklaunch/openapi/openapi.json".to_owned()),
        )?;
        return Ok(());
    }
    let telemetry = TelemetryRuntime::start({{crate_module}}::SERVICE_NAME, env!("CARGO_PKG_VERSION")).await?;
    let result = run().await;
    let flushed = telemetry.shutdown().await;
    result?;
    flushed?;
    Ok(())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if !args.is_empty() && args != ["--migrate"] { return Err("Expected no arguments or --migrate".into()); }
    let pool = PgPoolOptions::new().max_connections(10).connect(
        &env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?,
    ).await.map_err(|_| "Unable to connect to PostgreSQL")?;
    if args == ["--migrate"] {
        PostgresBillingStore::migrate(&pool).await?;
        pool.close().await;
        return Ok(());
    }
    let bound_checkout_config = bound_checkout_config_from_env()?;
    let bound_billing_config = bound_billing_config_from_env()?;
    let store = PostgresBillingStore::new(pool.clone(),
        TenantContext::new(env::var("TENANT_ID").map_err(|_| "TENANT_ID is required")?)?,
        Arc::new(FieldEncryptor::from_env()?),AuditContext::system("billing-startup"),
    ).await?;
    let secret = env::var("HMAC_SECRET_KEY").map_err(|_| "HMAC_SECRET_KEY is required")?;
    let jwks = RemoteJwks::new(RemoteJwksConfig::new(
        env::var("JWKS_PUBLIC_KEY_URL").map_err(|_| "JWKS_PUBLIC_KEY_URL is required")?,
    ))?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.algorithms = vec![Algorithm::RS256, Algorithm::RS384, Algorithm::RS512,
        Algorithm::PS256, Algorithm::PS384, Algorithm::PS512, Algorithm::ES256, Algorithm::ES384, Algorithm::EdDSA];
    validation.validate_nbf = true;
    validation.leeway = 0;
    validation.validate_aud = false;
    if let Ok(issuer) = env::var("JWT_ISSUER") { validation.set_issuer(&[issuer]); }
    if let Ok(audience) = env::var("JWT_AUDIENCE") { validation.validate_aud = true; validation.set_audience(&[audience]); }
    let auth = HmacJwksAuth::new(secret.into_bytes(), jwks, validation)?;
    let publisher_pool = if bound_checkout_config.is_some() || bound_billing_config.as_ref().is_some_and(|c|c.requires_publisher()) {
        Some(PgPoolOptions::new().max_connections(4).connect(&env::var("BILLING_PUBLISHER_DATABASE_URL").map_err(|_| "Selected bound creation requires a publisher database")?).await.map_err(|_| "Unable to connect to billing publisher database")?)
    } else { None };
    let checkout_selected=bound_checkout_config.is_some() || bound_billing_config.as_ref().is_some_and(|c|c.has_checkout());
    let portal_selected=bound_billing_config.as_ref().is_some_and(|c|c.has_portal());
    let startup_store=store.with_audit_context(AuditContext::actor("service:bound-billing-startup", "billing-startup"))?;
    let publisher = if checkout_selected {
        Some(startup_store.trusted_checkout_publisher(publisher_pool.as_ref().ok_or("Missing publisher")?.clone()).await?)
    } else { None };
    let portal_publisher = if portal_selected {
        Some(startup_store.trusted_portal_publisher(publisher_pool.as_ref().ok_or("Missing publisher")?.clone()).await?)
    } else { None };
    {{#stripe}}
    let gateway = env::var("PLATFORM_GATEWAY_URL").ok().filter(|value|!value.is_empty());
    let instance_id = env::var("INSTANCE_ID").ok().filter(|value|!value.is_empty());
    let instance_key = env::var("INSTANCE_HMAC_KEY").ok().filter(|value|!value.is_empty());
    let client = if gateway.is_some() && instance_id.is_some() && instance_key.is_some() {
        StripeBillingClient::with_gateway(forklaunch_blueprint_core::instance_gateway::InstanceGatewayClient::from_env()?)
    } else {
        if gateway.is_some() || instance_id.is_some() { return Err("Managed Stripe requires PLATFORM_GATEWAY_URL, INSTANCE_ID and INSTANCE_HMAC_KEY together".into()); }
        let secret = env::var("STRIPE_API_KEY").ok().filter(|value|!value.is_empty()).or_else(||env::var("STRIPE_SECRET_KEY").ok().filter(|value|!value.is_empty())).ok_or(()).map_err(|_| "STRIPE_API_KEY or STRIPE_SECRET_KEY is required outside managed mode")?;
        if let Ok(base) = env::var("STRIPE_API_BASE") {
            StripeBillingClient::with_transport(secret, ProviderHttpClient::new(&base,Duration::from_secs(15),1024*1024)?)?
        } else {StripeBillingClient::new(secret)?}
    };
    let mut backend = BillingBackend::stripe(store,client);
    if let Some(secret) = env::var("STRIPE_WEBHOOK_SECRET").ok().filter(|value|!value.is_empty()) { backend = backend.with_webhook_secret(secret)?; }
    if let Some(secret) = instance_key { backend = backend.with_platform_event_key(secret)?; }
    {{/stripe}}{{^stripe}}let backend = BillingBackend::base(store);{{/stripe}}
    let backend = backend.with_database_backup(env::var("BILLING_DATABASE_BACKUP").unwrap_or_else(|_|"false".into()).parse()?);
    let backend = backend.with_request_dates(request_date_profile_from_env()?);
    let backend = if let Some(config)=bound_billing_config {
        backend.with_bound_billing(BoundBillingV1::trusted(config,publisher,portal_publisher)?).await?
    } else {
        match (bound_checkout_config,publisher) {
            (Some(config),Some(publisher)) => backend.with_bound_checkout(BoundCheckoutV1::trusted(config,publisher)).await?,
            (None,None) => backend,
            _ => return Err("Incomplete bound billing configuration".into()),
        }
    };
    let host = env::var("HOST").or_else(|_| env::var("SERVICE_HOST")).unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port: u16 = env::var("SERVICE_PORT").or_else(|_| env::var("PORT")).unwrap_or_else(|_| "8000".to_owned()).parse()?;
    let listener = tokio::net::TcpListener::bind((host.as_str(), port)).await?;
    println!("{} listening on http://{}", {{crate_module}}::SERVICE_NAME, listener.local_addr()?);
    {{crate_module}}::routes::register_router(billing_router(backend, auth, service_info())).serve_with_default_mcp(listener, shutdown()).await?;
    if let Some(pool) = publisher_pool { pool.close().await; }
    pool.close().await;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut signal) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = signal.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
