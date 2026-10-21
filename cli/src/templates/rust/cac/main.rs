use std::{env, sync::Arc, error::Error};
use forklaunch_core::auth::jwt::{RemoteJwks, RemoteJwksConfig};
use forklaunch_cac_axum::{CacAuthConfig, cac_router};
use forklaunch_cac_base::MockProcedureCodeProvider;
use forklaunch_core::telemetry::{OpenTelemetryCollector, otlp::TelemetryRuntime};
use jsonwebtoken::{Algorithm, Validation};
use {{crate_module}}::{service_info, write_contract_artifacts};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
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

async fn run() -> Result<(), Box<dyn Error>> {
    if env::args().len() != 1 { return Err("CAC expects no command arguments".into()); }
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
    let auth = CacAuthConfig::new(secret.into_bytes(), jwks, validation)?;
    let provider = Arc::new(MockProcedureCodeProvider::new(OpenTelemetryCollector::new({{crate_module}}::SERVICE_NAME)));
    let host = env::var("HOST").or_else(|_| env::var("SERVICE_HOST")).unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port: u16 = env::var("SERVICE_PORT").or_else(|_| env::var("PORT")).unwrap_or_else(|_| "8000".to_owned()).parse()?;
    let listener = TcpListener::bind((host.as_str(), port)).await?;
    println!("{} listening on http://{}", {{crate_module}}::SERVICE_NAME, listener.local_addr()?);
    {{crate_module}}::routes::register_router(cac_router(provider, auth, service_info())).serve_with_default_mcp(listener, shutdown()).await?;
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
