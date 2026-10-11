//! Tenant-bound ecommerce HTTP handlers shared with MCP dispatch.
#![forbid(unsafe_code)]
mod checkout;
mod query;
mod webhook;
use axum::{
    Extension, Json,
    extract::{Path, RawQuery},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use checkout::checkout;
use forklaunch_axum::{
    ForkLaunchRouter, RouterConfig, module_auth::HmacJwksAuth, session::AuthenticatedSession,
};
use forklaunch_blueprint_core::{AuditContext, BlueprintError, BlueprintResult};
use forklaunch_core::ServiceInfo;
use forklaunch_ecommerce_base::{
    BaseEcommerceService, CartCache, postgres::PostgresEcommerceStore,
};
use forklaunch_ecommerce_interface::{
    AddCartItemDto, AdjustStockDto, CartService, CreateCartDto, CreateOrderDto, CreatePaymentDto,
    CreateProductDto, CreateVariantDto, InventoryService, OrderService, PaymentService,
    ProductService, RemoveCartItemDto, StockCheckDto, TransitionOrderDto, UpdateProductDto,
    UpdateVariantDto, VariantService, contracts,
    module::{CreateProviderPayment, CreatedPayment, OrderTransition, PaymentProvider},
};
use forklaunch_ecommerce_paypal::{PaypalClient, PaypalPaymentService};
use forklaunch_ecommerce_stripe::{StripeClient, StripeConnect, StripePaymentService};
use forklaunch_worker_database::postgres::PostgresWorkerQueue;
use std::sync::Arc;
use webhook::{handle_paypal_webhook, handle_stripe_webhook};

pub struct EcommerceBackend {
    store: PostgresEcommerceStore,
    queue: PostgresWorkerQueue,
    stripe: StripeClient,
    paypal: Option<PaypalClient>,
    connect: Option<StripeConnect>,
    cart_cache: Option<Arc<dyn CartCache>>,
    orchestration_slots: tokio::sync::Semaphore,
    stripe_webhook: Option<forklaunch_ecommerce_stripe::webhook::StripeWebhookVerifier>,
    paypal_webhook_id: Option<String>,
}
impl EcommerceBackend {
    #[must_use]
    pub const fn new(
        store: PostgresEcommerceStore,
        queue: PostgresWorkerQueue,
        stripe: StripeClient,
    ) -> Self {
        Self {
            store,
            queue,
            stripe,
            paypal: None,
            connect: None,
            cart_cache: None,
            orchestration_slots: tokio::sync::Semaphore::const_new(4),
            stripe_webhook: None,
            paypal_webhook_id: None,
        }
    }
    #[must_use]
    pub fn with_paypal(mut self, paypal: PaypalClient) -> Self {
        self.paypal = Some(paypal);
        self
    }
    #[must_use]
    pub fn with_connect(mut self, connect: StripeConnect) -> Self {
        self.connect = Some(connect);
        self
    }
    #[must_use]
    pub fn with_stripe_webhook(
        mut self,
        verifier: forklaunch_ecommerce_stripe::webhook::StripeWebhookVerifier,
    ) -> Self {
        self.stripe_webhook = Some(verifier);
        self
    }
    /// # Errors
    /// Rejects empty/control-containing provider configuration.
    pub fn with_paypal_webhook_id(mut self, id: String) -> BlueprintResult<Self> {
        if id.is_empty() || id.len() > 1024 || id.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(BlueprintError::new(
                "provider_invalid_config",
                "Invalid PayPal webhook identifier",
            ));
        }
        self.paypal_webhook_id = Some(id);
        Ok(self)
    }
    #[must_use]
    pub fn with_cart_cache(mut self, cache: Arc<dyn CartCache>) -> Self {
        self.cart_cache = Some(cache);
        self
    }
    fn service(
        &self,
        session: &AuthenticatedSession,
    ) -> BlueprintResult<BaseEcommerceService<PostgresEcommerceStore>> {
        self.audited_service(AuditContext::actor(
            &session.principal.subject,
            uuid::Uuid::new_v4().to_string(),
        ))
    }
    fn audited_service(
        &self,
        audit: AuditContext,
    ) -> BlueprintResult<BaseEcommerceService<PostgresEcommerceStore>> {
        let store = self.store.with_audit_context(audit)?;
        let mut service = BaseEcommerceService::new(Arc::new(store));
        if let Some(cache) = &self.cart_cache {
            service = service.with_cart_cache(Arc::clone(cache));
        }
        Ok(service)
    }
    async fn create_payment(
        &self,
        session: &AuthenticatedSession,
        input: CreateProviderPayment,
    ) -> BlueprintResult<CreatedPayment> {
        let _guard = self
            .store
            .lock_operation("payment-order", &input.order_id)
            .await?;
        let service = self.service(session)?;
        if service
            .pending_payment_for_order(&input.order_id)
            .await?
            .is_some()
        {
            return Err(BlueprintError::new(
                "ecommerce_conflict",
                "Order already has a pending provider payment",
            ));
        }
        let request = CreatePaymentDto {
            id: None,
            order_id: input.order_id,
            amount_cents: input.amount_cents,
            currency: input.currency,
        };
        if input.provider == Some(PaymentProvider::Paypal) {
            let client = self.paypal.clone().ok_or_else(|| {
                BlueprintError::new(
                    "ecommerce_provider_unavailable",
                    "Payment provider is not configured",
                )
            })?;
            let payment = PaypalPaymentService::new(service, client)
                .create_payment(request)
                .await?;
            Ok(CreatedPayment {
                payment,
                client_secret: None,
            })
        } else {
            let mut provider = StripePaymentService::new(service, self.stripe.clone());
            if let Some(connect) = &self.connect {
                provider = provider.with_connect(connect.clone());
            }
            let created = provider.create_payment(request).await?;
            Ok(CreatedPayment {
                payment: created.payment,
                client_secret: created.client_secret,
            })
        }
    }
}
fn failure(error: &BlueprintError) -> Response {
    let status = match error.code.as_str() {
        "ecommerce_not_found" => StatusCode::NOT_FOUND,
        "ecommerce_conflict" | "ecommerce_event_conflict" => StatusCode::CONFLICT,
        "ecommerce_validation" | "ecommerce_illegal_transition" | "ecommerce_invalid_query" => {
            StatusCode::BAD_REQUEST
        }
        "ecommerce_provider_unavailable"
        | "provider_request_failed"
        | "provider_transport_error"
        | "stripe_invalid_response"
        | "paypal_invalid_response" => StatusCode::BAD_GATEWAY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, "Ecommerce operation failed").into_response()
}
fn response<T: serde::Serialize>(result: BlueprintResult<T>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => failure(&error),
    }
}
fn text_response(result: BlueprintResult<()>, message: &'static str) -> Response {
    match result {
        Ok(()) => message.into_response(),
        Err(error) => failure(&error),
    }
}
macro_rules! service {
    ($backend:expr,$session:expr) => {
        match $backend.service(&$session) {
            Ok(service) => service,
            Err(error) => return failure(&error),
        }
    };
}

/// Mounts all core catalog/cart/order/payment routes. Provider configuration,
/// queue tenant and encryption keys come from trusted application configuration.
/// # Panics
/// Panics if package-owned operation names and handler registrations disagree.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn ecommerce_router(
    backend: EcommerceBackend,
    auth: HmacJwksAuth,
    service: ServiceInfo,
) -> ForkLaunchRouter {
    let registry = contracts::module_registry();
    let route = |operation: &str| {
        registry
            .routes()
            .iter()
            .find(|route| route.operation_name == operation)
            .expect("registered ecommerce route")
            .clone()
    };
    let auth = Arc::new(auth);
    let config = RouterConfig::default().with_authenticator(move |request, contract| {
        let auth = Arc::clone(&auth);
        async move { auth.authenticate(request, contract).await }
    });
    let name = service.name.clone();
    let backend = Arc::new(backend);
    macro_rules! handler {
        ($name:ident,$($argument:ident:$kind:ty),+) => {{
            let backend=Arc::clone(&backend);
            move |session:Extension<AuthenticatedSession>,$($argument:$kind),+| {
                let backend=Arc::clone(&backend);
                async move {$name(Extension(backend),session,$($argument),+).await}
            }
        }};
    }

    macro_rules! public_handler {
        ($name:ident) => {{
            let backend = Arc::clone(&backend);
            move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
                let backend = Arc::clone(&backend);
                async move { $name(Extension(backend), headers, body).await }
            }
        }};
    }
    ForkLaunchRouter::new(service)
        .post(route("handleStripeWebhook"),public_handler!(handle_stripe_webhook))
        .post(route("handlePaypalWebhook"),public_handler!(handle_paypal_webhook))
        .post(route("checkout"),handler!(checkout,input:CheckedJson<forklaunch_ecommerce_interface::module::Checkout>))
        .post(
            route("importCatalog"),
            handler!(import_catalog, input:CheckedJson<forklaunch_ecommerce_interface::import::CatalogImport>),
        )
        .post(
            route("createProduct"),
            handler!(create_product, input:CheckedJson<CreateProductDto>),
        )
        .get(
            route("listProducts"),
            handler!(list_products, input:RawQuery),
        )
        .get(route("listCatalog"), handler!(list_catalog, input:RawQuery))
        .get(
            route("getProductByHandle"),
            handler!(get_product_by_handle, input:Path<String>),
        )
        .get(
            route("getProduct"),
            handler!(get_product, input:Path<String>),
        )
        .put(
            route("updateProduct"),
            handler!(update_product, input:CheckedJson<UpdateProductDto>),
        )
        .delete(
            route("deleteProduct"),
            handler!(delete_product, input:Path<String>),
        )
        .post(
            route("createVariant"),
            handler!(create_variant, input:CheckedJson<CreateVariantDto>),
        )
        .get(
            route("listVariants"),
            handler!(list_variants, input:RawQuery),
        )
        .get(
            route("listVariantsByProduct"),
            handler!(list_variants_by_product, input:Path<String>),
        )
        .get(
            route("getVariant"),
            handler!(get_variant, input:Path<String>),
        )
        .put(
            route("updateVariant"),
            handler!(update_variant, input:CheckedJson<UpdateVariantDto>),
        )
        .delete(
            route("deleteVariant"),
            handler!(delete_variant, input:Path<String>),
        )
        .get(
            route("getInventory"),
            handler!(get_inventory, input:Path<String>),
        )
        .put(
            route("adjustStock"),
            handler!(adjust_stock, input:CheckedJson<AdjustStockDto>),
        )
        .post(
            route("checkStock"),
            handler!(check_stock, input:CheckedJson<StockCheckDto>),
        )
        .post(
            route("createCart"),
            handler!(create_cart, input:CheckedJson<CreateCartDto>),
        )
        .get(route("getCart"), handler!(get_cart, input:Path<String>))
        .post(
            route("addCartItem"),
            handler!(add_cart_item, input:CheckedJson<AddCartItemDto>),
        )
        .delete(
            route("removeCartItem"),
            handler!(remove_cart_item, input:Path<(String,String)>),
        )
        .delete(route("clearCart"), handler!(clear_cart, input:Path<String>))
        .post(
            route("createOrder"),
            handler!(create_order, input:CheckedJson<CreateOrderDto>),
        )
        .get(route("listOrders"), handler!(list_orders, input:RawQuery))
        .get(route("getOrder"), handler!(get_order, input:Path<String>))
        .put(
            route("transitionOrder"),
            handler!(transition_order, id:Path<String>, input:CheckedJson<OrderTransition>),
        )
        .post(
            route("createPayment"),
            handler!(create_payment, input:CheckedJson<CreateProviderPayment>),
        )
        .get(
            route("getPayment"),
            handler!(get_payment, input:Path<String>),
        )
        .get(health_contract(), move || {
            let service = name.clone();
            async move { Json(serde_json::json!({"service":service,"status":"ok"})) }
        })
        .with_config(config)
}
#[must_use]
pub fn health_contract() -> forklaunch_blueprint_core::contract::RouteContract {
    contracts::health_contract()
}
struct CheckedJson<T>(Json<T>);
impl<S, T> axum::extract::FromRequest<S> for CheckedJson<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = Response;
    async fn from_request(
        request: axum::extract::Request,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(Self)
            .map_err(|_| {
                (StatusCode::BAD_REQUEST, "Invalid ecommerce request body").into_response()
            })
    }
}

async fn create_product(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    CheckedJson(Json(input)): CheckedJson<CreateProductDto>,
) -> Response {
    response(service!(backend, session).create_product(input).await)
}

async fn update_product(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    CheckedJson(Json(input)): CheckedJson<UpdateProductDto>,
) -> Response {
    response(service!(backend, session).update_product(input).await)
}

async fn create_variant(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    CheckedJson(Json(input)): CheckedJson<CreateVariantDto>,
) -> Response {
    response(service!(backend, session).create_variant(input).await)
}

async fn update_variant(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    CheckedJson(Json(input)): CheckedJson<UpdateVariantDto>,
) -> Response {
    response(service!(backend, session).update_variant(input).await)
}

async fn adjust_stock(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    CheckedJson(Json(input)): CheckedJson<AdjustStockDto>,
) -> Response {
    response(service!(backend, session).adjust_stock(input).await)
}

async fn check_stock(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    CheckedJson(Json(input)): CheckedJson<StockCheckDto>,
) -> Response {
    response(service!(backend, session).check_stock(input).await)
}

async fn create_cart(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    CheckedJson(Json(input)): CheckedJson<CreateCartDto>,
) -> Response {
    response(service!(backend, session).create_cart(input).await)
}

async fn add_cart_item(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    CheckedJson(Json(input)): CheckedJson<AddCartItemDto>,
) -> Response {
    response(service!(backend, session).add_item(input).await)
}

async fn create_order(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    CheckedJson(Json(input)): CheckedJson<CreateOrderDto>,
) -> Response {
    response(service!(backend, session).create_order(input).await)
}

async fn get_product(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
) -> Response {
    response(service!(backend, session).get_product(&id).await)
}

async fn get_product_by_handle(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
) -> Response {
    response(service!(backend, session).get_product_by_handle(&id).await)
}

async fn get_variant(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
) -> Response {
    response(service!(backend, session).get_variant(&id).await)
}

async fn list_variants_by_product(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
) -> Response {
    response(
        service!(backend, session)
            .list_variants_by_product(&id)
            .await,
    )
}

async fn get_inventory(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
) -> Response {
    response(service!(backend, session).get_inventory(&id).await)
}

async fn get_cart(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
) -> Response {
    response(service!(backend, session).get_cart(&id).await)
}

async fn get_order(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
) -> Response {
    response(service!(backend, session).get_order(&id).await)
}

async fn get_payment(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
) -> Response {
    response(service!(backend, session).get_payment(&id).await)
}

async fn delete_product(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
) -> Response {
    text_response(
        service!(backend, session).delete_product(&id).await,
        "Deleted product",
    )
}

async fn delete_variant(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
) -> Response {
    text_response(
        service!(backend, session).delete_variant(&id).await,
        "Deleted variant",
    )
}

async fn clear_cart(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
) -> Response {
    text_response(
        service!(backend, session).clear_cart(&id).await,
        "Cleared cart",
    )
}

async fn remove_cart_item(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    Path((cart_id, variant_id)): Path<(String, String)>,
) -> Response {
    response(
        service!(backend, session)
            .remove_item(RemoveCartItemDto {
                cart_id,
                variant_id,
            })
            .await,
    )
}
async fn create_payment(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    CheckedJson(Json(input)): CheckedJson<CreateProviderPayment>,
) -> Response {
    response(backend.create_payment(&session, input).await)
}
async fn transition_order(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    Path(id): Path<String>,
    CheckedJson(Json(input)): CheckedJson<OrderTransition>,
) -> Response {
    let service = service!(backend, session);
    response(
        service
            .store()
            .transition_order_with_event(
                TransitionOrderDto { id, to: input.to },
                &backend.queue,
                "order-events",
            )
            .await,
    )
}
async fn list_products(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    RawQuery(query): RawQuery,
) -> Response {
    match query::search(query.as_deref()) {
        Ok(input) => response(service!(backend, session).search_products(input).await),
        Err(error) => failure(&error),
    }
}
async fn list_catalog(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    RawQuery(query): RawQuery,
) -> Response {
    match query::catalog(query.as_deref()) {
        Ok(input) => response(service!(backend, session).list_catalog(input).await),
        Err(error) => failure(&error),
    }
}
async fn list_variants(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    RawQuery(query): RawQuery,
) -> Response {
    match query::ids(query.as_deref()) {
        Ok(ids) => response(service!(backend, session).list_variants(ids).await),
        Err(error) => failure(&error),
    }
}
async fn list_orders(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    RawQuery(query): RawQuery,
) -> Response {
    match query::ids(query.as_deref()) {
        Ok(ids) => response(service!(backend, session).list_orders(ids).await),
        Err(error) => failure(&error),
    }
}

async fn import_catalog(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    CheckedJson(Json(input)): CheckedJson<forklaunch_ecommerce_interface::import::CatalogImport>,
) -> Response {
    response(service!(backend, session).import_catalog(input).await)
}
