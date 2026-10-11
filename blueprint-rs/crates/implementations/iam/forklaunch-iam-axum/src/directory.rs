//! IAM directory routes share HTTP/MCP validation, authentication and audited storage.
use axum::{
    Extension, Json,
    extract::{Path, RawQuery},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use forklaunch_axum::{
    ForkLaunchRouter, RouterConfig, module_auth::HmacJwksAuth, session::AuthenticatedSession,
};
use forklaunch_blueprint_core::{
    AuditContext, BlueprintError, BlueprintResult,
    contract::{AuthConfig, HttpMethod, RouteContract},
};
use forklaunch_core::ServiceInfo;
use forklaunch_iam_base::directory::{postgres::PostgresDirectoryStore, service::BaseIamDirectory};
use forklaunch_iam_interface::{
    AsyncIamSessions, module_contracts,
    services::{OrganizationService, PermissionService, RoleService, UserService},
};
use serde_json::{Value, json};
use std::sync::Arc;

/// Combines opaque session authentication with configured IAM directory CRUD.
/// Directory reads use JWKS JWTs; internal operations use request HMAC signatures.
/// # Panics
/// Only inconsistent package-owned route metadata can panic.
#[must_use]
pub fn iam_router<S: AsyncIamSessions + 'static>(
    sessions: Arc<S>,
    store: PostgresDirectoryStore,
    auth: HmacJwksAuth,
    info: ServiceInfo,
) -> ForkLaunchRouter {
    let opaque = super::session_config(&sessions)
        .authenticator
        .expect("session verifier exists");
    let auth = Arc::new(auth);
    let config = RouterConfig::default().with_authenticator(move |request, contract| {
        let opaque = opaque.clone();
        let auth = auth.clone();
        async move {
            if matches!(contract.auth, Some(AuthConfig::Bearer)) {
                opaque(request, contract).await
            } else {
                auth.authenticate(request, contract).await
            }
        }
    });
    add_routes(super::build_session_router(sessions, info, true), store).with_config(config)
}
#[allow(clippy::too_many_lines)]
fn add_routes(mut router: ForkLaunchRouter, store: PostgresDirectoryStore) -> ForkLaunchRouter {
    let store = Arc::new(store);
    for contract in module_contracts::registry().routes() {
        let handler_contract = contract.clone();
        let store = store.clone();
        router = match contract.method {
            HttpMethod::Post => router.post(
                contract.clone(),
                move |Extension(session): Extension<AuthenticatedSession>,
                      Json(input): Json<Value>| {
                    dispatch(store.clone(), session, handler_contract.clone(), input)
                },
            ),
            HttpMethod::Put => router.put(
                contract.clone(),
                move |Extension(session): Extension<AuthenticatedSession>,
                      Json(input): Json<Value>| {
                    dispatch(store.clone(), session, handler_contract.clone(), input)
                },
            ),
            HttpMethod::Get if contract.request.params.is_empty() => router.get(
                contract.clone(),
                move |Extension(session): Extension<AuthenticatedSession>,
                      RawQuery(query): RawQuery| {
                    let store = store.clone();
                    let contract = handler_contract.clone();
                    async move {
                        match query_input(query.as_deref()) {
                            Ok(input) => dispatch(store, session, contract, input).await,
                            Err(error) => failure(&error),
                        }
                    }
                },
            ),
            HttpMethod::Get => router.get(
                contract.clone(),
                move |Extension(session): Extension<AuthenticatedSession>,
                      Path(id): Path<String>| {
                    dispatch(
                        store.clone(),
                        session,
                        handler_contract.clone(),
                        json!({"id":id}),
                    )
                },
            ),
            HttpMethod::Delete if contract.request.params.is_empty() => router.delete(
                contract.clone(),
                move |Extension(session): Extension<AuthenticatedSession>,
                      RawQuery(query): RawQuery| {
                    let store = store.clone();
                    let contract = handler_contract.clone();
                    async move {
                        match query_input(query.as_deref()) {
                            Ok(input) => dispatch(store, session, contract, input).await,
                            Err(error) => failure(&error),
                        }
                    }
                },
            ),
            HttpMethod::Delete => router.delete(
                contract.clone(),
                move |Extension(session): Extension<AuthenticatedSession>,
                      Path(id): Path<String>| {
                    dispatch(
                        store.clone(),
                        session,
                        handler_contract.clone(),
                        json!({"id":id}),
                    )
                },
            ),
            _ => panic!("Unsupported package-owned IAM method"),
        };
    }
    router
}
fn query_input(query: Option<&str>) -> BlueprintResult<Value> {
    let mut result = json!({});
    for (key, value) in form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        if key == "ids" {
            if result.get("ids").is_some() {
                return Err(invalid());
            }
            let ids: Vec<String> = serde_json::from_str(&value).map_err(|_| invalid())?;
            result["ids"] = json!(ids);
        }
    }
    if result.get("ids").is_none() {
        return Err(invalid());
    }
    Ok(result)
}
async fn dispatch(
    store: Arc<PostgresDirectoryStore>,
    session: AuthenticatedSession,
    contract: RouteContract,
    input: Value,
) -> Response {
    match call(&store, &session, &contract, input).await {
        Ok(response) => response,
        Err(error) => failure(&error),
    }
}
async fn call(
    store: &PostgresDirectoryStore,
    session: &AuthenticatedSession,
    contract: &RouteContract,
    input: Value,
) -> BlueprintResult<Response> {
    let store = store.with_audit_context(AuditContext::actor(
        &session.principal.subject,
        uuid::Uuid::new_v4().to_string(),
    ))?;
    let mut service = BaseIamDirectory::new(Arc::new(store));
    let operation = contract.operation_name.as_str();
    if matches!(contract.auth, Some(AuthConfig::Jwt(_))) && contract.path.starts_with("/user") {
        let organization = service
            .get_organization_id_by_user_id(&session.principal.subject)
            .await?
            .ok_or_else(|| {
                BlueprintError::new(
                    "iam_directory_organization_required",
                    "Unauthorized: missing organization context",
                )
            })?;
        // Resolve organization membership from trusted storage rather than caller claims.
        service = service.with_organization_scope(organization);
    }
    let value = execute(&service, operation, input).await?;
    if contract.method == HttpMethod::Get
        || operation == "createOrganization"
        || operation == "updateOrganization"
    {
        let status = if contract.method == HttpMethod::Post {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        };
        return Ok((status, Json(value)).into_response());
    }
    let (verb, ending, status) = if operation.starts_with("create") {
        ("created", "successfully", StatusCode::CREATED)
    } else if operation.starts_with("update") {
        ("updated", "successfully", StatusCode::OK)
    } else {
        ("deleted", "successfully", StatusCode::OK)
    };
    let family = if operation.contains("Permissions") {
        "permissions"
    } else if operation.contains("Roles") {
        "roles"
    } else if operation.contains("Users") {
        "users"
    } else if operation.contains("Permission") {
        "Permission"
    } else if operation.contains("Role") {
        "Role"
    } else if operation.contains("User") {
        "User"
    } else {
        "Organization"
    };
    let message = if operation.contains("Batch") {
        format!("Batch {family} {verb} {ending}")
    } else {
        format!("{family} {verb} {ending}")
    };
    Ok((status, message).into_response())
}
#[allow(clippy::too_many_lines)]
async fn execute<S: UserService + RoleService + PermissionService + OrganizationService>(
    service: &S,
    operation: &str,
    input: Value,
) -> BlueprintResult<Value> {
    let id = input["id"].as_str().unwrap_or("");
    let ids: Vec<String> = input
        .get("ids")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| invalid())?
        .unwrap_or_default();
    macro_rules! request {
        ($method:ident) => {
            serde_json::to_value(
                service
                    .$method(serde_json::from_value(input.clone()).map_err(|_| invalid())?)
                    .await?,
            )
            .map_err(|_| invalid())
        };
    }
    macro_rules! get {
        ($method:ident) => {
            serde_json::to_value(service.$method(id).await?).map_err(|_| invalid())
        };
    }
    macro_rules! batch {
        ($method:ident) => {
            serde_json::to_value(service.$method(&ids).await?).map_err(|_| invalid())
        };
    }
    match operation {
        "createPermission" => request!(create_permission),
        "getPermission" => get!(get_permission),
        "updatePermission" => request!(update_permission),
        "deletePermission" => get!(delete_permission),
        "createBatchPermissions" => request!(create_batch_permissions),
        "getBatchPermissions" => batch!(get_batch_permissions),
        "updateBatchPermissions" => request!(update_batch_permissions),
        "deleteBatchPermissions" => batch!(delete_batch_permissions),
        "createRole" => request!(create_role),
        "getRole" => get!(get_role),
        "updateRole" => request!(update_role),
        "deleteRole" => get!(delete_role),
        "createBatchRoles" => request!(create_batch_roles),
        "getBatchRoles" => batch!(get_batch_roles),
        "updateBatchRoles" => request!(update_batch_roles),
        "deleteBatchRoles" => batch!(delete_batch_roles),
        "createUser" => request!(create_user),
        "getUser" => get!(get_user),
        "updateUser" => request!(update_user),
        "deleteUser" => {
            serde_json::to_value(service.delete_user(id, None).await?).map_err(|_| invalid())
        }
        "createBatchUsers" => request!(create_batch_users),
        "getBatchUsers" => {
            serde_json::to_value(service.get_batch_users(&ids, None).await?).map_err(|_| invalid())
        }
        "updateBatchUsers" => request!(update_batch_users),
        "deleteBatchUsers" => serde_json::to_value(service.delete_batch_users(&ids, None).await?)
            .map_err(|_| invalid()),
        "createOrganization" => request!(create_organization),
        "getOrganization" => get!(get_organization),
        "updateOrganization" => request!(update_organization),
        "deleteOrganization" => get!(delete_organization),
        "surfaceUserRoles" => get!(surface_roles),
        "surfaceUserPermissions" => get!(surface_permissions),
        _ => Err(invalid()),
    }
}
fn invalid() -> BlueprintError {
    BlueprintError::new("iam_directory_invalid", "Invalid IAM directory input")
}
fn failure(error: &BlueprintError) -> Response {
    let status = match error.code.as_str() {
        "iam_directory_invalid" => StatusCode::BAD_REQUEST,
        "iam_directory_not_found" => StatusCode::NOT_FOUND,
        "iam_directory_conflict" => StatusCode::CONFLICT,
        "iam_directory_forbidden" => StatusCode::FORBIDDEN,
        "iam_directory_organization_required" => StatusCode::UNAUTHORIZED,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, error.message.clone()).into_response()
}
