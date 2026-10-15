//! Trusted application-owned delivery scheduling; no implicit runtime or task spawn.
use forklaunch_blueprint_core::BlueprintResult;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
/// Owned, lazy delivery work. Callback errors are handled by the owning protocol.
pub type DeliveryFuture = Pin<Box<dyn Future<Output = BlueprintResult<()>> + Send + 'static>>;
/// Opaque lazy task. No Debug/Serde, secret accessor, or cancellation guarantee for
/// external work already started by its callback. The application owns polling,
/// executor capacity, shutdown draining and dropping pending tasks.
pub struct AuthBackgroundTask(Pin<Box<dyn Future<Output = ()> + Send + 'static>>);
impl Future for AuthBackgroundTask {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.0.as_mut().poll(cx)
    }
}
/// Trusted synchronous task handoff. Rust futures are lazy, unlike JS Promises;
/// blocking code and panics are outside this Result/cooperative contract.
pub trait AuthBackgroundDispatcher: Send + Sync {
    /// # Errors
    /// Return an error if scheduling fails. The owning protocol sanitizes it;
    /// acceptance followed by an error does not cancel already accepted work.
    fn dispatch(&self, task: AuthBackgroundTask) -> BlueprintResult<()>;
}
fn diagnostic(route: &'static str, code: &'static str, message: &'static str) {
    #[cfg(feature = "http")]
    crate::error_origin::log_handled_failure(
        route,
        &forklaunch_blueprint_core::BlueprintError::new(code, message),
    );
    #[cfg(not(feature = "http"))]
    let _ = (route, code, message);
}
pub(crate) async fn run(
    future: DeliveryFuture,
    dispatcher: Option<&dyn AuthBackgroundDispatcher>,
    route: &'static str,
    delivery_code: &'static str,
    dispatch_code: &'static str,
    message: &'static str,
) {
    let task = AuthBackgroundTask(Box::pin(async move {
        if future.await.is_err() {
            diagnostic(route, delivery_code, message);
        }
    }));
    if let Some(dispatcher) = dispatcher {
        if dispatcher.dispatch(task).is_err() {
            diagnostic(route, dispatch_code, message);
        }
    } else {
        task.await;
    }
}
