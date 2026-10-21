use std::fmt;
// Deliberately no plaintext, request, URL, token, or SQL diagnostics in persisted job errors.
#[derive(Debug)]
pub enum WorkerError {
    InvalidEnvelope,
    Storage,
    Publication,
    Configuration,
}
impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidEnvelope => "invalid sealed job",
            Self::Storage => "queue storage unavailable",
            Self::Publication => "insight publication unavailable",
            Self::Configuration => "worker configuration invalid",
        })
    }
}
impl std::error::Error for WorkerError {}
