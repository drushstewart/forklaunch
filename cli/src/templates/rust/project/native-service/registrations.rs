use crate::{config::ServiceConfig, domain::{services::HealthService, schemas::Record}, error::Result, persistence::InMemoryRepository};
use forklaunch_core::telemetry::OpenTelemetryCollector;

#[derive(Clone)]
pub struct AppState {
    pub config: ServiceConfig,
    pub health: HealthService,
    pub repository: InMemoryRepository,
    pub telemetry: OpenTelemetryCollector,
}

pub fn register(config: ServiceConfig) -> Result<AppState> {
    config.validate()?;
    let repository = InMemoryRepository::default();
    repository.seed(Record {
        id: "welcome".into(), name: format!("{} is ready", config.name),
    })?;
    let telemetry = OpenTelemetryCollector::new(config.name.clone());
    Ok(AppState { telemetry, health: HealthService::new(config.name.clone(), repository.clone()), repository, config })
}
