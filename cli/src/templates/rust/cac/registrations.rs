//! Builds the code-set provider and HTTP handlers from trusted configuration.
use crate::{config::ServiceConfig, domain::services::MockProcedureCodeProvider, error::Result, service_info};
use forklaunch_cac_base::http::{CacHttp, CacRoutes};
use forklaunch_core::telemetry::OpenTelemetryCollector;
use std::sync::Arc;

/// The module's routes, collected before mounting.
pub type Routes = CacRoutes<MockProcedureCodeProvider>;

#[derive(Clone)]
pub struct AppState {
    pub config: ServiceConfig,
    pub cac: CacHttp<MockProcedureCodeProvider>,
}

pub async fn register(config: ServiceConfig) -> Result<AppState> {
    let provider = Arc::new(MockProcedureCodeProvider::new(OpenTelemetryCollector::new(crate::SERVICE_NAME)));
    let cac = CacHttp::new(provider, config.module_auth()?, service_info());
    Ok(AppState { config, cac })
}
