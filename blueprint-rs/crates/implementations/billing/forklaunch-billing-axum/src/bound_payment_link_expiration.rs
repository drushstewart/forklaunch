//! Explicit metadata-only mutation capability, never inferred from reads or creation.
mod contracts;
use super::{BillingBackend, bound_billing::configuration, failure, invalid};
use axum::{
    Extension, Json,
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
};
pub use contracts::bound_payment_link_expiration_contracts;
use forklaunch_axum::{ForkLaunchRouter, session::AuthenticatedSession};
use forklaunch_billing_base::services::{
    BaseBillingService, postgres::TrustedPaymentLinkExpirationPublisher,
};
use forklaunch_billing_stripe::{
    bound_payment_link_expiration::{
        BoundPaymentLinkExpirationOutcome as Outcome, BoundPaymentLinkExpirationProfile,
    },
    services::StripeBillingService,
};
use forklaunch_blueprint_core::{AuditContext, BlueprintResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    operation_id: String,
}
pub(super) struct BoundPaymentLinkExpirationV1 {
    pub(super) profile: BoundPaymentLinkExpirationProfile,
    pub(super) publisher: TrustedPaymentLinkExpirationPublisher,
}
fn id(value: &str) -> BlueprintResult<Uuid> {
    Uuid::parse_str(value)
        .ok()
        .filter(|v| !v.is_nil())
        .ok_or_else(invalid)
}
impl BillingBackend {
    pub(super) async fn with_bound_payment_link_expiration(
        mut self,
        selected: BoundPaymentLinkExpirationV1,
    ) -> BlueprintResult<Self> {
        if self.backup || self.bound_payment_link_expiration.is_some() {
            return Err(configuration());
        }
        let audit = AuditContext::actor(
            "service:payment-link-expiration-startup",
            Uuid::new_v4().to_string(),
        );
        let store = self.store.with_audit_context(audit.clone())?;
        let publisher = selected.publisher.with_audit_context(audit)?;
        let service = StripeBillingService::new(
            BaseBillingService::new(Arc::new(store)),
            self.client.as_ref().ok_or_else(configuration)?.clone(),
        );
        service
            .validate_bound_payment_link_expiration_configuration(&publisher)
            .await?;
        self.bound_payment_link_expiration = Some(Arc::new(selected));
        Ok(self)
    }
}
fn response(outcome: &Outcome, inspect: bool) -> (StatusCode, Value) {
    let (status, state, operation_id, public_id) = match outcome {
        Outcome::Completed {
            operation_id,
            public_id,
        } => (StatusCode::OK, "completed", operation_id, public_id),
        Outcome::Unresolved {
            operation_id,
            public_id,
        } => (
            if inspect {
                StatusCode::OK
            } else {
                StatusCode::ACCEPTED
            },
            "pending",
            operation_id,
            public_id,
        ),
        Outcome::RecordedEvidenceNoLongerCurrent {
            operation_id,
            public_id,
        } => (
            StatusCode::ACCEPTED,
            "recordedEvidenceNoLongerCurrent",
            operation_id,
            public_id,
        ),
    };
    (
        status,
        json!({"state":state,"operationId":operation_id.to_string(),"publicId":public_id.to_string()}),
    )
}
async fn execute(
    backend: Arc<BillingBackend>,
    session: AuthenticatedSession,
    public_id: &str,
    input: Option<Value>,
    operation: Option<&str>,
) -> BlueprintResult<(StatusCode, Value)> {
    if backend.backup {
        return Err(configuration());
    }
    let selected = backend
        .bound_payment_link_expiration
        .as_ref()
        .ok_or_else(configuration)?;
    let public_id = id(public_id)?;
    let audit = AuditContext::actor(&session.principal.subject, Uuid::new_v4().to_string());
    let store = backend.store.with_audit_context(audit.clone())?;
    let service = StripeBillingService::new(
        BaseBillingService::new(Arc::new(store)),
        backend.client.as_ref().ok_or_else(configuration)?.clone(),
    );
    if let Some(input) = input {
        let request: Request = serde_json::from_value(input).map_err(|_| invalid())?;
        let operation = id(&request.operation_id)?;
        let publisher = selected.publisher.with_audit_context(audit)?;
        Ok(response(
            &service
                .expire_bound_created_payment_link(
                    &publisher,
                    &selected.profile,
                    operation,
                    public_id,
                )
                .await?,
            false,
        ))
    } else {
        let operation = id(operation.ok_or_else(invalid)?)?;
        match service
            .inspect_bound_payment_link_expiration(&selected.profile, operation, public_id)
            .await?
        {
            Some(outcome) => Ok(response(&outcome, true)),
            None => Ok((StatusCode::NOT_FOUND, Value::Null)),
        }
    }
}
fn dispatch(result: BlueprintResult<(StatusCode, Value)>) -> Response {
    match result {
        Ok((StatusCode::NOT_FOUND, _)) => (
            StatusCode::NOT_FOUND,
            "Payment link expiration operation not found",
        )
            .into_response(),
        Ok((status, value)) => (status, Json(value)).into_response(),
        Err(error) => match error.code.as_str() {
            "billing_bound_payment_link_expiration_invalid" => {
                (StatusCode::BAD_REQUEST, "Invalid payment link expiration").into_response()
            }
            "billing_provider_binding_conflict" | "billing_bound_payment_link_read_mismatch" => (
                StatusCode::CONFLICT,
                "Payment link expiration evidence does not match",
            )
                .into_response(),
            _ => failure(&error),
        },
    }
}
pub(super) fn register(
    mut router: ForkLaunchRouter,
    backend: &Arc<BillingBackend>,
) -> ForkLaunchRouter {
    if backend.bound_payment_link_expiration.is_none() {
        return router;
    }
    for contract in bound_payment_link_expiration_contracts() {
        let backend = Arc::clone(backend);
        if contract.operation_name == "expireBoundPaymentLink" {
            router =
                router.post(
                    contract,
                    move |Extension(session): Extension<AuthenticatedSession>,
                          Path(public): Path<String>,
                          Json(input): Json<Value>| {
                        let backend = Arc::clone(&backend);
                        async move {
                            dispatch(execute(backend, session, &public, Some(input), None).await)
                        }
                    },
                );
        } else {
            router = router.get(
                contract,
                move |Extension(session): Extension<AuthenticatedSession>,
                      Path((public, operation)): Path<(String, String)>| {
                    let backend = Arc::clone(&backend);
                    async move {
                        dispatch(execute(backend, session, &public, None, Some(&operation)).await)
                    }
                },
            );
        }
    }
    router
}
