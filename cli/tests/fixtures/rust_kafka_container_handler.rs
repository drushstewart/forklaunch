use std::time::Duration;
use forklaunch_core::domain::BlueprintError;
use forklaunch_core::domain::BlueprintResult;
use forklaunch_worker_database::runner::JobHandler;
use forklaunch_worker_interface::Job;
pub struct ApplicationHandler;
impl JobHandler for ApplicationHandler {
    async fn process(&self, job: Job) -> BlueprintResult<()> {
        let event = crate::events::open(&job)?;
        // Explicit test controls are read only after the production boundary authenticates.
        let payload: serde_json::Value = serde_json::from_str(&event.input.message)
            .map_err(|_| BlueprintError::new("test_invalid_payload", "test payload invalid"))?;
        println!("test-handler-started {}", job.id);
        tokio::time::sleep(Duration::from_millis(payload["sleep_ms"].as_u64().unwrap_or(0))).await;
        if payload["fail"].as_bool().unwrap_or(false) {
            Err(BlueprintError::new("test_handler_failure", "sensitive detail must not be persisted"))
        } else { Ok(()) }
    }
}
