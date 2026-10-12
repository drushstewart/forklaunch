pub mod routes;

use forklaunch_contract::{BodyContract, ContractRegistry, RouteContract};
use std::{fs, path::Path};

pub const SERVICE_NAME: &str = "{{{name}}}";
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn service_info() -> forklaunch_core::ServiceInfo {
    forklaunch_core::ServiceInfo::new(SERVICE_NAME, SERVICE_VERSION)
}

pub fn contract_registry() -> ContractRegistry {
    let mut registry = forklaunch_iam_better_auth::http::application::registry(SERVICE_NAME, SERVICE_VERSION);
    registry.register(health_contract());
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

pub fn health_contract() -> RouteContract {
    let mut body = BodyContract::json("Health");
    body.schema.json_schema = Some(r#"{"type":"object","required":["status","service"],"properties":{"status":{"type":"string"},"service":{"type":"string"}}}"#.to_owned());
    RouteContract::get("Get Health", "Service health", "/health").with_response(200, Some(body))
}
