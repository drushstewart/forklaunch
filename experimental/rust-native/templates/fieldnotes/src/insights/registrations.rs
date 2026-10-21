use crate::WorkerError;
use fieldnotes_security::EnvelopeKey;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub(crate) struct State {
    pub(crate) audit: Arc<dyn ::worker::JobAccessAudit>,
    pub(crate) key: Arc<EnvelopeKey>,
    pub(crate) api: fieldnotes_client::NotesClient,
    pub(crate) secret: Arc<zeroize::Zeroizing<String>>,
}

pub(crate) struct Dependencies {
    pub(crate) pool: PgPool,
    pub(crate) state: State,
    pub(crate) maintenance: Option<crate::persistence::AttachmentMaintenance>,
}

pub(crate) async fn register(pool: PgPool) -> Result<Dependencies, WorkerError> {
    let restricted: bool = sqlx::query_scalar("SELECT current_user='fieldnotes_worker' AND NOT rolsuper AND NOT rolbypassrls FROM pg_roles WHERE rolname=current_user").fetch_one(&pool).await.map_err(|_| WorkerError::Storage)?;
    if !restricted {
        return Err(WorkerError::Configuration);
    }
    let maintenance = if std::env::var_os("S3_BUCKET").is_some() {
        Some(
            crate::persistence::AttachmentMaintenance::from_env(pool.clone())
                .await
                .map_err(|_| WorkerError::Configuration)?,
        )
    } else {
        None
    };
    let key = EnvelopeKey::from_env().map_err(|_| WorkerError::Configuration)?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| WorkerError::Configuration)?;
    let api = fieldnotes_client::NotesClient::with_http_client(
        std::env::var("NOTES_INTERNAL_URL").map_err(|_| WorkerError::Configuration)?,
        http,
    );
    let api = if std::env::var("ALLOW_PLAINTEXT_SERVICE_TRANSPORT").as_deref() == Ok("true") {
        api.with_plaintext_development_transport()
    } else {
        api
    };
    let secret = std::env::var("INTERNAL_AUTH_SECRET").map_err(|_| WorkerError::Configuration)?;
    if secret.len() < 32 {
        return Err(WorkerError::Configuration);
    }
    // Dedicated bounded pool: held business/queue connections cannot prevent a
    // classified read from acknowledging its durable service access record.
    let audit_pool = framework_core::persistence::postgres09::clone_runtime_postgres_pool(
        &pool,
        framework_core::persistence::postgres09::PostgresConnectionPolicy::from_env(2)
            .map_err(|_| WorkerError::Configuration)?,
    )
    .map_err(|_| WorkerError::Storage)?;
    let state = State {
        audit: Arc::new(fieldnotes_security::ServiceJobAudit::new(
            audit_pool,
            fieldnotes_security::ServiceActor::Insights,
            crate::PURPOSE,
            "SealedNoteJob",
        )),
        key: Arc::new(key),
        api,
        secret: Arc::new(zeroize::Zeroizing::new(secret)),
    };
    Ok(Dependencies {
        pool,
        state,
        maintenance,
    })
}
