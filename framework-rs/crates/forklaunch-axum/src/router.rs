//! Real `axum`-backed router (`ForkLaunchRouter`).
//!
//! Wraps [`axum::Router`] while keeping a [`ContractRegistry`] in sync so that
//! contract-driven request validation, `OpenAPI` generation, and documentation
//! endpoints all flow from the same source of truth as the rest of the
//! framework. Enabled by the default `adapter` feature.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::session::{AuthenticatedSession, Authenticator};
use axum::{
    Router,
    body::{Body as AxumBody, to_bytes},
    extract::{MatchedPath, Request, State},
    handler::Handler,
    http::{HeaderValue, Method, StatusCode, header, request::Parts},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::MethodRouter,
};
use forklaunch_auth::{AuthError, authorize};
use forklaunch_common::{join_base_path, sanitize_path_slashes};
use forklaunch_contract::{
    AccessLevel, ContractRegistry, HttpMethod, RouteContract, ValidationMode,
};
use forklaunch_core::{ServiceInfo, telemetry::OpenTelemetryCollector};
use forklaunch_http::{
    Body as FrameworkBody, ErrorKind, ErrorResponse, FrameworkRequest, FrameworkResponse,
    LocalRouter, RequestContext, match_path, validate_framework_request, validation_error_response,
};
use forklaunch_openapi::{OpenApiOptions, OpenApiServer, generate_openapi_json_with_options};
use opentelemetry::trace::FutureExt;
use tower_http::{
    cors::CorsLayer,
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestId, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    trace::TraceLayer,
};

/// Default maximum request body size accepted before a `400` is returned (2 MiB).
pub const DEFAULT_BODY_LIMIT_BYTES: usize = 2 * 1024 * 1024;

/// Configuration for cross-cutting router behavior (docs, limits, CORS).
#[derive(Clone)]
pub struct RouterConfig {
    /// Whether the generated `OpenAPI` document is served.
    pub openapi_enabled: bool,
    /// Path the `OpenAPI` JSON document is served from.
    pub openapi_path: String,
    /// Whether the Scalar documentation UI is served.
    pub docs_enabled: bool,
    /// Path the documentation UI is served from.
    pub docs_path: String,
    /// Title rendered in the documentation UI.
    pub docs_title: String,
    /// Whether the Swagger UI is also served.
    pub swagger_enabled: bool,
    /// Path the Swagger UI is served from.
    pub swagger_path: String,
    /// Maximum accepted request body size in bytes.
    pub body_limit: usize,
    /// Optional `OpenAPI` `info.description`.
    pub description: Option<String>,
    /// `OpenAPI` servers advertised in the generated document.
    pub servers: Vec<OpenApiServer>,
    /// CORS policy; defaults to a permissive policy when unset.
    pub cors: Option<CorsLayer>,
    /// Credential verifier for routes requiring authentication. Missing verifiers fail closed.
    pub authenticator: Option<Authenticator>,
    /// Inject a configured collector to export HTTP traces and metrics.
    pub telemetry: Option<OpenTelemetryCollector>,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            openapi_enabled: true,
            openapi_path: "/openapi.json".to_owned(),
            docs_enabled: true,
            docs_path: "/docs".to_owned(),
            docs_title: "API Reference".to_owned(),
            swagger_enabled: false,
            swagger_path: "/swagger".to_owned(),
            body_limit: DEFAULT_BODY_LIMIT_BYTES,
            description: None,
            servers: Vec::new(),
            cors: None,
            authenticator: None,
            telemetry: None,
        }
    }
}

impl RouterConfig {
    /// Configures credential verification and optional session enrichment.
    /// The verifier receives the raw request and declared contract, including auth
    /// method/header settings. It must verify credentials before returning a session.
    #[must_use]
    pub fn with_authenticator<F, Fut>(mut self, verifier: F) -> Self
    where
        F: Fn(FrameworkRequest, RouteContract) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<AuthenticatedSession, AuthError>> + Send + 'static,
    {
        self.authenticator = Some(Arc::new(move |request, contract| {
            Box::pin(verifier(request, contract))
        }));
        self
    }

    fn openapi_options(&self, registry: &ContractRegistry) -> OpenApiOptions {
        let mut options = OpenApiOptions::from_registry(registry);
        if let Some(description) = &self.description {
            options = options.with_description(description.clone());
        }
        for server in &self.servers {
            options = options.with_server(server.clone());
        }
        options
    }
}

/// A contract-aware wrapper around [`axum::Router`].
pub struct ForkLaunchRouter {
    base_path: String,
    registry: ContractRegistry,
    routes: HashMap<String, MethodRouter>,
    config: RouterConfig,
}

macro_rules! typed_route_methods {
    ($($method:ident => $verb:ident),* $(,)?) => {
        $(
            #[doc = concat!("Registers a `", stringify!($verb), "` route with its contract and handler.")]
            #[must_use]
            pub fn $method<H, T>(mut self, contract: RouteContract, handler: H) -> Self
            where
                H: Handler<T, ()>,
                T: 'static,
            {
                let contract = self.rebase_contract(contract);
                let path = contract.openapi_path.clone();
                self.registry.register(contract);
                self.add_method_router(path, axum::routing::$verb(handler));
                self
            }
        )*
    };
}

impl ForkLaunchRouter {
    /// Creates a router that tags generated documentation with the given service.
    #[must_use]
    pub fn new(service: ServiceInfo) -> Self {
        Self {
            base_path: "/".to_owned(),
            registry: ContractRegistry::for_service(service.contract()),
            routes: HashMap::new(),
            config: RouterConfig::default(),
        }
    }

    /// Serves synchronous framework handlers through Axum's HTTP transport.
    /// Contract registration, validation, docs, and wire handling share one registry.
    #[must_use]
    pub fn from_local_router(
        service: ServiceInfo,
        contracts: impl IntoIterator<Item = RouteContract>,
        handlers: LocalRouter,
    ) -> Self {
        let handlers = Arc::new(handlers);
        let mut router = Self::new(service);
        for contract in contracts {
            let handlers = Arc::clone(&handlers);
            let handler_contract = contract.clone();
            let handler = move |request: Request| {
                let handlers = Arc::clone(&handlers);
                let contract = handler_contract.clone();
                async move {
                    let (parts, body) = request.into_parts();
                    let Ok(bytes) = to_bytes(body, usize::MAX).await else {
                        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
                    };
                    let request = build_framework_request(&contract, &parts, &bytes);
                    match handlers.execute(request) {
                        Ok(response) => framework_response_to_axum(response),
                        Err(error) => {
                            tracing::error!(error = %error, "local request handler failed");
                            framework_response_to_axum(
                                ErrorResponse::new(ErrorKind::Internal, "internal server error")
                                    .to_response(),
                            )
                        }
                    }
                }
            };
            router = match contract.method {
                HttpMethod::Get => router.get(contract, handler),
                HttpMethod::Post => router.post(contract, handler),
                HttpMethod::Put => router.put(contract, handler),
                HttpMethod::Patch => router.patch(contract, handler),
                HttpMethod::Delete => router.delete(contract, handler),
                HttpMethod::Head => router.head(contract, handler),
                HttpMethod::Options => router.options(contract, handler),
                HttpMethod::Trace => router.trace(contract, handler),
            };
        }
        router
    }

    /// Creates a router without any associated service metadata.
    #[must_use]
    pub fn anonymous() -> Self {
        Self {
            base_path: "/".to_owned(),
            registry: ContractRegistry::new(),
            routes: HashMap::new(),
            config: RouterConfig::default(),
        }
    }

    /// Sets a base path prepended to every route registered afterwards.
    #[must_use]
    pub fn with_base_path(mut self, base_path: impl AsRef<str>) -> Self {
        self.base_path = sanitize_path_slashes(base_path.as_ref());
        self
    }

    /// Overrides the cross-cutting router configuration.
    #[must_use]
    pub fn with_config(mut self, config: RouterConfig) -> Self {
        self.config = config;
        self
    }

    /// Adds an `OpenAPI` server entry advertised in the generated document.
    #[must_use]
    pub fn with_server(mut self, server: OpenApiServer) -> Self {
        self.config.servers.push(server);
        self
    }

    /// Sets the `OpenAPI` `info.description`.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.config.description = Some(description.into());
        self
    }

    typed_route_methods!(
        get => get,
        post => post,
        put => put,
        patch => patch,
        delete => delete,
        head => head,
        options => options,
        trace => trace,
    );

    /// Nests another router under `prefix`, flattening its contracts and routes
    /// so a single validation layer continues to cover every endpoint.
    #[must_use]
    pub fn nest(mut self, prefix: impl AsRef<str>, child: ForkLaunchRouter) -> Self {
        let prefix = join_base_path(&self.base_path, prefix.as_ref());

        let ForkLaunchRouter {
            registry, routes, ..
        } = child;

        for mut route in registry.into_artifact().routes {
            route.path = join_base_path(&prefix, &route.path);
            route.openapi_path = join_base_path(&prefix, &route.openapi_path);
            self.registry.register(route);
        }

        for (path, method_router) in routes {
            let nested_path = join_base_path(&prefix, &path);
            self.add_method_router(nested_path, method_router);
        }

        self
    }

    /// Borrows the underlying contract registry (useful for SDK/OpenAPI export).
    #[must_use]
    pub const fn registry(&self) -> &ContractRegistry {
        &self.registry
    }

    /// Builds the final [`axum::Router`] with validation, docs, and middleware.
    pub fn into_router(self) -> Router {
        let Self {
            registry,
            routes,
            config,
            ..
        } = self;

        let mut router = Router::new();
        for (path, method_router) in routes {
            router = router.route(&path, method_router);
        }

        if config.openapi_enabled {
            let options = config.openapi_options(&registry);
            let document = generate_openapi_json_with_options(&registry, &options);
            router = router.route(&config.openapi_path, openapi_handler(document));

            if config.docs_enabled {
                let html = scalar_html(&config.docs_title, &config.openapi_path);
                router = router.route(&config.docs_path, docs_handler(html));
            }

            if config.swagger_enabled {
                let html = swagger_html(&config.docs_title, &config.openapi_path);
                router = router.route(&config.swagger_path, docs_handler(html));
            }
        }

        let telemetry = config
            .telemetry
            .clone()
            .unwrap_or_else(|| OpenTelemetryCollector::new("forklaunch-http"));
        let state = Arc::new(ValidationState::from_registry(
            &registry,
            config.body_limit,
            config.authenticator,
            telemetry.clone(),
        ));
        router = router.layer(middleware::from_fn_with_state(state, validate_contract));

        let cors = config.cors.unwrap_or_else(CorsLayer::permissive);
        router
            .layer(RequestBodyLimitLayer::new(config.body_limit))
            .layer(middleware::from_fn_with_state(telemetry, observe_request))
            .layer(PropagateRequestIdLayer::x_request_id())
            .layer(SetRequestIdLayer::x_request_id(
                CounterMakeRequestId::default(),
            ))
            .layer(TraceLayer::new_for_http())
            .layer(cors)
    }

    /// Binds to `addr` and serves until shutdown.
    ///
    /// # Errors
    ///
    /// Returns an error if the listener cannot bind or the server task fails.
    pub async fn serve(self, addr: SocketAddr) -> std::io::Result<()> {
        let router = self.into_router();
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, router.into_make_service()).await
    }

    fn rebase_contract(&self, mut contract: RouteContract) -> RouteContract {
        if self.base_path != "/" {
            contract.path = join_base_path(&self.base_path, &contract.path);
            contract.openapi_path = join_base_path(&self.base_path, &contract.openapi_path);
        }
        contract
    }

    fn add_method_router(&mut self, path: String, method_router: MethodRouter) {
        let merged = match self.routes.remove(&path) {
            Some(existing) => existing.merge(method_router),
            None => method_router,
        };
        self.routes.insert(path, merged);
    }
}

fn openapi_handler(document: String) -> MethodRouter {
    axum::routing::get(move || {
        let document = document.clone();
        async move { ([(header::CONTENT_TYPE, "application/json")], document) }
    })
}

fn docs_handler(html: String) -> MethodRouter {
    axum::routing::get(move || {
        let html = html.clone();
        async move { Html(html) }
    })
}

/// Indexed contracts used by the validation middleware.
struct ValidationState {
    routes: HashMap<String, RouteContract>,
    body_limit: usize,
    authenticator: Option<Authenticator>,
    telemetry: OpenTelemetryCollector,
}

impl ValidationState {
    fn from_registry(
        registry: &ContractRegistry,
        body_limit: usize,
        authenticator: Option<Authenticator>,
        telemetry: OpenTelemetryCollector,
    ) -> Self {
        let routes = registry
            .routes()
            .iter()
            .map(|route| (route_key(route.method, &route.openapi_path), route.clone()))
            .collect();
        Self {
            routes,
            body_limit,
            authenticator,
            telemetry,
        }
    }
}

struct HeaderExtractor<'a>(&'a axum::http::HeaderMap);
impl opentelemetry::propagation::Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }
    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(axum::http::HeaderName::as_str).collect()
    }
}

async fn observe_request(
    State(collector): State<OpenTelemetryCollector>,
    request: Request,
    next: Next,
) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", MatchedPath::as_str);
    let method = method_to_contract(request.method()).map_or("OTHER", HttpMethod::as_str);
    let correlation_id = request
        .headers()
        .get("x-correlation-id")
        .or_else(|| request.headers().get("x-request-id"))
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let parent = opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderExtractor(request.headers()))
    });
    let measurement = collector.start_request_with_context(method, route, &parent);
    let context = measurement.context();
    let head = request.method() == Method::HEAD;
    let response = next.run(request).with_context(context).await;
    crate::body_telemetry::observe_response(response, measurement, correlation_id, head)
}

fn route_key(method: HttpMethod, path: &str) -> String {
    format!("{} {}", method.as_str(), path)
}

async fn validate_contract(
    State(state): State<Arc<ValidationState>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(method) = method_to_contract(request.method()) else {
        return next.run(request).await;
    };
    let Some(matched) = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| matched.as_str().to_owned())
    else {
        return next.run(request).await;
    };

    // Axum automatically serves HEAD with GET handlers; apply the GET contract
    // unless an explicit HEAD contract was registered, including its auth policy.
    let contract = state.routes.get(&route_key(method, &matched)).or_else(|| {
        (method == HttpMethod::Head)
            .then(|| state.routes.get(&route_key(HttpMethod::Get, &matched)))
            .flatten()
    });
    let Some(contract) = contract.cloned() else {
        return next.run(request).await;
    };

    let (mut parts, body) = request.into_parts();
    let Ok(bytes) = to_bytes(body, state.body_limit).await else {
        return framework_response_to_axum(
            ErrorResponse::new(ErrorKind::Validation, "request body exceeds limit").to_response(),
        );
    };

    let framework_request = build_framework_request(&contract, &parts, &bytes);
    let requires_auth = contract.auth.is_some()
        || contract.rbac.is_some()
        || contract.access != AccessLevel::Public;
    if requires_auth {
        let authenticated = match &state.authenticator {
            Some(verifier) => verifier(framework_request.clone(), contract.clone()).await,
            None => Err(AuthError::MissingCredentials),
        };
        let result = authenticated.and_then(|session| {
            if let Some(rbac) = &contract.rbac {
                authorize(&session.principal, rbac)?;
            }
            Ok(session)
        });
        match result {
            Ok(session) => {
                parts.extensions.insert(session);
            }
            Err(error) => {
                state.telemetry.auth_failure(
                    &contract.operation_name,
                    &framework_request.context.correlation_id,
                );
                tracing::warn!(operation = %contract.operation_name, correlation_id = %framework_request.context.correlation_id, "request authentication or authorization failed");
                return framework_response_to_axum(
                    error
                        .to_error_response()
                        .with_correlation_id(framework_request.context.correlation_id)
                        .to_response(),
                );
            }
        }
    }
    let errors = if contract.options.request_validation == ValidationMode::Error {
        validate_framework_request(&contract, &framework_request)
    } else {
        Vec::new()
    };
    if !errors.is_empty() {
        let correlation_id = framework_request.context.correlation_id.clone();
        return framework_response_to_axum(validation_error_response(&errors, &correlation_id));
    }

    let correlation_id = framework_request.context.correlation_id.clone();
    let response = next
        .run(Request::from_parts(parts, AxumBody::from(bytes)))
        .await;
    validate_response_status(&contract, response, &correlation_id)
}

/// Enforces that handlers only return status codes documented in the contract.
fn validate_response_status(
    contract: &RouteContract,
    response: Response,
    correlation_id: &str,
) -> Response {
    if contract.options.response_validation != ValidationMode::Error
        || contract.responses.is_empty()
    {
        return response;
    }

    let status = response.status().as_u16();
    if contract
        .responses
        .iter()
        .any(|documented| documented.status_code == status)
    {
        return response;
    }

    tracing::error!(status, operation = %contract.operation_name, correlation_id, "handler returned undocumented status");
    framework_response_to_axum(
        ErrorResponse::new(ErrorKind::Internal, "internal server error")
            .with_correlation_id(correlation_id.to_owned())
            .to_response(),
    )
}

fn build_framework_request(
    contract: &RouteContract,
    parts: &Parts,
    bytes: &[u8],
) -> FrameworkRequest {
    let path = parts.uri.path();
    let correlation_id = parts
        .headers
        .get("x-correlation-id")
        .or_else(|| parts.headers.get("x-request-id"))
        .and_then(|value| value.to_str().ok())
        .unwrap_or("forklaunch-request")
        .to_owned();

    let mut request =
        FrameworkRequest::new(contract.method, path, RequestContext::new(correlation_id));
    request.params = match_path(&contract.openapi_path, path).unwrap_or_default();

    for (key, value) in parse_query(parts.uri.query().unwrap_or_default()) {
        request = request.with_query(key, value);
    }

    for (name, value) in &parts.headers {
        if let Ok(value) = value.to_str() {
            request = request.with_header(name.as_str(), value);
        }
    }

    if !bytes.is_empty() {
        request = match std::str::from_utf8(bytes) {
            Ok(text) => request.with_body(FrameworkBody::Text(text.to_owned())),
            Err(_) => request.with_body(FrameworkBody::Binary(bytes.to_vec())),
        };
    }

    request.session = parts
        .extensions
        .get::<AuthenticatedSession>()
        .map(|session| session.claims.clone());
    request
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect()
}

fn method_to_contract(method: &Method) -> Option<HttpMethod> {
    match *method {
        Method::GET => Some(HttpMethod::Get),
        Method::POST => Some(HttpMethod::Post),
        Method::PUT => Some(HttpMethod::Put),
        Method::PATCH => Some(HttpMethod::Patch),
        Method::DELETE => Some(HttpMethod::Delete),
        Method::HEAD => Some(HttpMethod::Head),
        Method::OPTIONS => Some(HttpMethod::Options),
        Method::TRACE => Some(HttpMethod::Trace),
        _ => None,
    }
}

fn framework_response_to_axum(response: FrameworkResponse) -> Response {
    let status =
        StatusCode::from_u16(response.status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = match response.body {
        FrameworkBody::Empty => AxumBody::empty(),
        FrameworkBody::Text(text) => AxumBody::from(text),
        FrameworkBody::Binary(bytes) => AxumBody::from(bytes),
    };

    let mut builder = Response::builder().status(status);
    for (key, value) in response.headers {
        builder = builder.header(key, value);
    }
    builder
        .body(body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// A dependency-free request-id generator backed by a monotonic counter.
#[derive(Clone, Default)]
struct CounterMakeRequestId {
    counter: Arc<AtomicU64>,
}

impl MakeRequestId for CounterMakeRequestId {
    fn make_request_id<B>(&mut self, _request: &axum::http::Request<B>) -> Option<RequestId> {
        let id = self.counter.fetch_add(1, Ordering::Relaxed);
        HeaderValue::from_str(&format!("forklaunch-{id}"))
            .ok()
            .map(RequestId::new)
    }
}

fn scalar_html(title: &str, spec_url: &str) -> String {
    format!(
        "<!doctype html>\n<html>\n  <head>\n    <title>{title}</title>\n    <meta charset=\"utf-8\" />\n    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />\n  </head>\n  <body>\n    <script id=\"api-reference\" data-url=\"{spec_url}\"></script>\n    <script src=\"https://cdn.jsdelivr.net/npm/@scalar/api-reference\"></script>\n  </body>\n</html>\n"
    )
}

fn swagger_html(title: &str, spec_url: &str) -> String {
    format!(
        "<!doctype html>\n<html>\n  <head>\n    <title>{title}</title>\n    <meta charset=\"utf-8\" />\n    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />\n    <link rel=\"stylesheet\" href=\"https://cdn.jsdelivr.net/npm/swagger-ui-dist/swagger-ui.css\" />\n  </head>\n  <body>\n    <div id=\"swagger-ui\"></div>\n    <script src=\"https://cdn.jsdelivr.net/npm/swagger-ui-dist/swagger-ui-bundle.js\"></script>\n    <script>\n      window.onload = () => {{ window.ui = SwaggerUIBundle({{ url: '{spec_url}', dom_id: '#swagger-ui' }}); }};\n    </script>\n  </body>\n</html>\n"
    )
}

#[cfg(test)]
mod tests {
    use axum::{body::Body as AxumBody, http::Request};
    use forklaunch_contract::{BodyContract, FieldContract, RouteContract};
    use forklaunch_core::ServiceInfo;
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn local_handlers_preserve_wire_input_and_binary_output() {
        let contract = RouteContract::post("Echo", "Echo input", "/echo/:id")
            .with_param(FieldContract::required("id", "String"))
            .with_query(FieldContract::required("label", "String"))
            .with_response(201, None);
        let mut handlers = LocalRouter::new();
        handlers.register(contract.clone(), |request: FrameworkRequest| {
            assert_eq!(request.params.get("id").unwrap(), "42");
            assert_eq!(request.query.get("label").unwrap(), "hello world&more");
            assert_eq!(request.headers.get("x-custom").unwrap(), "yes");
            assert_eq!(request.context.correlation_id, "test-correlation");
            assert_eq!(request.body, FrameworkBody::Binary(vec![0, 255, 1]));
            Ok(FrameworkResponse::new(201).with_body(FrameworkBody::Binary(vec![255, 0, 128])))
        });
        let app = ForkLaunchRouter::from_local_router(
            ServiceInfo::new("echo", "0.1.0"),
            [contract],
            handlers,
        )
        .into_router();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/echo/42?label=hello+world%26more")
                    .header("x-custom", "yes")
                    .header("x-correlation-id", "test-correlation")
                    .body(AxumBody::from(vec![0, 255, 1]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            to_bytes(response.into_body(), 100).await.unwrap().as_ref(),
            &[255, 0, 128]
        );
    }

    fn ok_handler() -> impl Handler<((),), ()> {
        || async { "ok" }
    }

    #[tokio::test]
    async fn registers_and_serves_a_route() {
        let app = ForkLaunchRouter::new(ServiceInfo::new("users", "0.1.0"))
            .get(
                RouteContract::get("Get User", "Get a user", "/users/:id")
                    .with_param(FieldContract::required("id", "String"))
                    .with_response(200, None),
                ok_handler(),
            )
            .into_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/users/123")
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rejects_requests_failing_contract_validation() {
        let app = ForkLaunchRouter::new(ServiceInfo::new("users", "0.1.0"))
            .get(
                RouteContract::get("List Users", "List users", "/users")
                    .with_query(FieldContract::required("limit", "Integer"))
                    .with_response(200, None),
                ok_handler(),
            )
            .into_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/users")
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn supports_multiple_methods_on_the_same_path() {
        let app = ForkLaunchRouter::new(ServiceInfo::new("users", "0.1.0"))
            .get(
                RouteContract::get("List Users", "List users", "/users").with_response(200, None),
                ok_handler(),
            )
            .post(
                RouteContract::post("Create User", "Create a user", "/users")
                    .with_request_body(BodyContract::json("CreateUserBody"))
                    .with_response(201, None),
                || async { (StatusCode::CREATED, "created") },
            )
            .into_router();

        let listed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/users")
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(listed.status(), StatusCode::OK);

        let created = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/users")
                    .header("content-type", "application/json")
                    .body(AxumBody::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn nested_routers_share_validation_and_base_paths() {
        let child = ForkLaunchRouter::anonymous().get(
            RouteContract::get("Get Org", "Get an org", "/:id")
                .with_param(FieldContract::required("id", "String"))
                .with_response(200, None),
            ok_handler(),
        );
        let app = ForkLaunchRouter::new(ServiceInfo::new("orgs", "0.1.0"))
            .nest("/orgs", child)
            .into_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/orgs/42")
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rejects_undocumented_response_status() {
        let app = ForkLaunchRouter::new(ServiceInfo::new("users", "0.1.0"))
            .get(
                RouteContract::get("List Users", "List users", "/users").with_response(200, None),
                || async { (StatusCode::IM_A_TEAPOT, "nope") },
            )
            .into_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/users")
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = to_bytes(response.into_body(), DEFAULT_BODY_LIMIT_BYTES)
            .await
            .unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(body.contains("internal server error"));
        assert!(!body.contains("418"));
    }

    #[tokio::test]
    async fn serves_swagger_ui_when_enabled() {
        let config = RouterConfig {
            swagger_enabled: true,
            ..RouterConfig::default()
        };
        let app = ForkLaunchRouter::new(ServiceInfo::new("users", "0.1.0"))
            .with_config(config)
            .get(
                RouteContract::get("List Users", "List users", "/users").with_response(200, None),
                ok_handler(),
            )
            .into_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/swagger")
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), DEFAULT_BODY_LIMIT_BYTES)
            .await
            .unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("swagger-ui"));
        assert!(html.contains("/openapi.json"));
    }

    #[tokio::test]
    async fn serves_generated_openapi_document() {
        let app = ForkLaunchRouter::new(ServiceInfo::new("users", "1.0.0"))
            .get(
                RouteContract::get("List Users", "List users", "/users").with_response(200, None),
                ok_handler(),
            )
            .into_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/openapi.json")
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), DEFAULT_BODY_LIMIT_BYTES)
            .await
            .unwrap();
        let document = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(document.contains("\"openapi\":\"3.1.0\""));
        assert!(document.contains("listUsers"));
    }
}
