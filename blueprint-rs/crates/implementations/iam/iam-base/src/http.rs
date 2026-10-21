//! Contract-bound HTTP handlers for an asynchronous, tenant-bound IAM backend.
//! Generated services bind them in their own controllers and routes; storage,
//! authentication and audit stay here.
#[cfg(feature = "browser-sessions")]
pub mod browser;
pub mod directory;
use axum::{
    Json,
    extract::{
        Path, Query,
        rejection::{JsonRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{MethodRouter, any},
};
use forklaunch_axum::{
    ForkLaunchRouter, RouterConfig, module_auth::HmacJwksAuth, session::AuthenticatedSession,
};
use forklaunch_core::ServiceInfo;
use forklaunch_core::auth::{AuthError, Principal, parse_bearer};
use forklaunch_core::contract::RouteContract;
use forklaunch_core::domain::BlueprintError;
use forklaunch_core::domain::PageRequest;
use forklaunch_core::http::ErrorKind;
use forklaunch_core::http::ErrorResponse;
use forklaunch_iam_interface::{AsyncIamSessions, Credentials, module_contracts};
use std::{collections::HashMap, sync::Arc};

use crate::directory::{compliance::DirectoryCompliance, postgres::PostgresDirectoryStore};
pub use directory::DirectoryAuthorizationMode;

/// One operation's request handler. Its contract supplies the method and path.
pub type Handler = MethodRouter;

/// Opaque session operations, in their mounted order.
const SESSION_OPERATIONS: [&str; 7] = [
    "getHealth",
    "authenticate",
    "logout",
    "getSessionUser",
    "listUsers",
    "listUserPermissions",
    "listOrganizations",
];

/// Session, directory and compliance handlers over one trusted tenant. The
/// stores must already be bound to that tenant; HTTP input never selects one.
pub struct IamHttp<S> {
    sessions: Arc<S>,
    directory: Arc<PostgresDirectoryStore>,
    compliance: Arc<DirectoryCompliance>,
    auth: Arc<HmacJwksAuth>,
    mode: DirectoryAuthorizationMode,
}

impl<S> Clone for IamHttp<S> {
    fn clone(&self) -> Self {
        Self {
            sessions: Arc::clone(&self.sessions),
            directory: Arc::clone(&self.directory),
            compliance: Arc::clone(&self.compliance),
            auth: Arc::clone(&self.auth),
            mode: self.mode,
        }
    }
}

impl<S: AsyncIamSessions + 'static> IamHttp<S> {
    /// Directory reads use JWKS JWTs and internal operations use request HMAC
    /// signatures. Stored-session mode verifies the JWT, then replaces identity
    /// and grants with the persisted session before framework RBAC runs.
    #[must_use]
    pub fn new(
        sessions: Arc<S>,
        directory: PostgresDirectoryStore,
        auth: HmacJwksAuth,
        mode: DirectoryAuthorizationMode,
    ) -> Self {
        let auth = if mode == DirectoryAuthorizationMode::StoredSessions {
            auth.with_role_resolver(|_| async { Ok(Vec::new()) })
        } else {
            auth
        };
        Self {
            sessions,
            directory: Arc::new(directory),
            compliance: Arc::new(DirectoryCompliance::default()),
            auth: Arc::new(auth),
            mode,
        }
    }

    /// Use application-owned compliance classifications for export and erasure.
    /// Incomplete storage operations return an error rather than a successful erasure.
    #[must_use]
    pub fn with_compliance(mut self, compliance: DirectoryCompliance) -> Self {
        self.compliance = Arc::new(compliance);
        self
    }

    /// The handler for one session, directory or compliance operation.
    ///
    /// # Panics
    /// Panics if `operation` is not an IAM operation.
    pub fn handler(&self, operation: &str) -> Handler {
        if SESSION_OPERATIONS.contains(&operation) {
            self.session_handler(operation)
        } else {
            directory::handler(self, &contract(operation))
        }
    }

    /// Collects handlers for the service's IAM routes.
    #[must_use]
    pub fn routes(&self, info: ServiceInfo) -> IamRoutes<S> {
        IamRoutes {
            http: self.clone(),
            info,
            handlers: HashMap::new(),
        }
    }

    fn session_handler(&self, operation: &str) -> Handler {
        let service = Arc::clone(&self.sessions);
        match operation {
            "getHealth" => any(|| async { Json(serde_json::json!({"status":"ok"})) }),
            "authenticate" => any(
                move |headers: HeaderMap, credentials: Result<Json<Credentials>, JsonRejection>| {
                    login_handler(Arc::clone(&service), headers, credentials)
                },
            ),
            "logout" => {
                any(move |headers: HeaderMap| logout_handler(Arc::clone(&service), headers))
            }
            "getSessionUser" => any(move |headers: HeaderMap, Path(id): Path<String>| {
                user_handler(Arc::clone(&service), headers, id)
            }),
            "listUsers" => any(
                move |headers: HeaderMap, query: Result<Query<PageQuery>, QueryRejection>| {
                    users_handler(Arc::clone(&service), headers, query)
                },
            ),
            "listUserPermissions" => any(move |headers: HeaderMap, Path(id): Path<String>| {
                permissions_handler(Arc::clone(&service), headers, id)
            }),
            _ => any(
                move |headers: HeaderMap, query: Result<Query<PageQuery>, QueryRejection>| {
                    organizations_handler(Arc::clone(&service), headers, query)
                },
            ),
        }
    }
}

/// The service's IAM routes. Each operation's contract comes from the IAM
/// interface, so routes, SDKs, `OpenAPI` and MCP tools share one declaration.
pub struct IamRoutes<S> {
    http: IamHttp<S>,
    info: ServiceInfo,
    handlers: HashMap<String, Handler>,
}

impl<S: AsyncIamSessions + 'static> IamRoutes<S> {
    /// Serve `operation` with `handler`. Operations without a handler are not mounted.
    ///
    /// # Panics
    /// Panics if `operation` is not an IAM operation or is already routed.
    #[must_use]
    pub fn route(mut self, operation: &str, handler: Handler) -> Self {
        let _ = contract(operation);
        assert!(
            self.handlers
                .insert(operation.to_owned(), handler)
                .is_none(),
            "IAM operation {operation} is already routed"
        );
        self
    }

    /// Mount the routed operations in contract order with IAM authentication.
    #[must_use]
    pub fn finish(mut self) -> ForkLaunchRouter {
        let mut router = ForkLaunchRouter::new(self.info.clone());
        let directory = module_contracts::registry();
        let operations = SESSION_OPERATIONS
            .iter()
            .map(|operation| (*operation).to_owned())
            .chain(
                directory
                    .routes()
                    .iter()
                    .map(|route| route.operation_name.clone()),
            );
        for operation in operations {
            if let Some(handler) = self.handlers.remove(&operation) {
                router = router.route(contract(&operation), handler);
            }
        }
        router.with_config(directory::router_config(&self.http))
    }
}

/// The combined session and directory contract for `operation`.
fn contract(operation: &str) -> RouteContract {
    module_contracts::combined_registry()
        .routes()
        .iter()
        .find(|route| route.operation_name == operation)
        .cloned()
        .unwrap_or_else(|| panic!("IAM operation {operation} has no contract"))
}

fn session_config<S: AsyncIamSessions + 'static>(service: &Arc<S>) -> RouterConfig {
    let verifier = Arc::clone(service);
    RouterConfig::default().with_authenticator(move |request, _| {
        let service = Arc::clone(&verifier);
        async move {
            let token = request
                .headers
                .get("authorization")
                .and_then(|header| parse_bearer(header))
                .filter(|token| token.len() <= 4096)
                .ok_or(AuthError::MissingCredentials)?;
            authenticate_stored_session(service.as_ref(), token).await
        }
    })
}

async fn authenticate_stored_session<S: AsyncIamSessions>(
    service: &S,
    token: &str,
) -> Result<AuthenticatedSession, AuthError> {
    if token.is_empty() || token.len() > 4096 {
        return Err(AuthError::InvalidCredentials);
    }
    let session = service
        .validate_session(token)
        .await
        .map_err(|error| auth_backend_error(&error))?
        .ok_or(AuthError::InvalidCredentials)?;
    let user = service
        .get_user(&session.user_id)
        .await
        .map_err(|error| auth_backend_error(&error))?
        .filter(|user| !user.disabled)
        .ok_or(AuthError::InvalidCredentials)?;
    let permissions = service
        .permissions_for_user(&user.id)
        .await
        .map_err(|error| auth_backend_error(&error))?;
    let mut principal = Principal::new(&user.id);
    principal.roles = user.roles;
    principal.permissions = permissions
        .into_iter()
        .map(|p| format!("{}:{}", p.resource, p.action))
        .collect();
    let claims = serde_json::json!({"sub":principal.subject, "roles":principal.roles,
                "permissions":principal.permissions, "exp":session.expires_at_unix_seconds});
    Ok(AuthenticatedSession::new(principal, claims))
}

async fn login_handler<S: AsyncIamSessions>(
    service: Arc<S>,
    headers: HeaderMap,
    credentials: Result<Json<Credentials>, JsonRejection>,
) -> Response {
    let Ok(Json(credentials)) = credentials else {
        return invalid_input(&headers);
    };
    match service.authenticate(credentials).await {
        Ok(session) => {
            let mut response = Json(session).into_response();
            response.headers_mut().insert(
                axum::http::header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store"),
            );
            response
        }
        Err(error) if error.code == "iam_invalid_credentials" => {
            response_error(ErrorKind::Unauthorized, "invalid credentials", &headers)
        }
        Err(error) => backend_error(&error, &headers),
    }
}

async fn logout_handler<S: AsyncIamSessions>(service: Arc<S>, headers: HeaderMap) -> Response {
    let Some(token) = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_bearer)
    else {
        return response_error(ErrorKind::Unauthorized, "missing credentials", &headers);
    };
    match service.revoke_session(token).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => backend_error(&error, &headers),
    }
}

async fn user_handler<S: AsyncIamSessions>(
    service: Arc<S>,
    headers: HeaderMap,
    id: String,
) -> Response {
    match service.get_user(&id).await {
        Ok(Some(user)) => Json(user).into_response(),
        Ok(None) => response_error(ErrorKind::NotFound, "user not found", &headers),
        Err(error) => backend_error(&error, &headers),
    }
}

async fn users_handler<S: AsyncIamSessions>(
    service: Arc<S>,
    headers: HeaderMap,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Response {
    let Ok(request) = page_request(query) else {
        return invalid_input(&headers);
    };
    match service.list_users(request).await {
        Ok(page) => Json(page).into_response(),
        Err(error) => backend_error(&error, &headers),
    }
}

async fn permissions_handler<S: AsyncIamSessions>(
    service: Arc<S>,
    headers: HeaderMap,
    id: String,
) -> Response {
    match service.permissions_for_user(&id).await {
        Ok(permissions) => Json(permissions).into_response(),
        Err(error) => backend_error(&error, &headers),
    }
}

async fn organizations_handler<S: AsyncIamSessions>(
    service: Arc<S>,
    headers: HeaderMap,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Response {
    let Ok(request) = page_request(query) else {
        return invalid_input(&headers);
    };
    match service.list_organizations(request).await {
        Ok(page) => Json(page).into_response(),
        Err(error) => backend_error(&error, &headers),
    }
}

#[derive(serde::Deserialize)]
struct PageQuery {
    limit: Option<usize>,
    cursor: Option<String>,
}

fn page_request(query: Result<Query<PageQuery>, QueryRejection>) -> Result<PageRequest, ()> {
    let Query(query) = query.map_err(|_| ())?;
    let limit = query.limit.unwrap_or(100);
    if !(1..=1000).contains(&limit) {
        return Err(());
    }
    Ok(PageRequest {
        limit,
        cursor: query.cursor,
    })
}

fn invalid_input(headers: &HeaderMap) -> Response {
    response_error(ErrorKind::Validation, "invalid request", headers)
}

fn response_error(kind: ErrorKind, message: &str, headers: &HeaderMap) -> Response {
    let correlation = headers
        .get("x-correlation-id")
        .or_else(|| headers.get("x-request-id"))
        .and_then(|header| header.to_str().ok())
        .unwrap_or("");
    forklaunch_axum::router::framework_response_to_axum(
        ErrorResponse::new(kind, message)
            .with_correlation_id(correlation)
            .to_response(),
    )
}

fn backend_error(error: &BlueprintError, headers: &HeaderMap) -> Response {
    tracing::error!(code = %error.code, "IAM backend operation failed");
    response_error(ErrorKind::Internal, "internal server error", headers)
}

fn auth_backend_error(error: &BlueprintError) -> AuthError {
    tracing::error!(code = %error.code, "IAM session backend operation failed");
    AuthError::InvalidCredentials
}
