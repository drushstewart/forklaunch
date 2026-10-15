pub mod routes;

use forklaunch_contract::{ContractRegistry, ServiceContract};
use forklaunch_billing_interface::module_contracts as contracts;
use std::{fs, path::Path};
use forklaunch_billing_axum::{BillingRequestDateProfile, BillingIdentityProfile, BoundCheckoutConfig, bound_checkout_contracts};
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
    contract_registry_with_profiles(profile, BillingIdentityProfile::Legacy)
}

pub fn identity_profile_from_env() -> BlueprintResult<BillingIdentityProfile> {
    let selected: BillingIdentityProfile = std::env::var("BILLING_IDENTITY_PROFILE").unwrap_or_else(|_| "legacy".into()).parse()?;
    let config = std::env::var("BILLING_BOUND_CHECKOUT_CONFIG").ok();
    let publisher = std::env::var("BILLING_PUBLISHER_DATABASE_URL").ok();
    let invalid = || forklaunch_blueprint_core::BlueprintError::new("billing_configuration", "Invalid bound Checkout startup configuration");
    match selected {
        BillingIdentityProfile::Legacy if config.is_some() || publisher.is_some() => return Err(invalid()),
        BillingIdentityProfile::BoundCheckoutV1 => {
            if !{{#stripe}}true{{/stripe}}{{^stripe}}false{{/stripe}} || publisher.as_ref().is_none_or(|v|v.trim().is_empty()) { return Err(invalid()); }
            BoundCheckoutConfig::from_json(config.as_deref().ok_or_else(invalid)?)?;
        }
        _ => {}
    }
    Ok(selected)
}

pub fn bound_checkout_config_from_env() -> BlueprintResult<Option<BoundCheckoutConfig>> {
    match identity_profile_from_env()? {
        BillingIdentityProfile::Legacy => Ok(None),
        BillingIdentityProfile::BoundCheckoutV1 => Ok(Some(BoundCheckoutConfig::from_json(&std::env::var("BILLING_BOUND_CHECKOUT_CONFIG").map_err(|_| forklaunch_blueprint_core::BlueprintError::new("billing_configuration", "Missing bound Checkout configuration"))?)?)),
    }
}

pub fn contract_registry_with_profiles(profile: BillingRequestDateProfile, identity: BillingIdentityProfile) -> BlueprintResult<ContractRegistry> {
    if !{{#stripe}}true{{/stripe}}{{^stripe}}false{{/stripe}} && identity == BillingIdentityProfile::BoundCheckoutV1 {
        return Err(forklaunch_blueprint_core::BlueprintError::new("billing_configuration", "Bound Checkout requires Stripe billing"));
    }
    let mut registry = ContractRegistry::for_service(ServiceContract::new(SERVICE_NAME, SERVICE_VERSION));
    for route in contracts::module_registry({{#stripe}}true{{/stripe}}{{^stripe}}false{{/stripe}}).routes() {
        let mut route = route.clone();
        profile.project(&mut route)?;
        registry.register(route);
    }
    if identity == BillingIdentityProfile::BoundCheckoutV1 {
        for route in bound_checkout_contracts() { registry.register(route); }
    }
    for route in routes::contracts() { registry.register(route); }
    Ok(registry)
}

pub fn write_contract_artifacts(contract_output: impl AsRef<Path>, openapi_output: impl AsRef<Path>) -> std::io::Result<()> {
    let profile = request_date_profile_from_env().map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string()))?;
    let identity = identity_profile_from_env().map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string()))?;
    let registry = contract_registry_with_profiles(profile, identity).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string()))?;
    if let Some(parent) = contract_output.as_ref().parent() { fs::create_dir_all(parent)?; }
    fs::write(contract_output, registry.clone().into_artifact().to_json())?;
    forklaunch_openapi::write_openapi_json(&registry,
        &forklaunch_openapi::OpenApiOptions::from_registry(&registry), openapi_output)
}
