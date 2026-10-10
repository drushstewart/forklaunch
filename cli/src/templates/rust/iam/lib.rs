pub mod routes;

use forklaunch_contract::{ContractRegistry, ServiceContract};
use forklaunch_iam_interface::contracts;
use std::{fs, path::Path};

pub const SERVICE_NAME: &str = "{{{name}}}";
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn service_info() -> forklaunch_core::ServiceInfo {
    forklaunch_core::ServiceInfo::new(SERVICE_NAME, SERVICE_VERSION)
}

pub fn contract_registry() -> ContractRegistry {
    let mut registry = ContractRegistry::for_service(ServiceContract::new(SERVICE_NAME, SERVICE_VERSION));
    for route in contracts::session_registry().routes() {
        registry.register(route.clone());
    }
    for route in routes::contracts() { registry.register(route); }
    registry
}

pub fn write_contract_artifacts(contract_output: impl AsRef<Path>, openapi_output: impl AsRef<Path>) -> std::io::Result<()> {
    let registry = contract_registry();
    if let Some(parent) = contract_output.as_ref().parent() { fs::create_dir_all(parent)?; }
    fs::write(contract_output, registry.clone().into_artifact().to_json())?;
    forklaunch_openapi::write_openapi_json(&registry,
        &forklaunch_openapi::OpenApiOptions::from_registry(&registry), openapi_output)
}
