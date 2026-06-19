use std::fmt;

pub type Result<T> = std::result::Result<T, WorkerError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerError {
    InvalidConfig(String),
    JobFailed(String),
}

impl fmt::Display for WorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => write!(formatter, "invalid config: {message}"),
            Self::JobFailed(message) => write!(formatter, "job failed: {message}"),
        }
    }
}

impl std::error::Error for WorkerError {}
