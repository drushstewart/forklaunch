//! Explicit provenance-aware read capability; grants no creation/publication API.
use super::{BillingBackend, bound_billing::configuration, failure, invalid, stripe_names};
use axum::{
    Extension, Json,
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use forklaunch_axum::{ForkLaunchRouter, session::AuthenticatedSession};
use forklaunch_billing_base::services::BaseBillingService;
use forklaunch_billing_stripe::{
    bound_creation::BoundPaymentLinkCreationProfile, services::StripeBillingService,
};
use forklaunch_blueprint_core::{
    AuditContext, BlueprintResult,
    contract::{
        AccessLevel, AuthConfig, BodyContract, BodyKind, FieldContract, HmacAuth, RouteContract,
        SchemaRef,
    },
};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

/// One explicit read contract shared by HTTP, MCP and generated client artifacts.
/// # Panics
/// Only malformed package-owned Payment link response metadata can panic.
#[must_use]
pub fn bound_payment_link_read_contracts() -> Vec<RouteContract> {
    let registry = forklaunch_billing_interface::module_contracts::module_registry(true);
    let mut body = registry
        .routes()
        .iter()
        .find(|r| r.operation_name == "getPaymentLink")
        .unwrap()
        .responses
        .iter()
        .find(|r| r.status_code == 200)
        .unwrap()
        .body
        .clone()
        .unwrap();
    let mut schema: Value =
        serde_json::from_str(body.schema.json_schema.as_ref().unwrap()).unwrap();
    schema["properties"]["id"]["format"] = json!("uuid");
    body.schema.json_schema = Some(schema.to_string());
    body.schema.name = "BoundCreatedPaymentLink".into();
    let mut parameter = FieldContract::required("publicId", "String");
    parameter.schema.json_schema = Some(json!({"type":"string","format":"uuid"}).to_string());
    let mut route=RouteContract::get("Get Bound Created Payment Link", "Actor-owned completed direct PaymentLink creation; verifies current account/mode, sealed original price profile and local record/binding. Refreshes PaymentLink provider fields only; local amount/status remain unchanged, including when current provider active is false. No payment or entitlement evidence.", "/bound-payment-links/:publicId")
        .with_param(parameter).with_access(AccessLevel::Internal)
        .with_auth(AuthConfig::Hmac(HmacAuth::new().with_header_name("authorization")))
        .with_response(200,Some(body));
    for status in [400, 409, 500, 502, 503] {
        route = route.with_response(
            status,
            Some(BodyContract {
                kind: BodyKind::Text,
                schema: SchemaRef::named("String"),
                content_type: Some("text/plain".into()),
            }),
        );
    }
    vec![route]
}
impl BillingBackend {
    pub(super) async fn with_bound_payment_link_read(
        mut self,
        profile: BoundPaymentLinkCreationProfile,
    ) -> BlueprintResult<Self> {
        if self.backup || self.bound_payment_link_read.is_some() {
            return Err(configuration());
        }
        let client = self.client.as_ref().ok_or_else(configuration)?;
        let service = StripeBillingService::new(
            BaseBillingService::new(Arc::new(self.store.clone())),
            client.clone(),
        );
        service
            .validate_bound_payment_link_read_configuration()
            .await?;
        self.bound_payment_link_read = Some(Arc::new(profile));
        Ok(self)
    }
}
async fn execute(
    backend: Arc<BillingBackend>,
    session: AuthenticatedSession,
    id: &str,
) -> BlueprintResult<Value> {
    if backend.backup {
        return Err(configuration());
    }
    let profile = backend
        .bound_payment_link_read
        .as_ref()
        .ok_or_else(configuration)?;
    let id = Uuid::parse_str(id).map_err(|_| invalid())?;
    if id.is_nil() {
        return Err(invalid());
    }
    let store = backend.store.with_audit_context(AuditContext::actor(
        &session.principal.subject,
        Uuid::new_v4().to_string(),
    ))?;
    let service = StripeBillingService::new(
        BaseBillingService::new(Arc::new(store)),
        backend.client.as_ref().ok_or_else(configuration)?.clone(),
    );
    let mut value =
        serde_json::to_value(service.get_bound_created_payment_link(profile, id).await?)
            .map_err(|_| invalid())?;
    stripe_names(&mut value);
    Ok(value)
}
async fn dispatch(
    backend: Arc<BillingBackend>,
    session: AuthenticatedSession,
    id: &str,
) -> Response {
    match execute(backend, session, id).await {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(error) => match error.code.as_str() {
            "billing_bound_payment_link_read_mismatch"
            | "billing_provider_binding_conflict"
            | "billing_provider_context_mismatch"
            | "billing_bound_creation_invalid" => (
                StatusCode::CONFLICT,
                "Bound payment link evidence does not match",
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
    if backend.bound_payment_link_read.is_none() {
        return router;
    }
    for contract in bound_payment_link_read_contracts() {
        let backend = Arc::clone(backend);
        router = router.get(
            contract,
            move |Extension(session): Extension<AuthenticatedSession>, Path(id): Path<String>| {
                let backend = Arc::clone(&backend);
                async move { dispatch(backend, session, &id).await }
            },
        );
    }
    router
}
