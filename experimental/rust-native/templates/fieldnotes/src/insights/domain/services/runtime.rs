use crate::domain::services::process_job;
use crate::persistence::dispatch_batch;
use crate::registrations::Dependencies;
use crate::{QUEUE, SealedNoteJob, WorkerError};
use apalis::prelude::*;
use apalis_postgres::{Config, PostgresStorage};
use std::time::Duration;

pub(crate) async fn run(dependencies: Dependencies) -> Result<(), WorkerError> {
    let Dependencies {
        pool,
        state,
        maintenance,
    } = dependencies;
    let config = Config::default()
        .queue(QUEUE)
        .batch_size(4)
        .persist_results(true)
        .heartbeat_interval(Duration::from_secs(5))
        .missed_heartbeats(6);
    let backend = PostgresStorage::<SealedNoteJob>::new(&pool).with_config(config);
    let context = WorkerContext::new(&format!("insights-{}", uuid::Uuid::new_v4()));
    let worker = WorkerBuilder::new(&context)
        .backend(backend)
        .data(state)
        .retry(crate::retry_policy()?)
        .build(process_job);
    ::worker::native::supervise(
        worker.run(),
        ::worker::native::shutdown_signal(),
        || context.stop(),
        || async { dispatch_batch(&pool).await.map(|_| ()) },
        || async {
            if let Some(maintenance) = &maintenance {
                maintenance
                    .cleanup()
                    .await
                    .map_err(|_| WorkerError::Storage)?;
            }
            Ok::<_, WorkerError>(())
        },
        ::worker::native::SupervisorOptions::new(
            Duration::from_millis(500),
            Duration::from_secs(60),
            Duration::from_secs(10),
            Duration::from_secs(20),
        )
        .map_err(|_| WorkerError::Configuration)?,
    )
    .await
    .map_err(|_| WorkerError::Storage)
}
