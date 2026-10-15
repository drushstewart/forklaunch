//! Configured cache DTO date coercion. This never traverses opaque provider data.
use super::BillingKind;
use chrono::{DateTime, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use forklaunch_validation::typescript_coerce::{DateCoercionError, date_epoch_millis};
use serde_json::Value;

fn unsupported() -> BlueprintError {
    BlueprintError::new(
        "billing_configured_cache_date_unsupported",
        "Cached date is outside the supported native profile",
    )
}
fn invalid() -> BlueprintError {
    BlueprintError::new(
        "billing_configured_mapper_error",
        "Invalid cached response date",
    )
}
pub(super) fn ensure_kind(kind: BillingKind) -> BlueprintResult<()> {
    if matches!(
        kind,
        BillingKind::CheckoutSession | BillingKind::PaymentLink
    ) {
        Ok(())
    } else {
        Err(BlueprintError::new(
            "billing_configuration",
            "Configured cached date profile does not support this record kind",
        ))
    }
}
pub(super) fn normalize(kind: BillingKind, mut data: Value) -> BlueprintResult<Value> {
    let fields: &[&str] = match kind {
        BillingKind::CheckoutSession => &["expiresAt", "createdAt", "updatedAt"],
        BillingKind::PaymentLink => &["createdAt", "updatedAt"],
        _ => {
            return Err(BlueprintError::new(
                "billing_configuration",
                "Configured cached date profile does not support this record kind",
            ));
        }
    };
    let object = data.as_object_mut().ok_or_else(invalid)?;
    for field in fields {
        let Some(value) = object.get_mut(*field) else {
            continue;
        };
        let millis = date_epoch_millis(Some(value)).map_err(|error| match error {
            DateCoercionError::Validation(_) => invalid(),
            DateCoercionError::UnsupportedStringFormat => unsupported(),
        })?;
        // Preserve chrono's existing representable range, including signed
        // extended years. JS TimeClip is wider; do not clamp those values.
        let timestamp = DateTime::<Utc>::from_timestamp_millis(millis).ok_or_else(unsupported)?;
        *value = Value::String(timestamp.to_rfc3339_opts(SecondsFormat::Millis, true));
    }
    Ok(data)
}
