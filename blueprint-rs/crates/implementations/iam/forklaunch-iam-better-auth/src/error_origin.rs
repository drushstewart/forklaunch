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
