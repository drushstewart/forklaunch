//! Native callbacks around the shared version body parser.
use crate::{ContractValidationError, ContractValidationLocation, FrameworkRequest};
use std::sync::Arc;

type Verify = dyn Fn(&FrameworkRequest, &[u8], Option<&str>) -> Result<(), String> + Send + Sync;

/// Runtime parser callbacks, separate from serializable parser options.
#[derive(Clone, Default)]
pub struct BodyParserHooks {
    verify: Option<Arc<Verify>>,
}
impl std::fmt::Debug for BodyParserHooks {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BodyParserHooks")
            .field("verify", &self.verify.is_some())
            .finish()
    }
}
impl BodyParserHooks {
    /// Inspect bounded, decompressed bytes before character decoding or parsing.
    /// Charset is absent for raw/file bodies. Errors reject the request with 403.
    #[must_use]
    pub fn with_verify(
        mut self,
        callback: impl Fn(&FrameworkRequest, &[u8], Option<&str>) -> Result<(), String>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.verify = Some(Arc::new(callback));
        self
    }
    pub(super) fn verify(
        &self,
        request: &FrameworkRequest,
        bytes: &[u8],
        charset: Option<&str>,
    ) -> Result<(), ContractValidationError> {
        if let Some(callback) = &self.verify {
            callback(request, bytes, charset).map_err(|_| {
                ContractValidationError::new(
                    ContractValidationLocation::BodyVerification,
                    "request body verification failed",
                )
            })?;
        }
        Ok(())
    }
}
