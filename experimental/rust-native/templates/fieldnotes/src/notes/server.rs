//! Shared framework HTTP/MCP security and listener lifecycle.
use crate::registrations::AppState;
use framework::native::ApplicationResult;
pub fn router(state: AppState) -> ApplicationResult<axum::Router> { crate::application::http(state)?.router() }
pub async fn serve<F>(state: AppState, listener: tokio::net::TcpListener, shutdown: F) -> ApplicationResult
where F: std::future::Future<Output = ()> + Send { crate::application::http(state)?.serve(listener, shutdown).await }
