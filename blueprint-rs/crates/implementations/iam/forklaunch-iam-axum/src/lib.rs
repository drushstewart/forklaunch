#![forbid(unsafe_code)]

//! Contract-driven HTTP composition for an asynchronous, tenant-bound IAM backend.
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
};
use forklaunch_auth::{AuthError, Principal, parse_bearer};
use forklaunch_axum::{ForkLaunchRouter, RouterConfig, session::AuthenticatedSession};
use forklaunch_blueprint_core::{
    BlueprintError, PageRequest,
    http::{ErrorKind, ErrorResponse},
};
use forklaunch_core::ServiceInfo;
use forklaunch_iam_interface::{AsyncIamSessions, Credentials, contracts};
use std::sync::Arc;

/// Builds login, logout, user, permission, and organization routes. The store must
/// already be bound to a trusted tenant; HTTP headers never select a tenant.
/// Protected routes verify persistent sessions and resolve current user grants.
///
/// # Panics
/// Panics if the built-in IAM contract registry lacks a required operation.
#[must_use]
pub fn session_iam_router<S: AsyncIamSessions + 'static>(service: Arc<S>) -> ForkLaunchRouter {
    session_iam_router_with_service_info(service, ServiceInfo::new("iam", "0.1.0"))
}

/// Builds the same session-backed routes with application service metadata.
///
/// # Panics
/// Panics if the built-in IAM contract registry lacks a required operation.
#[must_use]
pub fn session_iam_router_with_service_info<S: AsyncIamSessions + 'static>(
    service: Arc<S>,
    info: ServiceInfo,
) -> ForkLaunchRouter {
    build_session_router(service, info, false)
}
fn build_session_router<S: AsyncIamSessions + 'static>(
    service: Arc<S>,
    info: ServiceInfo,
    combined: bool,
) -> ForkLaunchRouter {
    let registry = if combined {
        forklaunch_iam_interface::module_contracts::combined_registry()
    } else {
        contracts::session_registry()
    };
    let contract = |name: &str| {
        let name = if combined && name == "getUser" {
            "getSessionUser"
        } else {
            name
        };
        registry
            .routes()
            .iter()
            .find(|route| route.operation_name == name)
            .cloned()
            .expect("IAM contract exists")
    };
    let config = session_config(&service);
    let login = Arc::clone(&service);
    let logout = Arc::clone(&service);
    let get_user = Arc::clone(&service);
    let users = Arc::clone(&service);
    let permissions = Arc::clone(&service);
    ForkLaunchRouter::new(info)
        .get(contract("getHealth"), || async {
            Json(serde_json::json!({"status":"ok"}))
        })
        .post(
            contract("authenticate"),
            move |headers: HeaderMap, credentials: Result<Json<Credentials>, JsonRejection>| {
                login_handler(Arc::clone(&login), headers, credentials)
            },
        )
        .post(contract("logout"), move |headers: HeaderMap| {
            logout_handler(Arc::clone(&logout), headers)
        })
        .get(
            contract("getUser"),
            move |headers: HeaderMap, Path(id): Path<String>| {
                user_handler(Arc::clone(&get_user), headers, id)
            },
        )
        .get(
            contract("listUsers"),
            move |headers: HeaderMap, query: Result<Query<PageQuery>, QueryRejection>| {
                users_handler(Arc::clone(&users), headers, query)
            },
        )
        .get(
            contract("listUserPermissions"),
            move |headers: HeaderMap, Path(id): Path<String>| {
                permissions_handler(Arc::clone(&permissions), headers, id)
            },
        )
        .get(
            contract("listOrganizations"),
            move |headers: HeaderMap, query: Result<Query<PageQuery>, QueryRejection>| {
                organizations_handler(Arc::clone(&service), headers, query)
            },
        )
        .with_config(config)
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
