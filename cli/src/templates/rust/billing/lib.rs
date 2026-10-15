pub mod routes;

use forklaunch_contract::{ContractRegistry, ServiceContract};
use forklaunch_billing_interface::module_contracts as contracts;
use std::{fs, path::Path};
use forklaunch_billing_axum::BillingRequestDateProfile;
use forklaunch_blueprint_core::BlueprintResult;

pub const SERVICE_NAME: &str = "{{{name}}}";
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn service_info() -> forklaunch_core::ServiceInfo {
    forklaunch_core::ServiceInfo::new(SERVICE_NAME, SERVICE_VERSION)
}

pub fn contract_registry() -> ContractRegistry {
    contract_registry_with_request_dates(BillingRequestDateProfile::Legacy)
        .expect("package-owned billing request schemas")
}

pub fn request_date_profile_from_env() -> BlueprintResult<BillingRequestDateProfile> {
    std::env::var("BILLING_REQUEST_DATE_PROFILE").unwrap_or_else(|_| "legacy".into()).parse()
}

pub fn contract_registry_with_request_dates(profile: BillingRequestDateProfile) -> BlueprintResult<ContractRegistry> {
    let mut registry = ContractRegistry::for_service(ServiceContract::new(SERVICE_NAME, SERVICE_VERSION));
    for route in contracts::module_registry({{#stripe}}true{{/stripe}}{{^stripe}}false{{/stripe}}).routes() {
        let mut route = route.clone();
        profile.project(&mut route)?;
        registry.register(route);
    }
    for route in routes::contracts() { registry.register(route); }
    Ok(registry)
}

pub fn write_contract_artifacts(contract_output: impl AsRef<Path>, openapi_output: impl AsRef<Path>) -> std::io::Result<()> {
    let profile = request_date_profile_from_env().map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string()))?;
    let registry = contract_registry_with_request_dates(profile).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string()))?;
    if let Some(parent) = contract_output.as_ref().parent() { fs::create_dir_all(parent)?; }
    fs::write(contract_output, registry.clone().into_artifact().to_json())?;
    forklaunch_openapi::write_openapi_json(&registry,
        &forklaunch_openapi::OpenApiOptions::from_registry(&registry), openapi_output)
}
