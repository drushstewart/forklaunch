//! Private HTTP clock propagation. Direct services retain their supplied timestamp.
use chrono::{DateTime, Utc};
use std::sync::Arc;
#[derive(Clone, Default)]
pub(crate) struct TimeSource(Option<Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>>);
impl TimeSource {
    #[cfg(feature = "http")]
    pub(crate) fn http(clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>) -> Self {
        Self(Some(clock))
    }
    pub(crate) fn sample(&self, supplied: DateTime<Utc>) -> DateTime<Utc> {
        self.0.as_ref().map_or(supplied, |clock| clock())
    }
}
