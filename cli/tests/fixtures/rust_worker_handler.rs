use std::time::Duration;

use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use forklaunch_worker_database::runner::{JobFailure, JobHandler};
use forklaunch_worker_interface::Job;
pub struct ApplicationHandler;
impl JobHandler for ApplicationHandler {
    async fn process(&self, job: Job) -> BlueprintResult<()> {
        let payload: serde_json::Value = serde_json::from_str(&job.payload_json)
            .map_err(|_| BlueprintError::new("test_invalid_payload", "test payload invalid"))?;
        if !payload["slow_first"].as_bool().unwrap_or(false) || job.attempts == 1 {
            tokio::time::sleep(Duration::from_millis(
                payload["sleep_ms"].as_u64().unwrap_or(0),
            ))
            .await;
        }
        if payload["fail"].as_bool().unwrap_or(false) {
            Err(BlueprintError::new(
                "test_handler_failure",
                "sensitive detail must not be persisted",
            ))
        } else {
            Ok(())
        }
    }
    async fn process_batch(&self, jobs: Vec<Job>) -> BlueprintResult<Vec<JobFailure>> {
        println!("batch received: jobs={}", jobs.len());
        let mut failures = Vec::new();
        for job in jobs {
            let id = job.id.clone();
            if let Err(error) = self.process(job).await {
                failures.push(JobFailure { id, error });
            }
        }
        Ok(failures)
    }
}
