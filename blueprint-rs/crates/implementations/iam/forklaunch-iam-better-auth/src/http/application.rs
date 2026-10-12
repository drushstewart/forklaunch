//! The five configured application contracts share HTTP/MCP validation and
//! authentication. Raw Better Auth routes remain under their internal namespace.
use super::AuthHttp;
use crate::{compliance::ComplianceService, store::AuthStore, surfacing::SurfacingService};
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
mod redirects;

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
    for (name, path, field, summary) in [
        (
            "Surface User Roles",
            "surface-roles",
            "name",
            "Surfaces the org-scoped roles for a user in their active organization",
        ),
        (
            "Surface User Permissions",
            "surface-permissions",
            "slug",
            "Surfaces the org-scoped permissions for a user in their active organization",
        ),
    ] {
        let parameter = FieldContract::required("id", "String");
        registry.register(
            RouteContract::get(name, summary, format!("/user/:id/{path}"))
                .with_access(AccessLevel::Internal)
                .with_auth(AuthConfig::Hmac(HmacAuth::new().with_header_name("authorization")))
                .with_param(parameter)
                .with_response(200, Some(body(name, &json!({
                    "type":"array", "items":{"type":"object", "required":[field], "properties":{field:{"type":"string"}}}
                }))))
                .with_response(500, Some(text())),
        );
    }
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
    /// Combines five application operations with the raw auth namespace. Roles
    /// resolve from the trusted store, following configured TS callbacks.
    /// # Panics
    /// Only missing package-owned metadata can panic.
    #[must_use]
    pub fn application_router(self, auth: HmacJwksAuth, info: ServiceInfo) -> ForkLaunchRouter {
        let config = self.application_config(auth);
        let store = self.store.clone();
        let clock = self.clock.clone();
        let contracts = registry(&info.name, &info.version);
        let contract = |name| {
            contracts
                .routes()
                .iter()
                .find(|route| route.name == name)
                .cloned()
                .expect("configured application contract")
        };
        let roles = store.clone();
        let roles_clock = clock.clone();
        let permissions = store.clone();
        let permissions_clock = clock;
        let exporter = store.clone();
        let eraser = store;
        ForkLaunchRouter::new(info)
            .get(contract("GetAuthMethods"), || async { Json(discovery()) })
            .get(
                contract("Surface User Roles"),
                move |Path(id): Path<String>| {
                    let store = roles.clone();
                    let now = roles_clock();
                    async move {
                        match SurfacingService::new(store).surface_role(&id, now).await {
                            Ok(role) => Json(if crate::keyring::verify::truthy(&role) {
                                json!([{"name":role}])
                            } else {
                                json!([])
                            })
                            .into_response(),
                            Err(error) => failure(&error),
                        }
                    }
                },
            )
            .get(
                contract("Surface User Permissions"),
                move |Path(id): Path<String>| {
                    let store = permissions.clone();
                    let now = permissions_clock();
                    async move {
                        match SurfacingService::new(store)
                            .surface_permissions(&id, now)
                            .await
                        {
                            Ok(permissions) => Json(
                                permissions
                                    .into_iter()
                                    .map(|value| json!({"slug":value}))
                                    .collect::<Vec<_>>(),
                            )
                            .into_response(),
                            Err(error) => failure(&error),
                        }
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
            .nest_internal("/api/auth", self.router_with_redirects())
            .with_config(config)
    }
    fn application_config(&self, auth: HmacJwksAuth) -> RouterConfig {
        let cors = self.cors_layer();
        let roles_store = self.store.clone();
        let roles_clock = self.clock.clone();
        let auth = Arc::new(auth.with_role_resolver(move |claims| {
            let store = roles_store.clone();
            let now = roles_clock();
            async move {
                let Some(user) = claims
                    .get("sub")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                else {
                    return Ok(vec![]);
                };
                let role = SurfacingService::new(store)
                    .surface_role(user, now)
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
    }
}
fn discovery() -> Value {
    json!({"email":true,"username":false,"magicLink":false,"emailOtp":false,"phoneNumber":false,"passkey":false,"anonymous":false,"sso":false,"genericOAuth":false,"twoFactor":false,"socialProviders":["google"]})
}
fn failure(error: &BlueprintError) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, error.message.clone()).into_response()
}

#[cfg(test)]
mod tests;
