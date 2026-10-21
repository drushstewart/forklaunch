//! Shared framework process lifecycle; application hooks own state and domain setup.
pub async fn run() -> framework::native::ApplicationResult {
    framework::native::run_application("notes", env!("CARGO_PKG_VERSION"), &crate::sdk::registry(), crate::application::run()).await
}
