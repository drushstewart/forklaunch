use std::{env, sync::Arc};
use forklaunch_auth::jwt::{RemoteJwks, RemoteJwksConfig};
use forklaunch_axum::module_auth::HmacJwksAuth;
use forklaunch_blueprint_core::AuditContext;
use forklaunch_core::persistence::{TenantContext,encryption::FieldEncryptor};
use forklaunch_messaging_base::postgres::PostgresSmsStore;
use forklaunch_messaging_axum::{MessagingBackend,messaging_router};
{{#twilio}}use forklaunch_messaging_twilio::TwilioSmsService;{{/twilio}}
use sqlx::postgres::PgPoolOptions;
use forklaunch_core::telemetry::otlp::TelemetryRuntime;
use jsonwebtoken::{Algorithm, Validation};
use {{crate_module}}::{service_info, write_contract_artifacts};

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
        PostgresSmsStore::migrate(&pool).await?;
        pool.close().await;
        return Ok(());
    }
    let store = PostgresSmsStore::new(pool.clone(),
        TenantContext::new(env::var("TENANT_ID").map_err(|_| "TENANT_ID is required")?)?,
        Arc::new(FieldEncryptor::from_env()?),AuditContext::system("messaging-startup"),
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
    {{#twilio}}
    let mut provider = TwilioSmsService::new(store,
        env::var("TWILIO_ACCOUNT_SID").map_err(|_| "TWILIO_ACCOUNT_SID is required")?,
        env::var("TWILIO_AUTH_TOKEN").map_err(|_| "TWILIO_AUTH_TOKEN is required")?,
        env::var("TWILIO_FROM_NUMBER").map_err(|_| "TWILIO_FROM_NUMBER is required")?,
    )?.with_status_callback(env::var("TWILIO_STATUS_CALLBACK_URL").map_err(|_| "TWILIO_STATUS_CALLBACK_URL is required")?)?;
    if let Ok(base) = env::var("TWILIO_API_BASE") { provider = provider.with_api_base(&base)?; }
    let backend = MessagingBackend::Twilio(provider);
    {{/twilio}}{{^twilio}}let backend = MessagingBackend::Base(store);{{/twilio}}
    let host = env::var("HOST").or_else(|_| env::var("SERVICE_HOST")).unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port: u16 = env::var("SERVICE_PORT").or_else(|_| env::var("PORT")).unwrap_or_else(|_| "8000".to_owned()).parse()?;
    let listener = tokio::net::TcpListener::bind((host.as_str(), port)).await?;
    println!("{} listening on http://{}", {{crate_module}}::SERVICE_NAME, listener.local_addr()?);
    axum::serve(listener, messaging_router(backend, auth, service_info()).into_router()).with_graceful_shutdown(shutdown()).await?;
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
