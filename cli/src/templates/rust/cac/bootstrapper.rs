//! Process lifecycle: contract export, telemetry, registration and serving.
use crate::{config::ServiceConfig, error::Result, registrations, server, write_contract_artifacts};
use forklaunch_core::telemetry::otlp::TelemetryRuntime;
use std::env;
use tokio::net::TcpListener;

pub async fn run() -> Result<()> {
    if matches!(env::var("FORKLAUNCH_MODE").as_deref(), Ok("openapi" | "contract")) {
        write_contract_artifacts(
            env::var("FORKLAUNCH_CONTRACT_OUTPUT").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/sdk/typescript/contract.json").to_owned()),
            env::var("FORKLAUNCH_OPENAPI_OUTPUT").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/sdk/typescript/openapi.json").to_owned()),
        )?;
        return Ok(());
    }
    let telemetry = TelemetryRuntime::start(crate::SERVICE_NAME, env!("CARGO_PKG_VERSION")).await?;
    let result = start().await;
    let flushed = telemetry.shutdown().await;
    result?;
    flushed?;
    Ok(())
}

async fn start() -> Result<()> {
    if env::args().len() != 1 {
        return Err("CAC expects no command arguments".into());
    }
    let state = registrations::register(ServiceConfig::from_env()?).await?;
    let listener = TcpListener::bind(state.config.bind_addr()).await?;
    println!("{} listening on http://{}", crate::SERVICE_NAME, listener.local_addr()?);
    server::serve(&state, listener, server::shutdown()).await?;
    Ok(())
}
