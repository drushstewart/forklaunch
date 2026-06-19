use std::{env, fs, path::Path};

use {{crate_module}}::{
    WORKER_NAME, build_context, config::WorkerConfig, run_once, write_contract_artifacts,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if export_contract_artifacts()? {
        return Ok(());
    }

    let context = build_context(WorkerConfig::from_env())?;
    let report = run_once(&context)?;
    println!(
        "{} processed {} job(s): {}",
        WORKER_NAME, report.processed, report.message
    );
    Ok(())
}

fn export_contract_artifacts() -> std::io::Result<bool> {
    let mode = env::var("FORKLAUNCH_MODE").unwrap_or_default();
    if mode != "openapi" && mode != "contract" {
        return Ok(false);
    }

    let contract_output = env::var("FORKLAUNCH_CONTRACT_OUTPUT")
        .unwrap_or_else(|_| ".forklaunch/contracts/contract.json".to_string());
    let openapi_output = env::var("FORKLAUNCH_OPENAPI_OUTPUT")
        .unwrap_or_else(|_| ".forklaunch/openapi/openapi.json".to_string());

    if let Some(parent) = Path::new(&openapi_output).parent() {
        fs::create_dir_all(parent)?;
    }
    write_contract_artifacts(contract_output, openapi_output)?;
    Ok(true)
}
