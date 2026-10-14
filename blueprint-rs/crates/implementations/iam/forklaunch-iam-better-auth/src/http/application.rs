//! The seven configured application contracts share HTTP/MCP validation and
//! authentication. Raw Better Auth routes remain under their internal namespace.
use super::AuthHttp;
use crate::{
    compliance::ComplianceService,
    store::AuthStore,
    surfacing::{AuthorizationScope, SurfacingService},
};
use axum::{
    Json,
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use forklaunch_auth::AuthError;
use forklaunch_axum::{ForkLaunchRouter, RouterConfig, module_auth::HmacJwksAuth};
use forklaunch_blueprint_core::{
    BlueprintError,
    contract::{
        AccessLevel, AuthConfig, BodyContract, BodyKind, ContractRegistry, FieldContract, HmacAuth,
        JwtAuthConfig, RbacConfig, RouteContract, SchemaRef, ServiceContract,
    },
};
use forklaunch_core::ServiceInfo;
use serde_json::{Value, json};
use std::sync::Arc;
pub(crate) mod redirects;

/// Configured Better Auth application metadata, separate from the raw auth API.
#[must_use]
pub fn registry(name: &str, version: &str) -> ContractRegistry {
    let mut registry = ContractRegistry::for_service(ServiceContract::new(name, version));
    let properties = [
        "email",
        "username",
        "magicLink",
        "emailOtp",
        "phoneNumber",
        "passkey",
        "anonymous",
        "sso",
        "genericOAuth",
        "twoFactor",
    ]
    .into_iter()
    .map(|name| (name.to_owned(), json!({"type":"boolean"})))
    .chain([(
        "socialProviders".into(),
        json!({"type":"array","items":{"type":"string"}}),
    )])
    .collect::<serde_json::Map<_, _>>();
    registry.register(
        RouteContract::get("GetAuthMethods", "Get configured authentication methods", "/discovery/auth-methods")
            .with_response(200, Some(body("AuthMethods", &json!({
                "type":"object", "required":properties.keys().collect::<Vec<_>>(), "properties":properties
            })))),
    );
    register_surfacing(&mut registry);
    for (name, path, erase, schema) in [
        (
            "ExportUserData",
            "/compliance/export/:userId",
            false,
            json!({"type":"object","required":["userId","entities"],"properties":{"userId":{"type":"string"},"entities":{"type":"object","additionalProperties":{}}}}),
        ),
        (
            "EraseUserData",
            "/compliance/erase/:userId",
            true,
            json!({"type":"object","required":["entitiesAffected","recordsDeleted"],"properties":{"entitiesAffected":{"type":"array","items":{"type":"string"}},"recordsDeleted":{"type":"number"}}}),
        ),
    ] {
        let route = if erase {
            RouteContract::delete(
                name,
                "Erases all PII/PHI/PCI data for a user from IAM entities (GDPR Art. 17)",
                path,
            )
        } else {
            RouteContract::get(
                name,
                "Exports all PII/PHI/PCI data for a user from IAM entities (GDPR Art. 20)",
                path,
            )
        };
        registry.register(
            route
                .with_access(AccessLevel::Protected)
                .with_auth(AuthConfig::jwt(JwtAuthConfig::JwksUrl {
                    url: "${JWKS_PUBLIC_KEY_URL}".into(),
                }))
                .with_rbac(RbacConfig {
                    allowed_roles: vec!["system".into()],
                    ..RbacConfig::default()
                })
                .with_param(FieldContract::required("userId", "String"))
                .with_response(200, Some(body(name, &schema)))
                .with_response(404, Some(text())),
        );
    }
    registry
}
fn operation(registry: &ContractRegistry, name: &str) -> RouteContract {
    registry
        .routes()
        .iter()
        .find(|route| route.name == name)
        .cloned()
        .expect("configured application contract")
}

fn register_surfacing(registry: &mut ContractRegistry) {
    for (name, path, field, summary, scoped) in [
        (
            "Surface User Roles",
            "surface-roles",
            "name",
            "Retired user-only lookup; returns no roles until the client is upgraded",
            false,
        ),
        (
            "Surface User Permissions",
            "surface-permissions",
            "slug",
            "Retired user-only lookup; returns no permissions until the client is upgraded",
            false,
        ),
        (
            "Surface Scoped User Roles",
            "surface-roles",
            "name",
            "Surface current membership for the exact authenticated session and organization",
            true,
        ),
        (
            "Surface Scoped User Permissions",
            "surface-permissions",
            "slug",
            "Surface current permissions for the exact authenticated session and organization",
            true,
        ),
    ] {
        let parameter = FieldContract::required("id", "String");
        let path = if scoped {
            format!("/user/:id/organizations/:organizationId/sessions/:sessionId/{path}")
        } else {
            format!("/user/:id/{path}")
        };
        let mut route = RouteContract::get(name, summary, path)
                .with_access(AccessLevel::Internal)
                .with_auth(AuthConfig::Hmac(HmacAuth::new().with_header_name("authorization")))
                .with_param(parameter)
                .with_response(200, Some(body(&name.replace(' ', ""), &json!({
                    "type":"array", "items":{"type":"object", "required":[field], "properties":{field:{"type":"string"}}}
                }))))
                .with_response(500, Some(text()));
        if scoped {
            route = route
                .with_param(FieldContract::required("organizationId", "String"))
                .with_param(FieldContract::required("sessionId", "String"));
        }
        registry.register(route);
    }
}

fn body(name: &str, schema: &Value) -> BodyContract {
    let mut body = BodyContract::json(name);
    body.schema.json_schema = Some(schema.to_string());
    body
}
fn text() -> BodyContract {
    BodyContract {
        kind: BodyKind::Text,
        schema: SchemaRef {
            name: "String".into(),
            json_schema: Some(json!({"type":"string"}).to_string()),
        },
        content_type: Some("text/plain".into()),
    }
}

impl<S: AuthStore + 'static> AuthHttp<S> {
    /// Raw auth API plus the configured test-callback and OAuth popup helpers.
    /// The helpers use the same trusted origin policy and internal auth handlers.
    pub fn router_with_redirects(self) -> axum::Router {
        let settings = redirects::Settings::new(&self);
        settings.mount(self.router())
    }
    /// Combines seven application operations with the raw auth namespace. Roles
    /// resolve from the trusted store, following configured TS callbacks.
    /// # Panics
    /// Only missing package-owned metadata can panic.
    #[must_use]
    pub fn application_router(self, auth: HmacJwksAuth, info: ServiceInfo) -> ForkLaunchRouter {
        let config = self.application_config(auth);
        let store = self.store.clone();
        let contracts = registry(&info.name, &info.version);
        let contract = |name| operation(&contracts, name);
        let scoped_roles = store.clone();
        let scoped_roles_clock = self.clock.clone();
        let scoped_permissions = store.clone();
        let scoped_permissions_clock = self.clock.clone();
        let exporter = store.clone();
        let eraser = store;
        let router = ForkLaunchRouter::new(info)
            .get(contract("GetAuthMethods"), || async { Json(discovery()) })
            .get(contract("Surface User Roles"), || async { Json(json!([])) })
            .get(contract("Surface User Permissions"), || async {
                Json(json!([]))
            })
            .get(
                contract("Surface Scoped User Roles"),
                move |Path((user_id, organization_id, session_id)): Path<(
                    String,
                    String,
                    String,
                )>| {
                    let store = scoped_roles.clone();
                    let now = scoped_roles_clock();
                    async move {
                        scoped_surface(
                            store,
                            AuthorizationScope {
                                user_id,
                                organization_id,
                                session_id,
                            },
                            now,
                            false,
                        )
                        .await
                    }
                },
            )
            .get(
                contract("Surface Scoped User Permissions"),
                move |Path((user_id, organization_id, session_id)): Path<(
                    String,
                    String,
                    String,
                )>| {
                    let store = scoped_permissions.clone();
                    let now = scoped_permissions_clock();
                    async move {
                        scoped_surface(
                            store,
                            AuthorizationScope {
                                user_id,
                                organization_id,
                                session_id,
                            },
                            now,
                            true,
                        )
                        .await
                    }
                },
            )
            .get(
                contract("ExportUserData"),
                move |Path(user): Path<String>| {
                    let store = exporter.clone();
                    async move {
                        match ComplianceService::new(store).export(&user).await {
                            Ok(result) if result.entities.is_empty() => (
                                StatusCode::NOT_FOUND,
                                "User not found or no PII data to export",
                            )
                                .into_response(),
                            Ok(result) => Json(result).into_response(),
                            Err(error) => failure(&error),
                        }
                    }
                },
            )
            .delete(
                contract("EraseUserData"),
                move |Path(user): Path<String>| {
                    let store = eraser.clone();
                    async move {
                        match ComplianceService::new(store).erase(&user).await {
                            Ok(result) if result.records_deleted == 0 => (
                                StatusCode::NOT_FOUND,
                                "User not found or no PII data to erase",
                            )
                                .into_response(),
                            Ok(result) => Json(result).into_response(),
                            Err(error) => failure(&error),
                        }
                    }
                },
            )
            .with_config(config);
        self.mount_raw_auth(router)
    }
    fn mount_raw_auth(self, router: ForkLaunchRouter) -> ForkLaunchRouter {
        let base_path = self.auth_base_path();
        if base_path.is_empty() {
            let namespaces = root_namespaces(self.jwks_path());
            router.merge_internal(self.router_with_redirects(), namespaces)
        } else {
            router.nest_internal(base_path, self.router_with_redirects())
        }
    }
    fn application_config(&self, auth: HmacJwksAuth) -> RouterConfig {
        let cors = self.cors_layer();
        let roles_store = self.store.clone();
        let roles_clock = self.clock.clone();
        let auth = Arc::new(auth.with_role_resolver(move |claims| {
            let store = roles_store.clone();
            let now = roles_clock();
            async move {
                let Some(scope) = AuthorizationScope::from_payload(&claims) else {
                    return Ok(vec![]);
                };
                let role = SurfacingService::new(store)
                    .surface_role_scoped(&scope, now)
                    .await
                    .map_err(|_| AuthError::InvalidCredentials)?;
                Ok(role
                    .as_str()
                    .filter(|role| !role.is_empty())
                    .map(|role| vec![role.to_owned()])
                    .unwrap_or_default())
            }
        }));
        let mut config = RouterConfig::default().with_authenticator(move |request, contract| {
            let auth = auth.clone();
            async move { auth.authenticate(request, contract).await }
        });
        config.cors = Some(cors);
        config
            .docs_sources
            .push(forklaunch_axum::ScalarSource::inline(
                "BetterAuth",
                super::public::openapi_schema(&self.auth_base_url(), self.jwks_path()),
            ));
        config
    }
}

async fn scoped_surface<S: AuthStore>(
    store: Arc<S>,
    scope: AuthorizationScope,
    now: chrono::DateTime<chrono::Utc>,
    permissions: bool,
) -> Response {
    let service = SurfacingService::new(store);
    if permissions {
        match service.surface_permissions_scoped(&scope, now).await {
            Ok(values) => Json(
                values
                    .into_iter()
                    .map(|value| json!({"slug":value}))
                    .collect::<Vec<_>>(),
            )
            .into_response(),
            Err(error) => failure(&error),
        }
    } else {
        match service.surface_role_scoped(&scope, now).await {
            Ok(value) => Json(if crate::keyring::verify::truthy(&value) {
                json!([{"name":value}])
            } else {
                json!([])
            })
            .into_response(),
            Err(error) => failure(&error),
        }
    }
}

fn root_namespaces(jwks_path: &str) -> Vec<String> {
    // The fixed preset's OpenAPI paths supply the raw namespaces. These public
    // metadata/helper routes may be hidden by the upstream OpenAPI generator.
    let schema = super::public::openapi_schema("http://auth.invalid", jwks_path);
    schema["paths"]
        .as_object()
        .expect("embedded auth paths")
        .keys()
        .map(String::as_str)
        .chain([
            "/open-api",
            "/error",
            "/reference",
            "/test-callback",
            "/oauth-redirect",
        ])
        .map(|path| format!("/{}", path.split('/').nth(1).expect("absolute auth path")))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn discovery() -> Value {
    json!({"email":true,"username":false,"magicLink":false,"emailOtp":false,"phoneNumber":false,"passkey":false,"anonymous":false,"sso":false,"genericOAuth":false,"twoFactor":false,"socialProviders":["google"]})
}
fn failure(error: &BlueprintError) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, error.message.clone()).into_response()
}

#[cfg(test)]
mod tests;
