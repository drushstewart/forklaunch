//! Private provenance survives service errors without trusting JSON metadata.
use forklaunch_blueprint_core::BlueprintError;

// This type is intentionally private: external error details and arbitrary
// diagnostic values cannot impersonate an explicitly handled API failure.
struct HandledApiError;

pub(crate) fn handled(error: BlueprintError) -> BlueprintError {
    error.with_diagnostic_context(HandledApiError)
}

#[cfg(feature = "http")]
pub(crate) fn is_handled(error: &BlueprintError) -> bool {
    error.diagnostic_context::<HandledApiError>().is_some()
}
