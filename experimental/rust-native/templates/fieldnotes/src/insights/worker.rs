//! Generated runtime delegation. Application backend assembly belongs in worker_hooks.
pub async fn run() -> Result<(), ::worker::native::RuntimeError> {
    crate::worker_hooks::run().await
}
