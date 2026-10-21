use std::{env, fs, path::Path, error::Error, io};

use {{crate_module}}::{
    WORKER_NAME, build_context, config::WorkerConfig, run_once, write_contract_artifacts,
};
use forklaunch_core::telemetry::otlp::TelemetryRuntime;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    if export_contract_artifacts()? {
        return Ok(());
    }

    let telemetry = TelemetryRuntime::start(
        WORKER_NAME, env!("CARGO_PKG_VERSION"),
    ).await?;
    let result = run();
    let flushed = telemetry.shutdown().await;
    result?;
    flushed?;
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let context = build_context(WorkerConfig::from_env())?;
    let report = run_once(&context)?;
    println!(
        "{} processed {} job(s): {}",
        WORKER_NAME, report.processed, report.message
    );
    Ok(())
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
