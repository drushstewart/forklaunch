use crate::error::{Result, WorkerError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerConfig {
    pub name: String,
    pub worker_type: String,
    pub queue_name: String,
    pub batch_size: usize,
}

impl WorkerConfig {
    #[must_use]
    pub fn from_env() -> Self {
        let name = std::env::var("WORKER_NAME").unwrap_or_else(|_| "{{{name}}}".to_string());
        let worker_type = std::env::var("WORKER_TYPE").unwrap_or_else(|_| "database".to_string());
        let queue_name = std::env::var("WORKER_QUEUE").unwrap_or_else(|_| name.clone());
        let batch_size = std::env::var("WORKER_BATCH_SIZE")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1);

        Self {
            name,
            worker_type,
            queue_name,
            batch_size,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(WorkerError::InvalidConfig("worker name cannot be empty".to_string()));
        }
        if self.batch_size == 0 {
            return Err(WorkerError::InvalidConfig(
                "batch size must be greater than zero".to_string(),
            ));
        }
        Ok(())
    }
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            name: "{{{name}}}".to_string(),
            worker_type: "database".to_string(),
            queue_name: "{{{name}}}".to_string(),
            batch_size: 1,
        }
    }
}
