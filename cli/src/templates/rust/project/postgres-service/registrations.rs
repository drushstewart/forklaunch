use crate::{config::ServiceConfig, domain::services::HealthService, error::Result,
    persistence::{PostgresRepository, database::DatabaseConfig}};
use forklaunch_core::telemetry::OpenTelemetryCollector;
#[derive(Clone)]
pub struct AppState {
    pub config: ServiceConfig,
    pub health: HealthService,
    pub repository: PostgresRepository,
    pub telemetry: OpenTelemetryCollector,
}
pub async fn register(config: ServiceConfig) -> Result<AppState> {
    config.validate()?;
    let repository = PostgresRepository::connect(DatabaseConfig::from_env()?).await?;
    let telemetry = OpenTelemetryCollector::new(config.name.clone());
    Ok(AppState { telemetry, health: HealthService::new(config.name.clone(), repository.clone()), repository, config })
}
