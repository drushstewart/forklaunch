//! Explicit application dependencies, shared state, and resource registration.
use fieldnotes_contracts::PresenceEvent;
use fieldnotes_security::SecurityService;
use framework::native::Shutdown;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;
#[derive(Clone)]
pub struct AppState {
    pub telemetry: framework_core::telemetry::OpenTelemetryCollector,
    pub security: Arc<SecurityService>,
    pub notes: Arc<crate::domain::services::NotesService>,
    pub public_origin: Arc<str>,
    pub(crate) presence: broadcast::Sender<(Uuid, PresenceEvent)>,
    pub(crate) shutdown: Shutdown,
}
impl AppState {
    pub fn begin_shutdown(&self) {
        self.shutdown.trigger();
    }
    pub fn new(
        security: SecurityService,
        notes: crate::NotesRepository,
        public_origin: String,
    ) -> Result<Self, fieldnotes_security::SecurityError> {
        let (presence, _) = broadcast::channel(256);
        Ok(Self {
            telemetry: framework_core::telemetry::OpenTelemetryCollector::new("notes"),
            security: Arc::new(security),
            notes: Arc::new(crate::domain::services::NotesService::new(notes)?),
            public_origin: public_origin.into(),
            presence,
            shutdown: Shutdown::new(),
        })
    }
}

pub(crate) async fn register(
    config: crate::config::Config,
) -> Result<(AppState, String), Box<dyn std::error::Error>> {
    let pool = framework_core::persistence::postgres09::connect_runtime_postgres(
        &config.database_url,
        framework_core::persistence::postgres09::PostgresConnectionPolicy::from_env(10)?,
    )
    .await?;
    let key = fieldnotes_security::EnvelopeKey::from_env()?;
    let security = SecurityService::new(config.auth_url, config.internal_secret)?;
    let notes = crate::NotesRepository::new(pool, key)?
        .with_object_store_from_env()
        .await?
        .with_read_cache_from_env()?;
    Ok((
        AppState::new(security, notes, config.public_origin)?,
        config.listen_addr,
    ))
}
