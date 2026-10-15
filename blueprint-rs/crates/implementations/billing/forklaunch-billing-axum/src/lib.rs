//! Persistent billing HTTP handlers shared with authenticated MCP dispatch.
#![forbid(unsafe_code)]
#[cfg(feature = "bound-checkout")]
mod bound_billing;
#[cfg(feature = "bound-checkout")]
mod bound_checkout;
#[cfg(feature = "bound-checkout")]
mod bound_payment_link;
#[cfg(feature = "bound-checkout")]
mod bound_payment_link_read;
#[cfg(feature = "bound-checkout")]
mod bound_plan;
#[cfg(feature = "bound-checkout")]
mod bound_plan_read;
#[cfg(feature = "bound-checkout")]
mod bound_portal;
#[cfg(feature = "bound-checkout")]
mod bound_subscription;
#[cfg(feature = "bound-checkout")]
mod bound_subscription_read;
#[cfg(feature = "bound-checkout")]
pub use bound_billing::{BoundBillingConfig, BoundBillingV1, BoundPortalConfig};
#[cfg(feature = "bound-checkout")]
pub use bound_checkout::{
    BillingIdentityProfile, BoundCheckoutConfig, BoundCheckoutV1, bound_checkout_contracts,
};
#[cfg(feature = "bound-checkout")]
pub use bound_payment_link::bound_payment_link_contracts;
#[cfg(feature = "bound-checkout")]
pub use bound_payment_link_read::bound_payment_link_read_contracts;
#[cfg(feature = "bound-checkout")]
pub use bound_plan::bound_plan_contracts;
#[cfg(feature = "bound-checkout")]
pub use bound_plan_read::bound_plan_read_contracts;
#[cfg(feature = "bound-checkout")]
pub use bound_portal::{BoundPortalV1, bound_portal_contracts};
#[cfg(feature = "bound-checkout")]
pub use bound_subscription::bound_subscription_contracts;
#[cfg(feature = "bound-checkout")]
pub use bound_subscription_read::bound_subscription_read_contracts;
mod request_dates;
mod webhook;
use axum::{
    Extension, Json,
    extract::{Path, RawQuery},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use forklaunch_axum::{
    ForkLaunchRouter, RouterConfig, module_auth::HmacJwksAuth, session::AuthenticatedSession,
};
use forklaunch_billing_base::services::{BaseBillingService, postgres::PostgresBillingStore};
use forklaunch_billing_interface::{
    module_contracts,
    services::{
        BillingPortalService, CheckoutSessionService, PaymentLinkService, PlanService,
        SubscriptionService,
    },
};
use forklaunch_billing_stripe::{client::StripeBillingClient, services::StripeBillingService};
use forklaunch_blueprint_core::{
    AuditContext, BlueprintError, BlueprintResult,
    contract::{HttpMethod, RouteContract},
};
use forklaunch_core::ServiceInfo;
pub use request_dates::BillingRequestDateProfile;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
pub struct BillingBackend {
    store: PostgresBillingStore,
    client: Option<StripeBillingClient>,
    backup: bool,
    #[cfg(feature = "bound-checkout")]
    bound_checkout: Option<Arc<BoundCheckoutV1>>,
    #[cfg(feature = "bound-checkout")]
    bound_portal: Option<Arc<BoundPortalV1>>,
    #[cfg(feature = "bound-checkout")]
    bound_plan_read:
        Option<Arc<forklaunch_billing_stripe::bound_plan_creation::BoundPlanCreationProfile>>,
    #[cfg(feature = "bound-checkout")]
    bound_subscription_read: Option<Arc<forklaunch_billing_stripe::bound_subscription_creation::BoundSubscriptionCreationProfile>>,
    #[cfg(feature="bound-checkout")]
    bound_payment_link_read: Option<Arc<bound_billing::PaymentLinkReadConfig>>,
    #[cfg(feature = "bound-checkout")]
    bound_payment_link: Option<Arc<forklaunch_billing_stripe::bound_creation::BoundPaymentLinkCreationProfile>>,
    #[cfg(feature = "bound-checkout")]
    bound_plan: Option<Arc<forklaunch_billing_stripe::bound_plan_creation::BoundPlanCreationProfile>>,
    #[cfg(feature = "bound-checkout")]
    bound_subscription: Option<Arc<bound_subscription::BoundSubscriptionV1>>,
    request_dates: BillingRequestDateProfile,
    webhook: Option<forklaunch_billing_stripe::webhook::StripeWebhookVerifier>,
    platform_key: Option<zeroize::Zeroizing<String>>,
}
impl BillingBackend {
    #[must_use]
    pub const fn base(store: PostgresBillingStore) -> Self {
        Self {
            store,
            client: None,
            backup: false,
            #[cfg(feature = "bound-checkout")]
            bound_checkout: None,
            #[cfg(feature = "bound-checkout")]
            bound_portal: None,
            #[cfg(feature = "bound-checkout")]
            bound_plan_read: None,
            #[cfg(feature = "bound-checkout")]
            bound_subscription_read: None,
            #[cfg(feature = "bound-checkout")]
            bound_payment_link_read: None,
            #[cfg(feature = "bound-checkout")]
            bound_payment_link: None,
            #[cfg(feature = "bound-checkout")]
            bound_plan: None,
            #[cfg(feature = "bound-checkout")]
            bound_subscription: None,
            request_dates: BillingRequestDateProfile::Legacy,
            webhook: None,
            platform_key: None,
        }
    }
    #[must_use]
    pub const fn stripe(store: PostgresBillingStore, client: StripeBillingClient) -> Self {
        Self {
            store,
            client: Some(client),
            backup: false,
            #[cfg(feature = "bound-checkout")]
            bound_checkout: None,
            #[cfg(feature = "bound-checkout")]
            bound_portal: None,
            #[cfg(feature = "bound-checkout")]
            bound_plan_read: None,
            #[cfg(feature = "bound-checkout")]
            bound_subscription_read: None,
            #[cfg(feature = "bound-checkout")]
            bound_payment_link_read: None,
            #[cfg(feature = "bound-checkout")]
            bound_payment_link: None,
            #[cfg(feature = "bound-checkout")]
            bound_plan: None,
            #[cfg(feature = "bound-checkout")]
            bound_subscription: None,
            request_dates: BillingRequestDateProfile::Legacy,
            webhook: None,
            platform_key: None,
        }
    }
    #[must_use]
    pub const fn with_database_backup(mut self, enabled: bool) -> Self {
        self.backup = enabled;
        self
    }
    #[must_use]
    pub const fn with_request_dates(mut self, profile: BillingRequestDateProfile) -> Self {
        self.request_dates = profile;
        self
    }
    /// # Errors
    /// Rejects invalid server-owned webhook secrets.
    pub fn with_webhook_secret(mut self, secret: String) -> BlueprintResult<Self> {
        self.webhook =
            Some(forklaunch_billing_stripe::webhook::StripeWebhookVerifier::new(secret)?);
        Ok(self)
    }
    /// # Errors
    /// Rejects empty or control-containing instance credentials.
    pub fn with_platform_event_key(mut self, secret: String) -> BlueprintResult<Self> {
        if secret.is_empty() || secret.len() > 4096 || secret.chars().any(char::is_control) {
            return Err(BlueprintError::new(
                "billing_configuration",
                "Invalid platform event key",
            ));
        }
        self.platform_key = Some(zeroize::Zeroizing::new(secret));
        Ok(self)
    }
    async fn call(
        &self,
        session: &AuthenticatedSession,
        contract: &RouteContract,
        mut input: Value,
    ) -> BlueprintResult<BillingResponse> {
        self.request_dates.normalize(contract, &mut input)?;
        validate_input(contract, &mut input)?;
        if self.client.is_some()
            && let Some(fields) = input
                .as_object_mut()
                .and_then(|fields| fields.remove("stripeFields"))
        {
            input["providerFields"] = fields;
        }
        if self.client.is_some()
            && matches!(
                contract.operation_name.as_str(),
                "createPlan" | "createSubscription"
            )
        {
            input.as_object_mut().ok_or_else(invalid)?.remove("id");
        }
        let store = self.store.with_audit_context(AuditContext::actor(
            &session.principal.subject,
            uuid::Uuid::new_v4().to_string(),
        ))?;
        let service = BaseBillingService::new(Arc::new(store))
            .with_cache_ttl(Duration::from_secs(600))?
            .with_party_types("user".into(), "organization".into())?
            .with_database_backup(self.backup);
        let mut response = match &self.client {
            Some(client) => {
                execute(
                    &StripeBillingService::new(service, client.clone()),
                    &contract.operation_name,
                    input,
                )
                .await?
            }
            None => execute(&service, &contract.operation_name, input).await?,
        };
        if self.client.is_some()
            && let BillingResponse::Json(value) = &mut response
        {
            stripe_names(value);
        }
        Ok(response)
    }
}
enum BillingResponse {
    Json(Value),
    Text(String),
}
impl IntoResponse for BillingResponse {
    fn into_response(self) -> Response {
        match self {
            Self::Json(value) => Json(value).into_response(),
            Self::Text(value) => value.into_response(),
        }
    }
}
fn stripe_names(value: &mut Value) {
    if let Some(items) = value.as_array_mut() {
        for item in items {
            stripe_names(item);
        }
    } else if let Some(object) = value.as_object_mut()
        && let Some(fields) = object.remove("providerFields")
    {
        object.insert("stripeFields".into(), fields);
    }
}
fn validate_input(contract: &RouteContract, input: &mut Value) -> BlueprintResult<()> {
    let Some(body) = &contract.request.body else {
        return Ok(());
    };
    let schema: Value =
        serde_json::from_str(body.schema.json_schema.as_deref().ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
    let object = input.as_object_mut().ok_or_else(invalid)?;
    if let Some(required) = schema["required"].as_array() {
        for field in required {
            if !object.contains_key(field.as_str().ok_or_else(invalid)?) {
                return Err(invalid());
            }
        }
    }
    let properties = schema["properties"].as_object().ok_or_else(invalid)?;
    // HTTP parsing in TS strips unknown object keys before mapper dispatch.
    object.retain(|field, _| properties.contains_key(field));
    for (field, value) in object {
        let Some(schema) = properties.get(field) else {
            continue;
        };
        if schema["type"] == "object" && !value.is_object() {
            return Err(invalid());
        }
        if schema.get("enum").is_some_and(|values| {
            values
                .as_array()
                .is_none_or(|values| !values.contains(value))
        }) {
            return Err(invalid());
        }
        if schema["format"] == "uuid"
            && value
                .as_str()
                .is_none_or(|value| uuid::Uuid::parse_str(value).is_err())
        {
            return Err(invalid());
        }
        if let Some(values) = schema["items"]["enum"].as_array()
            && value
                .as_array()
                .is_none_or(|items| items.iter().any(|item| !values.contains(item)))
        {
            return Err(invalid());
        }
    }
    Ok(())
}
fn invalid() -> BlueprintError {
    BlueprintError::new("billing_validation", "Invalid billing request")
}
fn failure(error: &BlueprintError) -> Response {
    let (status, message) = match error.code.as_str() {
        "billing_date_profile_unsupported" => (
            StatusCode::BAD_REQUEST,
            "Date is outside the configured billing date profile",
        ),
        "billing_not_found" => (StatusCode::NOT_FOUND, "Billing record not found"),
        "billing_validation"
        | "billing_invalid_webhook"
        | "billing_invalid_payment_link_options" => {
            (StatusCode::BAD_REQUEST, "Invalid billing request")
        }
        "billing_conflict" | "billing_event_conflict" => {
            (StatusCode::CONFLICT, "Billing record conflict")
        }
        "billing_webhook_reconciliation_required" => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Billing event requires reconciliation",
        ),
        "billing_webhook_busy" => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Billing event is being processed",
        ),
        "provider_request_failed" | "provider_transport_error" | "stripe_invalid_response" => {
            (StatusCode::BAD_GATEWAY, "Billing provider operation failed")
        }
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Billing operation failed",
        ),
    };
    (status, message).into_response()
}
async fn dispatch(
    backend: Arc<BillingBackend>,
    session: AuthenticatedSession,
    contract: RouteContract,
    input: Value,
) -> Response {
    match backend.call(&session, &contract, input).await {
        Ok(result) => result.into_response(),
        Err(error) => failure(&error),
    }
}
/// Builds thirty core billing operations, compliance routes and health. Each data read audits
/// the verified request actor and uses the server's configured tenant.
/// # Panics
/// Only package-owned unsupported HTTP methods can panic.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn billing_router(
    backend: BillingBackend,
    auth: HmacJwksAuth,
    service: ServiceInfo,
) -> ForkLaunchRouter {
    let registry = module_contracts::module_registry(backend.client.is_some());
    let backend = Arc::new(backend);
    let auth = Arc::new(auth);
    let config = RouterConfig::default().with_authenticator(move |request, contract| {
        let auth = Arc::clone(&auth);
        async move { auth.authenticate(request, contract).await }
    });
    let mut router = ForkLaunchRouter::new(service.clone());
    for contract in registry
        .routes()
        .iter()
        .filter(|route| route.operation_name != "getHealth")
    {
        let mut contract = contract.clone();
        backend
            .request_dates
            .project(&mut contract)
            .expect("package-owned billing request schema");
        if contract.operation_name == "handlewebhookevent" {
            let backend = Arc::clone(&backend);
            router = router.post(
                contract,
                move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
                    webhook::stripe(Arc::clone(&backend), headers, body)
                },
            );
            continue;
        }
        if contract.operation_name == "receiveplatformevent" {
            let backend = Arc::clone(&backend);
            router = router.post(
                contract,
                move |Path(feature): Path<String>,
                      headers: axum::http::HeaderMap,
                      body: axum::body::Bytes| {
                    webhook::platform(Arc::clone(&backend), feature, headers, body)
                },
            );
            continue;
        }
        if contract.operation_name == "exportuserdata" {
            router = router.get(contract, || async {
                (
                    StatusCode::NOT_FOUND,
                    "User not found or no PII data to export",
                )
            });
            continue;
        }
        if contract.operation_name == "eraseuserdata" {
            router = router.delete(contract, || async {
                (
                    StatusCode::NOT_FOUND,
                    "User not found or no PII data to erase",
                )
            });
            continue;
        }
        let handler_contract = contract.clone();
        let backend = Arc::clone(&backend);
        router = match contract.method {
            HttpMethod::Post => router.post(
                contract,
                move |Extension(session): Extension<AuthenticatedSession>,
                      Json(input): Json<Value>| {
                    dispatch(
                        Arc::clone(&backend),
                        session,
                        handler_contract.clone(),
                        input,
                    )
                },
            ),
            HttpMethod::Put if !contract.request.params.is_empty() => router.put(
                contract,
                move |Extension(session): Extension<AuthenticatedSession>,
                      Path(id): Path<String>,
                      Json(mut input): Json<Value>| {
                    let backend = Arc::clone(&backend);
                    let contract = handler_contract.clone();
                    async move {
                        let Some(object) = input.as_object_mut() else {
                            return failure(&invalid());
                        };
                        object.entry("id").or_insert(json!(id));
                        dispatch(backend, session, contract, input).await
                    }
                },
            ),
            HttpMethod::Put => router.put(
                contract,
                move |Extension(session): Extension<AuthenticatedSession>,
                      Json(input): Json<Value>| {
                    dispatch(
                        Arc::clone(&backend),
                        session,
                        handler_contract.clone(),
                        input,
                    )
                },
            ),
            HttpMethod::Get if contract.request.params.is_empty() => router.get(
                contract,
                move |Extension(session): Extension<AuthenticatedSession>,
                      RawQuery(query): RawQuery| {
                    let backend = Arc::clone(&backend);
                    let contract = handler_contract.clone();
                    async move {
                        match list_input(query.as_deref()) {
                            Ok(input) => dispatch(backend, session, contract, input).await,
                            Err(error) => failure(&error),
                        }
                    }
                },
            ),
            HttpMethod::Get => router.get(
                contract,
                move |Extension(session): Extension<AuthenticatedSession>,
                      Path(id): Path<String>| {
                    dispatch(
                        Arc::clone(&backend),
                        session,
                        handler_contract.clone(),
                        json!({"id":id}),
                    )
                },
            ),
            HttpMethod::Delete => router.delete(
                contract,
                move |Extension(session): Extension<AuthenticatedSession>,
                      Path(id): Path<String>| {
                    dispatch(
                        Arc::clone(&backend),
                        session,
                        handler_contract.clone(),
                        json!({"id":id}),
                    )
                },
            ),
            _ => panic!("Unsupported package-owned billing HTTP method"),
        };
    }
    #[cfg(feature = "bound-checkout")]
    let router = bound_checkout::register(router, &backend);
    #[cfg(feature = "bound-checkout")]
    let router = bound_portal::register(router, &backend);
    #[cfg(feature = "bound-checkout")]
    let router = bound_plan_read::register(router, &backend);
    #[cfg(feature = "bound-checkout")]
    let router = bound_subscription_read::register(router, &backend);
    #[cfg(feature = "bound-checkout")]
    let router = bound_payment_link_read::register(router, &backend);
    #[cfg(feature = "bound-checkout")]
    let router = bound_payment_link::register(router, &backend);
    #[cfg(feature = "bound-checkout")]
    let router = bound_plan::register(router, &backend);
    #[cfg(feature = "bound-checkout")]
    let router = bound_subscription::register(router, &backend);
    router
        .get(module_contracts::health_contract(), move || {
            let name = service.name.clone();
            async move { Json(json!({"service":name,"status":"ok"})) }
        })
        .with_config(config)
}
fn list_input(query: Option<&str>) -> BlueprintResult<Value> {
    let mut input = json!({});
    for (key, value) in form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        if key == "ids" {
            if input.get("ids").is_some() {
                return Err(invalid());
            }
            let ids: Vec<String> = serde_json::from_str(&value).map_err(|_| invalid())?;
            input["ids"] = json!(ids);
        }
    }
    Ok(input)
}
#[allow(clippy::too_many_lines)]
async fn execute<
    S: PlanService
        + SubscriptionService
        + CheckoutSessionService
        + PaymentLinkService
        + BillingPortalService,
>(
    service: &S,
    operation: &str,
    input: Value,
) -> BlueprintResult<BillingResponse> {
    let id = input["id"].as_str().unwrap_or("");
    let ids: Option<Vec<String>> = input
        .get("ids")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()
        .map_err(|_| invalid())?;
    macro_rules! request {
        ($method:ident) => {
            serde_json::to_value(
                service
                    .$method(serde_json::from_value(input.clone()).map_err(|_| invalid())?)
                    .await?,
            )
            .map(BillingResponse::Json)
            .map_err(|_| invalid())
        };
    }
    macro_rules! get {
        ($method:ident) => {
            serde_json::to_value(service.$method(id).await?)
                .map(BillingResponse::Json)
                .map_err(|_| invalid())
        };
    }
    macro_rules! list {
        ($method:ident) => {
            serde_json::to_value(service.$method(ids.as_deref()).await?)
                .map(BillingResponse::Json)
                .map_err(|_| invalid())
        };
    }
    match operation {
        "createPlan" => request!(create_plan),
        "getPlan" => get!(get_plan),
        "updatePlan" => request!(update_plan),
        "deletePlan" => {
            service.delete_plan(id).await?;
            Ok(BillingResponse::Text(format!("Deleted plan {id}")))
        }
        "listPlans" => list!(list_plans),
        "createSubscription" => request!(create_subscription),
        "getSubscription" => get!(get_subscription),
        "updateSubscription" => request!(update_subscription),
        "deleteSubscription" => {
            service.delete_subscription(id).await?;
            Ok(BillingResponse::Text(format!("Deleted subscription {id}")))
        }
        "listSubscriptions" => list!(list_subscriptions),
        "getUserSubscription" => get!(get_user_subscription),
        "getOrganizationSubscription" => get!(get_organization_subscription),
        "cancelSubscription" => {
            service.cancel_subscription(id).await?;
            Ok(BillingResponse::Text(format!(
                "Cancelled subscription {id}"
            )))
        }
        "resumeSubscription" => {
            service.resume_subscription(id).await?;
            Ok(BillingResponse::Text(format!("Resumed subscription {id}")))
        }
        "createCheckoutSession" => request!(create_checkout_session),
        "getCheckoutSession" => get!(get_checkout_session),
        "expireCheckoutSession" => {
            service.expire_checkout_session(id).await?;
            Ok(BillingResponse::Text(format!(
                "Expired checkout session {id}"
            )))
        }
        "handleCheckoutSuccess" => {
            service.handle_checkout_success(id).await?;
            Ok(BillingResponse::Text(format!(
                "Handled checkout success for session {id}"
            )))
        }
        "handleCheckoutFailure" => {
            service.handle_checkout_failure(id).await?;
            Ok(BillingResponse::Text(format!(
                "Handled checkout failure for session {id}"
            )))
        }
        "createPaymentLink" => request!(create_payment_link),
        "getPaymentLink" => get!(get_payment_link),
        "updatePaymentLink" => request!(update_payment_link),
        "expirePaymentLink" => {
            service.expire_payment_link(id).await?;
            Ok(BillingResponse::Text(format!("Expired payment link {id}")))
        }
        "handlePaymentSuccess" => {
            service.handle_payment_success(id).await?;
            Ok(BillingResponse::Text(format!(
                "Handled payment success for {id}"
            )))
        }
        "handlePaymentFailure" => {
            service.handle_payment_failure(id).await?;
            Ok(BillingResponse::Text(format!(
                "Handled payment failure for {id}"
            )))
        }
        "listPaymentLinks" => list!(list_payment_links),
        "createBillingPortalSession" => request!(create_billing_portal_session),
        "getBillingPortalSession" => get!(get_billing_portal_session),
        "updateBillingPortalSession" => request!(update_billing_portal_session),
        "expireBillingPortalSession" => {
            service.expire_billing_portal_session(id).await?;
            Ok(BillingResponse::Text(format!(
                "Expired billing portal session {id}"
            )))
        }
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod failure_tests {
    use super::*;

    #[tokio::test]
    async fn invalid_payment_link_options_are_sanitized_client_errors() {
        let response = failure(&BlueprintError::new(
            "billing_invalid_payment_link_options",
            "secret provider payload must not escape",
        ));
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), b"Invalid billing request");
        assert_eq!(
            failure(&BlueprintError::new("billing_unknown", "private")).status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
