//! Contract export runs before application configuration or resource initialization.
use forklaunch_core::contract::ContractRegistry;
use std::{env, fs, io, path::Path};

/// Write language-neutral contracts and `OpenAPI` from the same registry.
///
/// # Errors
/// Propagates filesystem errors without starting application resources.
pub fn write_contract_artifacts(
    registry: &ContractRegistry,
    contract_output: impl AsRef<Path>,
    openapi_output: impl AsRef<Path>,
) -> io::Result<()> {
    let contract_output = contract_output.as_ref();
    let openapi_output = openapi_output.as_ref();
    for path in [contract_output, openapi_output] {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(contract_output, registry.clone().into_artifact().to_json())?;
    fs::write(
        openapi_output,
        forklaunch_core::openapi::generate_openapi_json(registry),
    )
}

/// Handle the CLI contract-export protocol before configuration/auth/database setup.
/// Returns true after export so the caller can exit without binding a listener.
/// Supports `FORKLAUNCH_MODE=contract|openapi` and direct `--export-contracts`.
/// Output locations are `FORKLAUNCH_CONTRACT_OUTPUT` and `FORKLAUNCH_OPENAPI_OUTPUT`,
/// defaulting to the service SDK folder (`sdk/typescript/`) under the working directory.
///
/// # Errors
/// Propagates filesystem errors; callers must fail startup rather than serve on error.
pub fn export_contracts_from_env(registry: &ContractRegistry) -> io::Result<bool> {
    let mode = env::var("FORKLAUNCH_MODE").unwrap_or_default();
    if !matches!(mode.as_str(), "contract" | "openapi")
        && !env::args().any(|argument| argument == "--export-contracts")
    {
        return Ok(false);
    }
    let contract = env::var_os("FORKLAUNCH_CONTRACT_OUTPUT")
        .unwrap_or_else(|| "sdk/typescript/contract.json".into());
    let openapi = env::var_os("FORKLAUNCH_OPENAPI_OUTPUT")
        .unwrap_or_else(|| "sdk/typescript/openapi.json".into());
    write_contract_artifacts(registry, contract, openapi)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use forklaunch_core::contract::{HttpMethod, RouteContract, ServiceContract};

    #[test]
    fn exports_one_registry_to_nested_contract_and_openapi_paths_without_resources() {
        let root = env::temp_dir().join(format!("native-contract-export-{}", uuid::Uuid::new_v4()));
        let mut registry = ContractRegistry::for_service(ServiceContract::new("notes", "1.0.0"));
        registry.register(RouteContract::new(
            "List Notes",
            "List notes",
            HttpMethod::Get,
            "/notes",
        ));
        let contract = root.join("contracts/notes.json");
        let openapi = root.join("openapi/notes.json");
        write_contract_artifacts(&registry, &contract, &openapi).unwrap();
        let contract: serde_json::Value =
            serde_json::from_slice(&fs::read(contract).unwrap()).unwrap();
        let openapi: serde_json::Value =
            serde_json::from_slice(&fs::read(openapi).unwrap()).unwrap();
        assert_eq!(contract["generator"], "framework-rs");
        assert_eq!(contract["service"]["name"], "notes");
        assert!(openapi["paths"]["/notes"]["get"].is_object());
        fs::remove_dir_all(root).unwrap();
    }
}
