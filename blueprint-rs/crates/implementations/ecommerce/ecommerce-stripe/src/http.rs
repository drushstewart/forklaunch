//! Tenant-bound ecommerce HTTP handlers shared with MCP dispatch.
#![forbid(unsafe_code)]
mod checkout;
mod query;
mod webhook;
use crate::{StripeClient, StripeConnect, StripePaymentService};
use axum::{
    Extension, Json,
    extract::{Path, RawQuery},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{MethodRouter, any},
};
use checkout::checkout;
use forklaunch_axum::{
    ForkLaunchRouter, RouterConfig, module_auth::HmacJwksAuth, session::AuthenticatedSession,
};
use forklaunch_core::ServiceInfo;
use forklaunch_core::domain::AuditContext;
use forklaunch_core::domain::BlueprintError;
use forklaunch_core::domain::BlueprintResult;
use forklaunch_ecommerce_base::{
    BaseEcommerceService, CartCache, postgres::PostgresEcommerceStore,
};
use forklaunch_ecommerce_interface::{
    AddCartItemDto, AdjustStockDto, CartService, CreateCartDto, CreateInventoryDto, CreateOrderDto,
    CreatePaymentDto, CreateProductDto, CreateVariantDto, InventoryService, OrderService,
    PaymentService, ProductService, RemoveCartItemDto, StockCheckDto, TransitionOrderDto,
    UpdateProductDto, UpdateVariantDto, VariantService, contracts,
    module::{CreateProviderPayment, CreatedPayment, OrderTransition, PaymentProvider},
};
use forklaunch_ecommerce_paypal::{PaypalClient, PaypalPaymentService};
use forklaunch_worker_database::postgres::PostgresWorkerQueue;
use std::{collections::HashMap, sync::Arc};
use webhook::{handle_paypal_webhook, handle_stripe_webhook};

pub struct EcommerceBackend {
    store: PostgresEcommerceStore,
    queue: PostgresWorkerQueue,
    stripe: StripeClient,
    paypal: Option<PaypalClient>,
    connect: Option<StripeConnect>,
    cart_cache: Option<Arc<dyn CartCache>>,
    orchestration_slots: tokio::sync::Semaphore,
    stripe_webhook: Option<crate::webhook::StripeWebhookVerifier>,
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
    pub fn with_stripe_webhook(mut self, verifier: crate::webhook::StripeWebhookVerifier) -> Self {
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

/// One operation's request handler. Its contract supplies the method and path.
pub type Handler = MethodRouter;

/// Operations in their mounted order.
const OPERATIONS: [&str; 33] = [
    "handleStripeWebhook",
    "handlePaypalWebhook",
    "checkout",
    "importCatalog",
    "createProduct",
    "listProducts",
    "listCatalog",
    "getProductByHandle",
    "getProduct",
    "updateProduct",
    "deleteProduct",
    "createVariant",
    "listVariants",
    "listVariantsByProduct",
    "getVariant",
    "updateVariant",
    "deleteVariant",
    "createInventory",
    "getInventory",
    "adjustStock",
    "checkStock",
    "createCart",
    "getCart",
    "addCartItem",
    "removeCartItem",
    "clearCart",
    "createOrder",
    "listOrders",
    "getOrder",
    "transitionOrder",
    "createPayment",
    "getPayment",
    "getHealth",
];

/// Catalog, cart, order, payment, import, checkout, provider callback and
/// health handlers. Provider configuration, queue tenant and encryption keys
/// come from trusted application configuration.
#[derive(Clone)]
pub struct EcommerceHttp {
    backend: Arc<EcommerceBackend>,
    auth: Arc<HmacJwksAuth>,
    service: ServiceInfo,
}

impl EcommerceHttp {
    #[must_use]
    pub fn new(backend: EcommerceBackend, auth: HmacJwksAuth, service: ServiceInfo) -> Self {
        Self {
            backend: Arc::new(backend),
            auth: Arc::new(auth),
            service,
        }
    }

    /// The handler for one ecommerce operation.
    ///
    /// # Panics
    /// Panics if `operation` is not an ecommerce operation.
    #[allow(clippy::too_many_lines)]
    pub fn handler(&self, operation: &str) -> Handler {
        let backend = Arc::clone(&self.backend);
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
        match operation {
            "handleStripeWebhook" => any(public_handler!(handle_stripe_webhook)),
            "handlePaypalWebhook" => any(public_handler!(handle_paypal_webhook)),
            "checkout" => any(
                handler!(checkout,input:CheckedJson<forklaunch_ecommerce_interface::module::Checkout>),
            ),
            "importCatalog" => any(
                handler!(import_catalog, input:CheckedJson<forklaunch_ecommerce_interface::import::CatalogImport>),
            ),
            "createProduct" => any(handler!(create_product, input:CheckedJson<CreateProductDto>)),
            "listProducts" => any(handler!(list_products, input:RawQuery)),
            "listCatalog" => any(handler!(list_catalog, input:RawQuery)),
            "getProductByHandle" => any(handler!(get_product_by_handle, input:Path<String>)),
            "getProduct" => any(handler!(get_product, input:Path<String>)),
            "updateProduct" => any(handler!(update_product, input:CheckedJson<UpdateProductDto>)),
            "deleteProduct" => any(handler!(delete_product, input:Path<String>)),
            "createVariant" => any(handler!(create_variant, input:CheckedJson<CreateVariantDto>)),
            "listVariants" => any(handler!(list_variants, input:RawQuery)),
            "listVariantsByProduct" => any(handler!(list_variants_by_product, input:Path<String>)),
            "getVariant" => any(handler!(get_variant, input:Path<String>)),
            "updateVariant" => any(handler!(update_variant, input:CheckedJson<UpdateVariantDto>)),
            "deleteVariant" => any(handler!(delete_variant, input:Path<String>)),
            "createInventory" => {
                any(handler!(create_inventory, input:CheckedJson<CreateInventoryDto>))
            }
            "getInventory" => any(handler!(get_inventory, input:Path<String>)),
            "adjustStock" => any(handler!(adjust_stock, input:CheckedJson<AdjustStockDto>)),
            "checkStock" => any(handler!(check_stock, input:CheckedJson<StockCheckDto>)),
            "createCart" => any(handler!(create_cart, input:CheckedJson<CreateCartDto>)),
            "getCart" => any(handler!(get_cart, input:Path<String>)),
            "addCartItem" => any(handler!(add_cart_item, input:CheckedJson<AddCartItemDto>)),
            "removeCartItem" => any(handler!(remove_cart_item, input:Path<(String,String)>)),
            "clearCart" => any(handler!(clear_cart, input:Path<String>)),
            "createOrder" => any(handler!(create_order, input:CheckedJson<CreateOrderDto>)),
            "listOrders" => any(handler!(list_orders, input:RawQuery)),
            "getOrder" => any(handler!(get_order, input:Path<String>)),
            "transitionOrder" => {
                any(handler!(transition_order, id:Path<String>, input:CheckedJson<OrderTransition>))
            }
            "createPayment" => {
                any(handler!(create_payment, input:CheckedJson<CreateProviderPayment>))
            }
            "getPayment" => any(handler!(get_payment, input:Path<String>)),
            "getHealth" => {
                let name = self.service.name.clone();
                any(move || {
                    let service = name.clone();
                    async move { Json(serde_json::json!({"service":service,"status":"ok"})) }
                })
            }
            _ => panic!("ecommerce operation {operation} has no contract"),
        }
    }

    /// Collects handlers for the service's ecommerce routes.
    #[must_use]
    pub fn routes(&self) -> EcommerceRoutes {
        EcommerceRoutes {
            http: self.clone(),
            handlers: HashMap::new(),
        }
    }

    /// Every ecommerce operation with its packaged handler.
    #[must_use]
    pub fn router(&self) -> ForkLaunchRouter {
        OPERATIONS
            .iter()
            .fold(self.routes(), |routes, operation| {
                routes.route(operation, self.handler(operation))
            })
            .finish()
    }
}

/// The service's ecommerce routes. Each operation's contract comes from the
/// ecommerce interface, so routes, SDKs, `OpenAPI` and MCP tools share one declaration.
pub struct EcommerceRoutes {
    http: EcommerceHttp,
    handlers: HashMap<String, Handler>,
}

impl EcommerceRoutes {
    /// Serve `operation` with `handler`. Operations without a handler are not mounted.
    ///
    /// # Panics
    /// Panics if `operation` is not an ecommerce operation or is already routed.
    #[must_use]
    pub fn route(mut self, operation: &str, handler: Handler) -> Self {
        assert!(
            OPERATIONS.contains(&operation),
            "ecommerce operation {operation} has no contract"
        );
        assert!(
            self.handlers
                .insert(operation.to_owned(), handler)
                .is_none(),
            "ecommerce operation {operation} is already routed"
        );
        self
    }

    /// Mount the routed operations with request HMAC and JWKS authentication.
    ///
    /// # Panics
    /// Panics if the package-owned registry lacks a routed operation.
    #[must_use]
    pub fn finish(mut self) -> ForkLaunchRouter {
        let registry = contracts::module_registry();
        let auth = Arc::clone(&self.http.auth);
        let config = RouterConfig::default().with_authenticator(move |request, contract| {
            let auth = Arc::clone(&auth);
            async move { auth.authenticate(request, contract).await }
        });
        let mut router = ForkLaunchRouter::new(self.http.service.clone());
        for operation in OPERATIONS {
            if let Some(handler) = self.handlers.remove(operation) {
                let contract = if operation == "getHealth" {
                    health_contract()
                } else {
                    registry
                        .routes()
                        .iter()
                        .find(|route| route.operation_name == operation)
                        .expect("registered ecommerce route")
                        .clone()
                };
                router = router.route(contract, handler);
            }
        }
        router.with_config(config)
    }
}
#[must_use]
pub fn health_contract() -> forklaunch_core::contract::RouteContract {
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

async fn create_inventory(
    Extension(backend): Extension<Arc<EcommerceBackend>>,
    Extension(session): Extension<AuthenticatedSession>,
    CheckedJson(Json(input)): CheckedJson<CreateInventoryDto>,
) -> Response {
    response(service!(backend, session).create_inventory(input).await)
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
