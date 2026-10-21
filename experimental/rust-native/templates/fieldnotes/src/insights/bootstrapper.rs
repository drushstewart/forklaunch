use ::worker::native::{RuntimeError, WorkerApplication};
struct Application;
impl WorkerApplication for Application {
    async fn migrate(&self) -> Result<(), RuntimeError> { crate::worker_hooks::migrate().await }
    async fn run(&self) -> Result<(), RuntimeError> { crate::worker::run().await }
}
pub async fn run() -> Result<(), RuntimeError> {
    ::worker::native::run_application(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"), &Application).await
}
