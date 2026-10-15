//! Backward-compatible OTP aliases for shared application-owned scheduling.
use super::TwoFactorService;
pub use crate::background::{
    AuthBackgroundDispatcher as OtpBackgroundDispatcher, AuthBackgroundTask as OtpBackgroundTask,
};
use std::sync::Arc;
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
