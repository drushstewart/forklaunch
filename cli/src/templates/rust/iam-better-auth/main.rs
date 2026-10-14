use std::{env, sync::Arc};
use forklaunch_core::persistence::{TenantContext,encryption::FieldEncryptor};
use forklaunch_iam_better_auth::{http::AuthHttp, store::{postgres::PostgresAuthStore, MemberQueryProfile}, oauth::google_client::GoogleClient};
use forklaunch_auth::jwt::{Algorithm, RemoteJwks, RemoteJwksConfig, Validation};
use forklaunch_axum::module_auth::HmacJwksAuth;
use forklaunch_blueprint_core::AuditContext;
use sqlx::postgres::PgPoolOptions;
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
    let telemetry = forklaunch_core::telemetry::otlp::TelemetryRuntime::start(
        {{crate_module}}::SERVICE_NAME, env!("CARGO_PKG_VERSION"),
    ).await?;
    let result = run().await;
    let flushed = telemetry.shutdown().await;
    result?;
    flushed?;
    Ok(())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = env::args().skip(1).collect();
    if !arguments.is_empty() && arguments != ["--migrate"] {
        return Err("Expected no arguments or --migrate".into());
    }
    let database_url = env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?;
    let pool = PgPoolOptions::new().max_connections(10).connect(&database_url).await
        .map_err(|_| "Unable to connect to PostgreSQL")?;
    if arguments == ["--migrate"] {
        PostgresAuthStore::migrate(&pool).await?;
        pool.close().await;
        return Ok(());
    }
    let tenant = TenantContext::new(env::var("TENANT_ID").map_err(|_| "TENANT_ID is required")?)?;
    let store = Arc::new(PostgresAuthStore::new(pool.clone(), tenant,
        Arc::new(FieldEncryptor::from_env()?), AuditContext::system("better-auth-startup")).await?
        .with_member_query_profile(MemberQueryProfile::Postgres));
    let base_url = env::var("BETTER_AUTH_URL").map_err(|_| "BETTER_AUTH_URL is required")?;
    let auth_secret = env::var("BETTER_AUTH_SECRET").map_err(|_| "BETTER_AUTH_SECRET is required")?;
    let trusted = env::var("CORS_ORIGINS").unwrap_or_default().split(',')
        .map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned).collect();
    let mut application = AuthHttp::new(store, base_url.clone(), auth_secret.into_bytes(), trusted)?
        .with_base_path(env::var("BETTER_AUTH_BASE_PATH").unwrap_or_else(|_| "/api/auth".into()))?;
    let google_id = env::var("GOOGLE_CLIENT_ID").unwrap_or_default();
    let google_secret = env::var("GOOGLE_CLIENT_SECRET").unwrap_or_default();
    if !google_id.is_empty() || !google_secret.is_empty() {
        application = application.with_google(GoogleClient::new(google_id, google_secret)?);
    }
    let secret = env::var("HMAC_SECRET_KEY").map_err(|_| "HMAC_SECRET_KEY is required")?;
    let jwks = RemoteJwks::new(RemoteJwksConfig::new(
        env::var("JWKS_PUBLIC_KEY_URL").unwrap_or_else(|_| format!("{}/jwks", application.auth_base_url().trim_end_matches('/'))),
    ))?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.algorithms = vec![Algorithm::RS256, Algorithm::RS384, Algorithm::RS512,
        Algorithm::PS256, Algorithm::PS384, Algorithm::PS512, Algorithm::ES256, Algorithm::ES384, Algorithm::EdDSA];
    validation.validate_nbf = true;
    validation.leeway = 0;
    let issuer = application.auth_origin();
    validation.set_issuer(&[issuer.as_str()]);
    validation.set_audience(&[issuer.as_str()]);
    if let Ok(issuer) = env::var("JWT_ISSUER") { validation.set_issuer(&[issuer]); }
    if let Ok(audience) = env::var("JWT_AUDIENCE") { validation.validate_aud = true; validation.set_audience(&[audience]); }
    let auth = HmacJwksAuth::new(secret.into_bytes(), jwks, validation)?;
    let host = env::var("HOST").or_else(|_| env::var("SERVICE_HOST")).unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port: u16 = env::var("SERVICE_PORT").or_else(|_| env::var("PORT")).unwrap_or_else(|_| "8000".to_owned()).parse()?;
    let listener = tokio::net::TcpListener::bind((host.as_str(), port)).await?;
    println!("{} listening on http://{}", {{crate_module}}::SERVICE_NAME, listener.local_addr()?);
    let router = application.application_router(auth, service_info())
        .get({{crate_module}}::health_contract(), || async {
            axum::Json(serde_json::json!({"status":"ok","service":{{crate_module}}::SERVICE_NAME}))
        });
    {{crate_module}}::routes::register_router(router).serve_with_default_mcp(listener, shutdown()).await?;
    pool.close().await;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut signal) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = signal.recv() => {},
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
