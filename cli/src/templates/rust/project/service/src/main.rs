use std::{env, fs, path::Path, error::Error, io};

use {{crate_module}}::{build_state, config::ServiceConfig, router, write_contract_artifacts};
use forklaunch_core::telemetry::otlp::TelemetryRuntime;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    if export_contract_artifacts()? {
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
    let config = ServiceConfig::from_env();
    let listener = TcpListener::bind(config.bind_addr()).await?;
    let state = build_state(config)?;
    println!("{} listening on http://{}", {{crate_module}}::SERVICE_NAME, listener.local_addr()?);
    router(state).serve_with_default_mcp(listener, shutdown_signal()).await?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

fn export_contract_artifacts() -> io::Result<bool> {
    let mode = env::var("FORKLAUNCH_MODE").unwrap_or_default();
    if mode != "openapi" && mode != "contract" {
        return Ok(false);
    }

    let contract_output = env::var("FORKLAUNCH_CONTRACT_OUTPUT")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/sdk/typescript/contract.json").to_string());
    let openapi_output = env::var("FORKLAUNCH_OPENAPI_OUTPUT")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/sdk/typescript/openapi.json").to_string());

    if let Some(parent) = Path::new(&openapi_output).parent() {
        fs::create_dir_all(parent)?;
    }
    write_contract_artifacts(contract_output, openapi_output)?;
    Ok(true)
}
