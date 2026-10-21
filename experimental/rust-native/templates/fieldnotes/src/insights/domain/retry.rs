use crate::WorkerError;

/// Framework policy bounds retries and rejects permanent errors even when boxed
/// by the native Apalis runtime. No transaction is held during backoff.
pub fn retry_policy() -> Result<::worker::apalis::SecureRetryPolicy, WorkerError> {
    ::worker::apalis::retry_policy(::worker::RetrySettings::default())
        .map_err(|_| WorkerError::Configuration)
}
