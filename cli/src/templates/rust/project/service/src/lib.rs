pub mod config;
pub mod domain;
pub mod error;
pub mod persistence;
pub mod routes;

use config::ServiceConfig;
use domain::Record;
use error::Result;
use forklaunch_axum::AxumApplication;
use forklaunch_contract::{
    BodyContract, BodyKind, ContractRegistry, RouteContract, RouteOptions, SchemaRef,
    ServiceContract,
};
use forklaunch_core::ServiceInfo;
use forklaunch_http::{FrameworkResponse, HttpApplication, LocalRouter};
use persistence::InMemoryRepository;
use std::{fs, path::Path};

pub const SERVICE_NAME: &str = "{{{name}}}";
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_OPENAPI_PATH: &str = "/openapi.json";

#[derive(Debug, Clone)]
pub struct AppState {
    config: ServiceConfig,
    repository: InMemoryRepository,
}

impl AppState {
    #[must_use]
    pub fn config(&self) -> &ServiceConfig {
        &self.config
    }

    #[must_use]
    pub fn repository(&self) -> &InMemoryRepository {
        &self.repository
    }
}

pub fn build_state(config: ServiceConfig) -> Result<AppState> {
    config.validate()?;

    let repository = InMemoryRepository::default();
    repository.seed(Record::new(
        "welcome",
        format!("{} is ready", config.name),
    ))?;

    Ok(AppState { config, repository })
}

pub fn health_check(state: &AppState) -> routes::HealthResponse {
    routes::health::handle(state)
}

pub fn route_names() -> Vec<&'static str> {
    routes::route_names()
}

#[must_use]
pub fn service_info() -> ServiceInfo {
    ServiceInfo::new(SERVICE_NAME, SERVICE_VERSION)
}

#[must_use]
pub fn contract_registry() -> ContractRegistry {
    let mut registry = ContractRegistry::for_service(ServiceContract::new(
        SERVICE_NAME,
        SERVICE_VERSION,
    ));
    for contract in routes::contracts() {
        registry.register(contract);
    }
    registry
}

#[must_use]
pub fn http_application() -> HttpApplication {
    let mut application = HttpApplication::new(service_info());
    for contract in routes::contracts() {
        application.register_route(contract);
    }
    application.register_route(openapi_route_contract(DEFAULT_OPENAPI_PATH));
    application
}

#[must_use]
pub fn axum_application() -> AxumApplication {
    AxumApplication::new(http_application())
}

#[must_use]
pub fn local_router(state: AppState) -> LocalRouter {
    let docs_path = state.config().docs_path.clone();
    let openapi_config = state.config().clone();
    let mut router = LocalRouter::new();
    routes::register_handlers(&mut router, state);
    register_openapi_route(&mut router, DEFAULT_OPENAPI_PATH, openapi_config.clone());
    if docs_path != DEFAULT_OPENAPI_PATH {
        register_openapi_route(&mut router, &docs_path, openapi_config);
    }
    router
}

#[must_use]
pub fn openapi_json() -> String {
    openapi_json_with_config(&ServiceConfig::from_env())
}

#[must_use]
pub fn openapi_json_with_config(config: &ServiceConfig) -> String {
    let registry = contract_registry();
    forklaunch_openapi::generate_openapi_json_with_options(
        &registry,
        &openapi_options(&registry, config),
    )
}

fn openapi_options(
    registry: &ContractRegistry,
    config: &ServiceConfig,
) -> forklaunch_openapi::OpenApiOptions {
    let mut options = forklaunch_openapi::OpenApiOptions::from_registry(registry);
    for (index, url) in config.docs_server_urls.iter().enumerate() {
        let description = config
            .docs_server_descriptions
            .get(index)
            .cloned()
            .unwrap_or_else(|| "Main Server".to_string());
        options = options
            .with_server(forklaunch_openapi::OpenApiServer::new(url).with_description(description));
    }
    options
}

fn register_openapi_route(router: &mut LocalRouter, path: &str, config: ServiceConfig) {
    router.register(openapi_route_contract(path), move |_request| {
        Ok(FrameworkResponse::json(
            200,
            openapi_json_with_config(&config),
        ))
    });
}

fn openapi_route_contract(path: &str) -> RouteContract {
    let mut options = RouteOptions::default();
    options.openapi = false;
    options.mcp = false;

    RouteContract::get("Get OpenAPI", "Serves the OpenAPI document", path)
        .with_response(
            200,
            Some(BodyContract {
                kind: BodyKind::Json,
                schema: SchemaRef::named("OpenApiDocument"),
                content_type: Some("application/json".to_string()),
            }),
        )
        .with_options(options)
}

pub fn write_contract_artifacts(
    contract_output: impl AsRef<Path>,
    openapi_output: impl AsRef<Path>,
) -> std::io::Result<()> {
    let registry = contract_registry();
    let config = ServiceConfig::from_env();

    if let Some(parent) = contract_output.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(contract_output, registry.clone().into_artifact().to_json())?;

    forklaunch_openapi::write_openapi_json(
        &registry,
        &openapi_options(&registry, &config),
        openapi_output,
    )
}
