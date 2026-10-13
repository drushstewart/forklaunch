use axum::{Extension, Json};
use forklaunch_auth::{
    AuthError, parse_bearer,
    typescript::{TypeScriptRbacPolicy, basic::verify_basic},
    verify_jwt_hs256,
};
use forklaunch_axum::{ForkLaunchRouter, RouterConfig, session::AuthenticatedSession};
use forklaunch_contract::{AccessLevel, AuthConfig, BodyContract, JwtAuthConfig, RouteContract};
use forklaunch_http::{ErrorKind, ErrorResponse};
use serde_json::{Value, json};
const SECRET: &[u8] = b"fixture-only-session-authorization-key";
pub fn contracts() -> Vec<RouteContract> {
    ["permissions", "scope", "features", "unconfigured", "basic"]
        .into_iter()
        .map(|name| {
            RouteContract::get(
                format!("Check {name}"),
                "Session authorization fixture",
                format!("/policy/{name}"),
            )
            .with_auth(if name == "basic" {
                AuthConfig::basic()
            } else {
                AuthConfig::jwt(JwtAuthConfig::SignatureKey)
            })
            .with_access(
                if name == "scope" || name == "features" || name == "basic" {
                    AccessLevel::Authenticated
                } else {
                    AccessLevel::Protected
                },
            )
            .with_response(200, Some(BodyContract::json("PolicyResponse")))
        })
        .collect()
}
pub fn register(mut router: ForkLaunchRouter) -> ForkLaunchRouter {
    let config = RouterConfig::default()
        .with_response_authenticator(|request, contract| async move {
            if contract.operation_name == "checkBasic" {
                let principal = verify_basic(
                    request.headers.get("authorization").map(String::as_str),
                    None,
                    |user, password| user == "fixture-user" && password == "fixture-password",
                )?;
                return Ok(AuthenticatedSession::new(principal, json!({})));
            }
            let invalid = || ErrorResponse::new(ErrorKind::Forbidden, "invalid credentials");
            let header = request
                .headers
                .get("authorization")
                .ok_or_else(|| AuthError::MissingCredentials.to_error_response())?;
            let token = parse_bearer(header).ok_or_else(invalid)?;
            let principal = verify_jwt_hs256(token, SECRET).map_err(|_| invalid())?;
            Ok(AuthenticatedSession::new(principal, json!({})))
        })
        .with_authorizer(|session, contract| {
            let policy = match contract.operation_name.as_str() {
                "checkPermissions" => TypeScriptRbacPolicy {
                    allowed_permissions: Some(vec!["read".into()]),
                    allowed_roles: Some(vec!["admin".into()]),
                    ..Default::default()
                },
                "checkScope" => TypeScriptRbacPolicy {
                    required_scope: Some("team".into()),
                    scope_hierarchy: Some(vec!["admin".into(), "team".into()]),
                    ..Default::default()
                },
                "checkFeatures" => TypeScriptRbacPolicy {
                    require_active_subscription: true,
                    required_features: vec!["reports".into()],
                    ..Default::default()
                },
                _ => TypeScriptRbacPolicy::default(),
            };
            policy.authorize(&session.principal, Some(contract.access))
        });
    router = router.with_config(config);
    for contract in contracts() {
        router = router.get(
            contract,
            |Extension(session): Extension<AuthenticatedSession>| async move {
                Json::<Value>(json!({"subject":session.principal.subject}))
            },
        );
    }
    router
}
