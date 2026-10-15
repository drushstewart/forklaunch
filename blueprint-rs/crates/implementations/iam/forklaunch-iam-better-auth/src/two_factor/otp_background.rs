//! Optional trusted OTP scheduling. Runtime lifetime and capacity are app-owned.
use super::{BlueprintResult, OtpDeliveryFuture, TwoFactorService};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

/// Owned, lazy delivery task with sanitized error handling. No Debug/Serialize or
/// secret inspection API. Poll/spawn it to execute; dropping it cancels pending
/// work. Unlike an eager JavaScript Promise, construction alone does not poll it.
pub struct OtpBackgroundTask(Pin<Box<dyn Future<Output = ()> + Send + 'static>>);
impl Future for OtpBackgroundTask {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.0.as_mut().poll(cx)
    }
}
impl OtpBackgroundTask {
    pub(super) fn new(future: OtpDeliveryFuture) -> Self {
        Self(Box::pin(async move {
            if future.await.is_err() {
                diagnostic(false);
            }
        }))
    }
}
/// Trusted synchronous handoff to an application-owned executor/queue. The app
/// owns capacity, scheduling, shutdown draining and cancellation. Return an error
/// if handoff fails; it is sanitized and does not roll back the persisted OTP.
/// Blocking dispatch and panics are outside this Result/cooperative contract.
pub trait OtpBackgroundDispatcher: Send + Sync {
    /// # Errors
    /// Return an error when the application rejects handoff; error details are discarded.
    fn dispatch(&self, task: OtpBackgroundTask) -> BlueprintResult<()>;
}
pub(super) fn diagnostic(dispatch: bool) {
    #[cfg(feature = "http")]
    crate::error_origin::log_handled_failure(
        "/two-factor/send-otp",
        &super::error(
            if dispatch {
                "TWO_FACTOR_DISPATCH_FAILED"
            } else {
                "TWO_FACTOR_DELIVERY_FAILED"
            },
            "OTP delivery failed",
        ),
    );
    #[cfg(not(feature = "http"))]
    let _ = dispatch;
}
impl<S> TwoFactorService<S> {
    /// Installs trusted scheduling for OTP delivery only. No task is automatically
    /// spawned, and no HTTP request can choose this policy. Existing hooks/options
    /// remain intact. Dispatcher errors leave the committed proof valid.
    #[must_use]
    pub fn with_otp_background_dispatcher(
        mut self,
        dispatcher: Arc<dyn OtpBackgroundDispatcher>,
    ) -> Self {
        self.otp_background_dispatcher = Some(dispatcher);
        self
    }
}
