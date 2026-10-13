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
    AccessLevel, AuthConfig, BodyKind, ContractRegistry, HttpMethod, RouteContract, ValidationMode,
};
use forklaunch_core::{ServiceInfo, telemetry::OpenTelemetryCollector};
use forklaunch_http::{
    Body as FrameworkBody, ErrorKind, ErrorResponse, FrameworkRequest, FrameworkResponse,
    LocalRouter, RequestContext, match_path, validate_framework_request, validation_error_response,
};
use forklaunch_openapi::{OpenApiOptions, OpenApiServer, generate_openapi_json_with_options};
use opentelemetry::trace::FutureExt;
use tower::{Layer, ServiceExt};
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
    /// Optional server-side browser sessions and CSRF policy.
    #[cfg(feature = "cookie-session")]
    pub cookie_sessions: Option<crate::cookie_session::CookieSessions>,
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
    /// Additional Scalar documents, shown after the generated application API.
    pub docs_sources: Vec<crate::ScalarSource>,
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
            #[cfg(feature = "cookie-session")]
            cookie_sessions: None,
            openapi_enabled: true,
            openapi_path: "/openapi.json".to_owned(),
            docs_enabled: true,
            docs_path: "/docs".to_owned(),
            docs_title: "API Reference".to_owned(),
            docs_sources: Vec::new(),
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

mod internal;
#[cfg(test)]
mod versioning_tests;

/// A contract-aware wrapper around [`axum::Router`].
pub struct ForkLaunchRouter {
    base_path: String,
    registry: ContractRegistry,
    routes: HashMap<String, MethodRouter>,
    extended_routes: HashMap<String, HashMap<String, MethodRouter>>,
    config: RouterConfig,
    internal_mounts: Vec<internal::InternalMount>,
}

macro_rules! typed_route_methods {
    ($($method:ident => $variant:ident),* $(,)?) => {
        $(
            #[doc = concat!("Registers a `", stringify!($variant), "` route with its contract and handler.")]
            ///
            /// # Panics
            /// Panics if the contract method differs or duplicates an existing route.
            #[must_use]
            pub fn $method<H, T>(self, contract: RouteContract, handler: H) -> Self
            where
                H: Handler<T, ()>,
                T: 'static,
            {
                assert_eq!(contract.method, HttpMethod::$variant,
                    "route method must match its contract");
                self.route(contract, handler)
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
            extended_routes: HashMap::new(),
            config: RouterConfig::default(),
            internal_mounts: Vec::new(),
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
            router = router.route(contract, handler);
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
            extended_routes: HashMap::new(),
            config: RouterConfig::default(),
            internal_mounts: Vec::new(),
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

    /// Registers any supported method using the contract as the dispatch source of truth.
    ///
    /// # Panics
    /// Panics when the same method and path are registered twice.
    #[must_use]
    pub fn route<H, T>(mut self, contract: RouteContract, handler: H) -> Self
    where
        H: Handler<T, ()>,
        T: 'static,
    {
        let contract = self.rebase_contract(contract);
        let path = contract.openapi_path.clone();
        let method = contract.method;
        assert!(
            !self
                .registry
                .routes()
                .iter()
                .any(|registered| registered.openapi_path == path && registered.method == method),
            "duplicate route method and path"
        );
        self.registry.register(contract);
        if let Ok(filter) = axum::routing::MethodFilter::try_from(
            Method::from_bytes(method.as_str().as_bytes()).expect("valid method"),
        ) {
            self.add_method_router(path, axum::routing::on(filter, handler));
        } else {
            self.routes.entry(path.clone()).or_default();
            self.extended_routes
                .entry(path)
                .or_default()
                .insert(method.as_str().to_owned(), axum::routing::any(handler));
        }
        self
    }

    typed_route_methods!(
        get => Get,
        post => Post,
        put => Put,
        patch => Patch,
        delete => Delete,
        head => Head,
        options => Options,
        trace => Trace,
        checkout => Checkout,
        copy => Copy,
        link => Link,
        lock => Lock,
        m_search => MSearch,
        merge => Merge,
        mkactivity => MkActivity,
        mkcol => MkCol,
        move_resource => Move,
        notify => Notify,
        propfind => PropFind,
        proppatch => PropPatch,
        purge => Purge,
        report => Report,
        search => Search,
        subscribe => Subscribe,
        unlink => Unlink,
        unlock => Unlock,
        unsubscribe => Unsubscribe,

    );

    /// Nests another router under `prefix`, flattening its contracts and routes
    /// so a single validation layer continues to cover every endpoint.
    ///
    /// # Panics
    /// Panics on overlapping method/path registrations.
    #[must_use]
    pub fn nest(mut self, prefix: impl AsRef<str>, child: ForkLaunchRouter) -> Self {
        let prefix = join_base_path(&self.base_path, prefix.as_ref());

        let ForkLaunchRouter {
            registry,
            routes,
            extended_routes,
            internal_mounts,
            ..
        } = child;

        for mut mount in internal_mounts {
            mount.prefix = join_base_path(&prefix, &mount.prefix);
            for namespace in &mut mount.reserved_prefixes {
                *namespace = join_base_path(&prefix, namespace);
            }
            self.internal_mounts.push(mount);
        }

        for mut route in registry.into_artifact().routes {
            route.path = join_base_path(&prefix, &route.path);
            route.openapi_path = join_base_path(&prefix, &route.openapi_path);
            self.registry.register(route);
        }

        for (path, method_router) in routes {
            let nested_path = join_base_path(&prefix, &path);
            self.add_method_router(nested_path, method_router);
        }

        for (path, methods) in extended_routes {
            let target = self
                .extended_routes
                .entry(join_base_path(&prefix, &path))
                .or_default();
            for (method, handler) in methods {
                assert!(
                    target.insert(method, handler).is_none(),
                    "duplicate nested route"
                );
            }
        }

        self
    }

    /// Decorates handlers at a registered router-relative path without changing
    /// their contracts. The transform covers standard and extended methods,
    /// survives nesting, and is used by HTTP and MCP dispatch alike. Use
    /// `MethodRouter::layer` to include extended-method fallback handlers.
    /// # Errors
    /// Rejects an unregistered path.
    pub fn map_route(
        mut self,
        path: &str,
        mut transform: impl FnMut(MethodRouter) -> MethodRouter,
    ) -> Result<Self, String> {
        let path =
            forklaunch_common::open_api_compliant_path(&join_base_path(&self.base_path, path));
        if !self.routes.contains_key(&path) {
            return Err("Cannot decorate an unregistered route".into());
        }
        let has_standard = self.registry.routes().iter().any(|route| {
            route.openapi_path == path
                && Method::from_bytes(route.method.as_str().as_bytes())
                    .is_ok_and(|method| axum::routing::MethodFilter::try_from(method).is_ok())
        });
        if has_standard {
            let Some(route) = self.routes.remove(&path) else {
                return Err("Cannot decorate an unregistered route".into());
            };
            self.routes.insert(path.clone(), transform(route));
        }
        if let Some(methods) = self.extended_routes.get_mut(&path) {
            for handler in methods.values_mut() {
                *handler = transform(std::mem::take(handler));
            }
        }
        Ok(self)
    }

    /// Borrows the underlying contract registry (useful for SDK/OpenAPI export).
    #[must_use]
    pub const fn registry(&self) -> &ContractRegistry {
        &self.registry
    }

    /// Builds the final [`axum::Router`] with validation, docs, and middleware.
    ///
    /// # Panics
    /// Panics on invalid or overlapping paths, including documentation endpoints.
    pub fn into_router(self) -> Router {
        let Self {
            registry,
            routes,
            mut extended_routes,
            config,
            internal_mounts,
            ..
        } = self;

        internal::validate(&internal_mounts, &registry, &config)
            .expect("invalid internal HTTP mount");
        let mut router = Router::new();
        for (path, mut method_router) in routes {
            if let Some(methods) = extended_routes.remove(&path) {
                let mut allowed: Vec<_> = registry
                    .routes()
                    .iter()
                    .filter(|route| route.openapi_path == path)
                    .map(|route| route.method.as_str())
                    .collect();
                if allowed.contains(&"GET") && !allowed.contains(&"HEAD") {
                    allowed.push("HEAD");
                }
                allowed.sort_unstable();
                allowed.dedup();
                let allow = allowed.join(", ");
                method_router = method_router.fallback(move |request: Request| {
                    let handler = methods.get(request.method().as_str()).cloned();
                    let allow = allow.clone();
                    async move {
                        match handler {
                            Some(handler) => match handler.oneshot(request).await {
                                Ok(response) => response,
                                Err(never) => match never {},
                            },
                            None => (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, allow)])
                                .into_response(),
                        }
                    }
                });
            }
            router = router.route(&path, method_router);
        }

        for mount in internal_mounts {
            router = if mount.prefix == "/" {
                router.merge(mount.router)
            } else {
                router.nest(&mount.prefix, mount.router)
            };
        }

        router = mount_documentation(router, &config, &registry);

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
        let router = router
            .layer(RequestBodyLimitLayer::new(config.body_limit))
            .layer(middleware::from_fn_with_state(telemetry, observe_request))
            .layer(PropagateRequestIdLayer::x_request_id())
            .layer(SetRequestIdLayer::x_request_id(
                CounterMakeRequestId::default(),
            ))
            .layer(TraceLayer::new_for_http())
            .layer(middleware::from_fn(move |request: Request, next: Next| {
                let cors = cors.clone();
                async move {
                    // Only a genuine CORS preflight may bypass an explicit OPTIONS handler.
                    let preflight = request.headers().contains_key(header::ORIGIN)
                        && request
                            .headers()
                            .contains_key(header::ACCESS_CONTROL_REQUEST_METHOD);
                    if request.method() == Method::OPTIONS && !preflight {
                        return next.run(request).await;
                    }
                    let service = tower::service_fn(move |request| {
                        let next = next.clone();
                        async move { Ok::<_, std::convert::Infallible>(next.run(request).await) }
                    });
                    match cors.layer(service).oneshot(request).await {
                        Ok(response) => response,
                        Err(never) => match never {},
                    }
                }
            }));
        #[cfg(feature = "cookie-session")]
        if let Some(sessions) = config.cookie_sessions {
            return sessions.wrap(router);
        }
        router
    }

    /// Mounts an MCP transport whose tools dispatch through this HTTP router.
    /// # Errors
    /// Rejects invalid MCP configuration and conflicting tool names or paths.
    #[cfg(feature = "mcp")]
    pub fn into_router_with_mcp(
        self,
        options: forklaunch_mcp::server::McpServerOptions,
    ) -> Result<Router, String> {
        let (http, transport) = self.into_routers_with_mcp(options)?;
        Ok(http.merge(transport))
    }

    /// Builds separate HTTP and MCP-only routers; MCP still invokes the same
    /// authenticated HTTP handlers internally.
    /// # Errors
    /// Rejects invalid MCP options, names and endpoint collisions.
    #[cfg(feature = "mcp")]
    pub fn into_routers_with_mcp(
        self,
        options: forklaunch_mcp::server::McpServerOptions,
    ) -> Result<(Router, Router), String> {
        if self.internal_mounts.iter().any(|mount| {
            mount
                .reserved_prefixes
                .iter()
                .any(|prefix| internal::contains_path(prefix, &options.path))
        }) {
            return Err("MCP endpoint conflicts with an internal HTTP namespace".into());
        }
        if (self.config.openapi_enabled && options.path == self.config.openapi_path)
            || (self.config.openapi_enabled
                && self.config.docs_enabled
                && options.path == self.config.docs_path)
            || (self.config.openapi_enabled
                && self.config.swagger_enabled
                && options.path == self.config.swagger_path)
        {
            return Err("MCP endpoint conflicts with documentation".into());
        }
        let registry = self.registry.clone();
        let http = self.into_router();
        let transport = forklaunch_mcp::server::http_transport(http.clone(), &registry, options)?;
        Ok((http, transport))
    }

    /// Enables `/mcp` on generated services. Server-owned environment variables
    /// configure `MCP_ENABLED`, `MCP_PATH`, `MCP_ALLOWED_HOSTS` and
    /// `MCP_ALLOWED_ORIGINS`; route authentication remains in effect.
    /// # Errors
    /// Rejects invalid environment settings and MCP configuration.
    #[cfg(feature = "mcp")]
    pub fn into_router_with_default_mcp(self) -> Result<Router, String> {
        match self.default_mcp_options()? {
            Some(options) => self.into_router_with_mcp(options),
            None => Ok(self.into_router()),
        }
    }

    #[cfg(feature = "mcp")]
    pub(super) fn default_mcp_options(
        &self,
    ) -> Result<Option<forklaunch_mcp::server::McpServerOptions>, String> {
        let enabled = std::env::var("MCP_ENABLED").unwrap_or_else(|_| "true".into());
        match enabled.as_str() {
            "false" | "0" => return Ok(None),
            "true" | "1" => {}
            _ => return Err("MCP_ENABLED must be true, false, 1 or 0".into()),
        }
        let service = self.registry.service();
        let mut options = forklaunch_mcp::server::McpServerOptions::new(
            service.map_or("forklaunch-service", |s| s.name.as_str()),
            service.map_or("0.1.0", |s| s.version.as_str()),
        );
        if let Ok(path) = std::env::var("MCP_PATH") {
            options.path = path;
        }
        for (variable, hosts) in [("MCP_ALLOWED_HOSTS", true), ("MCP_ALLOWED_ORIGINS", false)] {
            if let Ok(value) = std::env::var(variable) {
                let values: Vec<_> = value
                    .split(',')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned)
                    .collect();
                if values.is_empty() {
                    return Err(format!("{variable} must not be empty"));
                }
                if hosts {
                    options.transport = options.transport.with_allowed_hosts(values);
                } else {
                    options.transport = options.transport.with_allowed_origins(values);
                }
            }
        }
        Ok(Some(options))
    }

    /// Serves HTTP and optional MCP with coordinated graceful shutdown. Use
    /// `MCP_LISTENER=separate` (HTTP port + 2000), `MCP_PORT` or `MCP_HOST` for a
    /// separate listener; the default keeps MCP on the HTTP listener.
    /// # Errors
    /// Returns invalid configuration, bind, server and bounded drain errors.
    #[cfg(feature = "mcp")]
    pub async fn serve_with_default_mcp<F>(
        self,
        listener: tokio::net::TcpListener,
        shutdown: F,
    ) -> std::io::Result<()>
    where
        F: std::future::Future<Output = ()> + Send,
    {
        crate::mcp_runtime::serve(self, listener, shutdown).await
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
    versions: HashMap<String, forklaunch_http::VersionedRoutePlan>,
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
        let versions = registry
            .routes()
            .iter()
            .map(|route| {
                (
                    route_key(route.method, &route.openapi_path),
                    forklaunch_http::VersionedRoutePlan::compile(route)
                        .expect("invalid versioned route schema"),
                )
            })
            .collect();
        Self {
            routes,
            versions,
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
        let authenticated = if let Some(AuthConfig::Cookie { name }) = &contract.auth {
            #[cfg(feature = "cookie-session")]
            let result = crate::cookie_session::authenticate(&parts, name).await;
            #[cfg(not(feature = "cookie-session"))]
            let result = {
                let _ = name;
                Err(AuthError::MissingCredentials)
            };
            result
        } else {
            match &state.authenticator {
                Some(verifier) => verifier(framework_request.clone(), contract.clone()).await,
                None => Err(AuthError::MissingCredentials),
            }
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
                // The TypeScript JWT middleware returns 403 for presented but
                // rejected tokens. Opaque sessions retain their 401 semantics.
                let response = if error == AuthError::InvalidCredentials
                    && matches!(contract.auth, Some(AuthConfig::Jwt(_)))
                {
                    ErrorResponse::new(ErrorKind::Forbidden, "invalid credentials")
                } else {
                    error.to_error_response()
                };
                return framework_response_to_axum(
                    response
                        .with_correlation_id(framework_request.context.correlation_id)
                        .to_response(),
                );
            }
        }
    }
    let version_plan = &state.versions[&route_key(contract.method, &contract.openapi_path)];
    let contract =
        match select_contract_version(version_plan, contract, &mut parts, &framework_request) {
            Ok(contract) => contract,
            Err(errors) => {
                return framework_response_to_axum(validation_error_response(
                    &errors,
                    &framework_request.context.correlation_id,
                ));
            }
        };
    let errors = validate_framework_request(&contract, &framework_request);
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

fn select_contract_version(
    plan: &forklaunch_http::VersionedRoutePlan,
    contract: RouteContract,
    parts: &mut Parts,
    request: &FrameworkRequest,
) -> Result<RouteContract, Vec<forklaunch_http::ContractValidationError>> {
    match plan.select(request) {
        Ok(Some((selected, version))) => {
            parts.extensions.insert(version);
            Ok(selected)
        }
        Err(errors) if contract.options.request_validation == ValidationMode::Error => Err(errors),
        Ok(None) | Err(_) => Ok(contract),
    }
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

    let mut request = FrameworkRequest::new(
        method_to_contract(&parts.method).unwrap_or(contract.method),
        path,
        RequestContext::new(correlation_id),
    );
    request.params = match_path(&contract.openapi_path, path).unwrap_or_default();

    for (key, value) in parse_query(parts.uri.query().unwrap_or_default()) {
        request = request.with_query(key, value);
    }

    for (name, value) in &parts.headers {
        if let Ok(value) = value.to_str() {
            request = request.with_header(name.as_str(), value);
        }
    }

    if contract.request.body.as_ref().is_some_and(|body| {
        matches!(
            body.kind,
            BodyKind::Binary | BodyKind::File | BodyKind::Multipart
        )
    }) {
        request = request.with_body(FrameworkBody::Binary(bytes.to_vec()));
    } else if !bytes.is_empty()
        || contract
            .request
            .body
            .as_ref()
            .is_some_and(|body| matches!(body.kind, BodyKind::Text | BodyKind::UrlEncoded))
    {
        request = match std::str::from_utf8(bytes) {
            Ok(text) => request.with_body(FrameworkBody::Text(text.to_owned())),
            Err(_) => request.with_body(FrameworkBody::Binary(bytes.to_vec())),
        };
    }

    request.version = parts
        .extensions
        .get::<forklaunch_http::SelectedVersion>()
        .cloned();
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
    HttpMethod::parse(method.as_str())
}

/// Converts a framework response, preserving its status, body, and headers.
#[must_use]
pub fn framework_response_to_axum(response: FrameworkResponse) -> Response {
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

fn mount_documentation(
    mut router: Router,
    config: &RouterConfig,
    registry: &ContractRegistry,
) -> Router {
    if config.openapi_enabled {
        let options = config.openapi_options(registry);
        let document = generate_openapi_json_with_options(registry, &options);
        router = router.route(&config.openapi_path, openapi_handler(document));

        if config.docs_enabled {
            let html = crate::docs::scalar_html(
                &config.docs_title,
                &config.openapi_path,
                &config.docs_sources,
            );
            router = router.route(&config.docs_path, docs_handler(html));
        }

        if config.swagger_enabled {
            let html = crate::docs::swagger_html(&config.docs_title, &config.openapi_path);
            router = router.route(&config.swagger_path, docs_handler(html));
        }
    }

    router
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

    #[tokio::test]
    async fn declared_binary_and_file_bodies_preserve_utf8_and_empty_bytes() {
        use forklaunch_contract::SchemaRef;
        for kind in [BodyKind::Binary, BodyKind::File, BodyKind::Multipart] {
            for bytes in [
                b"plain ASCII".to_vec(),
                "hello 水".as_bytes().to_vec(),
                vec![0, 255],
                vec![],
            ] {
                let contract = RouteContract::post("Upload", "Upload bytes", "/upload")
                    .with_request_body(BodyContract {
                        kind,
                        schema: SchemaRef::named("Bytes"),
                        content_type: Some("application/octet-stream".into()),
                    })
                    .with_response(200, None);
                let expected = bytes.clone();
                let mut handlers = LocalRouter::new();
                handlers.register(contract.clone(), move |request: FrameworkRequest| {
                    assert_eq!(request.body, FrameworkBody::Binary(expected.clone()));
                    Ok(FrameworkResponse::new(200).with_body(request.body))
                });
                let app = ForkLaunchRouter::from_local_router(
                    ServiceInfo::new("upload", "1"),
                    [contract],
                    handlers,
                )
                .into_router();
                let response = app
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri("/upload")
                            .header("content-type", "application/octet-stream")
                            .body(AxumBody::from(bytes.clone()))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(
                    to_bytes(response.into_body(), 100).await.unwrap().as_ref(),
                    bytes
                );
            }
        }
    }

    #[tokio::test]
    async fn declared_text_media_with_charset_passes_transport_validation() {
        use forklaunch_contract::SchemaRef;
        for (kind, mime) in [
            (BodyKind::Text, "text/plain; charset=utf-8"),
            (
                BodyKind::UrlEncoded,
                "application/x-www-form-urlencoded; charset=utf-8",
            ),
        ] {
            let contract = RouteContract::post("Echo Text", "Echo Text", "/text")
                .with_request_body(BodyContract {
                    kind,
                    schema: SchemaRef::named("String"),
                    content_type: Some(mime.into()),
                })
                .with_response(200, None);
            let mut handlers = LocalRouter::new();
            handlers.register(contract.clone(), |request: FrameworkRequest| {
                assert!(request.body.as_text().is_some());
                Ok(FrameworkResponse::new(200).with_body(request.body))
            });
            let app = ForkLaunchRouter::from_local_router(
                ServiceInfo::new("echo", "1"),
                [contract],
                handlers,
            )
            .into_router();
            for text in ["hello 水", ""] {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri("/text")
                            .header("content-type", mime.to_ascii_uppercase())
                            .body(AxumBody::from(text))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(
                    to_bytes(response.into_body(), 100).await.unwrap().as_ref(),
                    text.as_bytes()
                );
            }
            let missing_type = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/text")
                        .body(AxumBody::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(missing_type.status(), StatusCode::BAD_REQUEST);
        }
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
    async fn decorating_handlers_preserves_contracts_nesting_and_extended_methods() {
        assert!(
            ForkLaunchRouter::anonymous()
                .map_route("/missing", |route| route)
                .is_err()
        );
        let mut child = ForkLaunchRouter::anonymous();
        for method in [HttpMethod::Post, HttpMethod::Subscribe] {
            child = child.route(
                RouteContract::new(
                    format!("Dispatch {}", method.as_str()),
                    "Event dispatcher",
                    method,
                    "/events/:feature",
                )
                .with_param(FieldContract::required("feature", "String"))
                .with_query(FieldContract::required("limit", "Integer"))
                .with_response(200, None)
                .with_response(404, None),
                || async { (StatusCode::NOT_FOUND, "original") },
            );
        }
        let before = child.registry().clone().into_artifact().to_json();
        let child = child
            .map_route("/events/:feature", |route| {
                route.layer(middleware::from_fn(
                    |request: axum::extract::Request, next: Next| async move {
                        if request
                            .headers()
                            .get("x-dispatch")
                            .is_some_and(|value| value == "new")
                        {
                            (StatusCode::OK, "decorated").into_response()
                        } else {
                            next.run(request).await
                        }
                    },
                ))
            })
            .unwrap();
        assert_eq!(child.registry().clone().into_artifact().to_json(), before);
        let app = ForkLaunchRouter::new(ServiceInfo::new("events", "0.1.0"))
            .nest("/api", child)
            .into_router();
        for method in ["POST", "SUBSCRIBE"] {
            for (query, dispatch, expected, body) in [
                ("?limit=1", "new", StatusCode::OK, Some("decorated")),
                ("?limit=1", "old", StatusCode::NOT_FOUND, Some("original")),
                ("", "new", StatusCode::BAD_REQUEST, None),
            ] {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method)
                            .uri(format!("/api/events/email{query}"))
                            .header("x-dispatch", dispatch)
                            .body(AxumBody::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), expected);
                if let Some(body) = body {
                    assert_eq!(to_bytes(response.into_body(), 1024).await.unwrap(), body);
                }
            }
        }
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

#[cfg(test)]
mod extended_method_tests {
    use super::*;
    use forklaunch_auth::Principal;
    use forklaunch_contract::FieldContract;

    #[tokio::test]
    async fn every_exported_method_dispatches_validates_and_authenticates_after_nesting() {
        let mut child = ForkLaunchRouter::anonymous();
        for method in HttpMethod::ALL {
            let contract =
                RouteContract::new(method.as_str(), "Method probe", method, "/probe/:id")
                    .with_access(AccessLevel::Authenticated)
                    .with_param(FieldContract::required("id", "String"))
                    .with_response(200, None);
            child = child.route(contract, move |request: Request| async move {
                assert_eq!(request.method().as_str(), method.as_str());
                assert!(request.extensions().get::<AuthenticatedSession>().is_some());
                (StatusCode::OK, [("x-method", method.as_str())])
            });
        }
        let config = RouterConfig::default().with_authenticator(|request, contract| async move {
            assert_eq!(request.method, contract.method);
            assert_eq!(request.params.get("id").map(String::as_str), Some("42"));
            if request.headers.get("authorization").map(String::as_str) != Some("verified") {
                return Err(AuthError::MissingCredentials);
            }
            Ok(AuthenticatedSession::new(
                Principal::new("tester"),
                serde_json::json!({"sub":"tester"}),
            ))
        });
        let app = ForkLaunchRouter::anonymous()
            .nest("/module", child)
            .with_config(config)
            .into_router();
        for method in HttpMethod::ALL {
            for authorized in [false, true] {
                let mut request = Request::builder()
                    .method(method.as_str())
                    .uri("/module/probe/42");
                if authorized {
                    request = request.header("authorization", "verified");
                }
                let response = app
                    .clone()
                    .oneshot(request.body(AxumBody::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(
                    response.status(),
                    if authorized {
                        StatusCode::OK
                    } else {
                        StatusCode::UNAUTHORIZED
                    },
                    "{}",
                    method.as_str()
                );
                if authorized {
                    assert_eq!(response.headers()["x-method"], method.as_str());
                }
            }
        }
        let preflight = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/module/probe/42")
                    .header(header::ORIGIN, "https://browser.example")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(preflight.status(), StatusCode::OK);
        assert!(
            preflight
                .headers()
                .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("BREW")
                    .uri("/module/probe/42")
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        let allow = response.headers()[header::ALLOW].to_str().unwrap();
        for method in HttpMethod::ALL {
            assert!(
                allow.split(", ").any(|value| value == method.as_str()),
                "{allow}"
            );
        }
    }

    #[tokio::test]
    async fn local_handlers_preserve_extended_methods() {
        let mut local = LocalRouter::new();
        let contracts: Vec<_> = HttpMethod::ALL
            .into_iter()
            .filter(|method| !method.is_openapi_standard())
            .map(|method| {
                let contract =
                    RouteContract::new(method.as_str(), "Local method", method, "/probe")
                        .with_response(200, None);
                local.register(contract.clone(), move |request: FrameworkRequest| {
                    assert_eq!(request.method, method);
                    Ok(FrameworkResponse::text(200, method.as_str()))
                });
                contract
            })
            .collect();
        let app = ForkLaunchRouter::from_local_router(
            ServiceInfo::new("methods", "0.1.0"),
            contracts,
            local,
        )
        .into_router();
        for method in HttpMethod::ALL
            .into_iter()
            .filter(|method| !method.is_openapi_standard())
        {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method.as_str())
                        .uri("/probe")
                        .body(AxumBody::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                to_bytes(response.into_body(), 1024).await.unwrap(),
                method.as_str()
            );
        }
    }

    #[test]
    #[should_panic(expected = "route method must match its contract")]
    fn mismatched_handler_method_cannot_bypass_contract_security() {
        let _ = ForkLaunchRouter::anonymous()
            .get(RouteContract::post("wrong", "wrong", "/wrong"), || async {
                StatusCode::OK
            });
    }
}
