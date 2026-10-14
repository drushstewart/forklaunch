//! Owned test delivery channel, outside the production HTTP surface.
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use forklaunch_iam_better_auth::two_factor::{OtpDelivery, OtpDeliveryContext, OtpDeliveryFuture};
use std::sync::{
    Mutex,
    atomic::{AtomicU8, Ordering},
};
#[derive(Default)]
pub struct FixtureDelivery {
    code: Mutex<String>,
    pub mode: AtomicU8,
}
impl FixtureDelivery {
    pub fn last(&self) -> String {
        self.code.lock().unwrap().clone()
    }
}
impl OtpDelivery for FixtureDelivery {
    fn prepare(&self, context: OtpDeliveryContext) -> BlueprintResult<OtpDeliveryFuture> {
        assert!(context.user().get("token").is_none());
        self.code
            .lock()
            .unwrap()
            .clone_from(&context.code().to_owned());
        let mode = self.mode.load(Ordering::SeqCst);
        if mode == 1 {
            return Err(BlueprintError::new("private", "owned preparation detail"));
        }
        Ok(Box::pin(async move {
            if mode == 2 {
                Err(BlueprintError::new("private", "owned async detail"))
            } else {
                Ok(())
            }
        }))
    }
}
