//! Private provenance survives service errors without trusting JSON metadata.
use forklaunch_blueprint_core::BlueprintError;

// This type is intentionally private: external error details and arbitrary
// diagnostic values cannot impersonate an explicitly handled API failure.
struct HandledApiError {
    // Service provenance is retained without HTTP; only the HTTP observer reads this flag.
    #[cfg_attr(not(feature = "http"), allow(dead_code))]
    internal_diagnostic: bool,
}

pub(crate) fn handled(error: BlueprintError) -> BlueprintError {
    error.with_diagnostic_context(HandledApiError {
        internal_diagnostic: false,
    })
}

#[cfg(feature = "http")]
pub(crate) fn is_handled(error: &BlueprintError) -> bool {
    error.diagnostic_context::<HandledApiError>().is_some()
}

#[cfg(feature = "http")]
pub(crate) fn database_metadata(
    error: &BlueprintError,
) -> std::collections::BTreeMap<String, String> {
    let mut fields = std::collections::BTreeMap::new();
    for field in ["code", "constraint", "table", "column"] {
        if let Some(value) = error.details.get(field).and_then(serde_json::Value::as_str)
            && !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
            && (field != "code" || value.len() == 5)
        {
            fields.insert(field.to_owned(), value.to_owned());
        }
    }
    fields
}

/// Safe internal diagnostics never invoke the HTTP API-error callback.
#[cfg(feature = "http")]
pub(crate) fn log_handled_failure(route: &'static str, error: &BlueprintError) {
    let metadata = database_metadata(error);
    tracing::error!(target: "forklaunch_iam_better_auth", route, handled = true,
        database = ?metadata, "Handled Better Auth operation failure");
}

// Trusted catch boundaries only; never inferred from public error code/details.
pub(crate) fn internal_catch(error: BlueprintError) -> BlueprintError {
    error.with_diagnostic_context(HandledApiError {
        internal_diagnostic: true,
    })
}
pub(crate) fn session_catch(error: BlueprintError) -> BlueprintError {
    if error.diagnostic_context::<HandledApiError>().is_some() {
        error
    } else {
        internal_catch(error)
    }
}
#[cfg(feature = "http")]
pub(crate) fn has_internal_diagnostic(error: &BlueprintError) -> bool {
    error
        .diagnostic_context::<HandledApiError>()
        .is_some_and(|origin| origin.internal_diagnostic)
}
