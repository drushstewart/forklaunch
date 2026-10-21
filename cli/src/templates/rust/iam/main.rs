use std::{env, sync::Arc, error::Error};
use forklaunch_core::persistence::{TenantContext,encryption::FieldEncryptor};
use forklaunch_iam_base::directory::postgres::PostgresDirectoryStore;
use forklaunch_core::auth::jwt::{RemoteJwks,RemoteJwksConfig};
use forklaunch_axum::module_auth::HmacJwksAuth;
use forklaunch_core::domain::AuditContext;
use jsonwebtoken::{Algorithm,Validation};
use forklaunch_iam_base::postgres::PostgresIamService;
use sqlx::postgres::PgPoolOptions;
use {{crate_module}}::{service_info, write_contract_artifacts};
use forklaunch_core::{auth::oidc::OidcProvider, telemetry::otlp::TelemetryRuntime};
use forklaunch_iam_axum::{browser::BrowserIam, directory::{self, DirectoryAuthorizationMode}};
use forklaunch_iam_base::{browser_sessions::PostgresCookieStore, jwt_sessions::RsaJwtSessionIssuer};
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
    let telemetry = TelemetryRuntime::start(
        {{crate_module}}::SERVICE_NAME, env!("CARGO_PKG_VERSION"),
    ).await?;
    let result = run().await;
    let flushed = telemetry.shutdown().await;
    result?;
    flushed?;
    Ok(())
}

async fn run() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<_> = env::args().skip(1).collect();
    if !arguments.is_empty() && arguments != ["--migrate"] {
        return Err("Expected no arguments or --migrate".into());
    }
    let database_url = env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?;
    let pool = PgPoolOptions::new().max_connections(10).connect(&database_url).await
        .map_err(|_| "Unable to connect to PostgreSQL")?;
    if arguments == ["--migrate"] {
        PostgresIamService::migrate(&pool).await?;
        pool.close().await;
        return Ok(());
    }
    let tenant = TenantContext::new(env::var("TENANT_ID").map_err(|_| "TENANT_ID is required")?)?;
    let directory_mode = match env::var("IAM_DIRECTORY_AUTHORIZATION") {
        Err(env::VarError::NotPresent) => DirectoryAuthorizationMode::JwtClaims,
        Ok(value) if value == "jwt-claims" => DirectoryAuthorizationMode::JwtClaims,
        Ok(value) if value == "stored-session" => DirectoryAuthorizationMode::StoredSessions,
        _ => return Err("IAM_DIRECTORY_AUTHORIZATION must be jwt-claims or stored-session".into()),
    };
    let issuer = RsaJwtSessionIssuer::from_env()?;
    if directory_mode == DirectoryAuthorizationMode::StoredSessions && issuer.is_none() {
        return Err("Stored-session directory authorization requires SESSION_JWT signing configuration".into());
    }
    let store = PostgresIamService::new(pool.clone(), tenant.clone()).await?;
    let store = match issuer {
        Some(issuer) => store.with_jwt_session_issuer(Arc::new(issuer)),
        None => store,
    };
    let store = Arc::new(store);
    let encryptor = Arc::new(FieldEncryptor::from_env()?);
    let oidc_enabled = ["SESSION_OIDC_ISSUER", "SESSION_OIDC_CLIENT_ID", "SESSION_OIDC_CLIENT_SECRET"]
        .iter().any(|name| env::var_os(name).is_some());
    if oidc_enabled && env::var_os("SESSION_COOKIE_ORIGIN").is_none() {
        return Err("OIDC requires SESSION_COOKIE_ORIGIN".into());
    }
    let browser = match env::var("SESSION_COOKIE_ORIGIN") {
        Ok(origin) => {
            let cookie_store = PostgresCookieStore::new(
                pool.clone(), tenant.clone(), {{crate_module}}::SERVICE_NAME, encryptor.clone(),
            ).await?;
            let browser = BrowserIam::new(store.clone(), cookie_store.clone(), "__Host-iam", &origin)?;
            let browser = if oidc_enabled {
                let issuer = env::var("SESSION_OIDC_ISSUER").map_err(|_| "SESSION_OIDC_ISSUER is required")?;
                let client_id = env::var("SESSION_OIDC_CLIENT_ID").map_err(|_| "SESSION_OIDC_CLIENT_ID is required")?;
                let secret = match env::var("SESSION_OIDC_CLIENT_SECRET") {
                    Ok(value) => Some(value),
                    Err(env::VarError::NotPresent) => None,
                    Err(_) => return Err("Invalid OIDC client secret".into()),
                };
                let provider = OidcProvider::discover(
                    &issuer, client_id, secret, &format!("{origin}/auth/browser/oidc/callback"),
                ).await?;
                browser.with_oidc(store.clone(), cookie_store, Arc::new(provider), vec![])?
            } else { browser };
            Some(browser)
        }
        Err(env::VarError::NotPresent) => None,
        Err(env::VarError::NotUnicode(_)) => return Err("Invalid browser session origin".into()),
    };
    let directory=PostgresDirectoryStore::new(pool.clone(),tenant,encryptor,AuditContext::system("iam-startup")).await?;
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
    let host = env::var("HOST").or_else(|_| env::var("SERVICE_HOST")).unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port: u16 = env::var("SERVICE_PORT").or_else(|_| env::var("PORT")).unwrap_or_else(|_| "8000".to_owned()).parse()?;
    let listener = TcpListener::bind((host.as_str(), port)).await?;
    println!("{} listening on http://{}", {{crate_module}}::SERVICE_NAME, listener.local_addr()?);
    let router = directory::iam_router_with_options(store, directory, auth, service_info(), Default::default(), directory_mode);
    let router = match browser { Some(browser) => browser.mount(router), None => router };
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
