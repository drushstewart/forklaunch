use axum::{Extension, Json};
use forklaunch_auth::{
    AuthError, parse_bearer, typescript::TypeScriptRbacPolicy, verify_jwt_hs256,
};
use forklaunch_axum::{ForkLaunchRouter, RouterConfig, session::AuthenticatedSession};
use forklaunch_contract::{AccessLevel, AuthConfig, BodyContract, JwtAuthConfig, RouteContract};
use serde_json::{Value, json};
const SECRET: &[u8] = b"fixture-only-session-authorization-key";
pub fn contracts() -> Vec<RouteContract> {
    ["permissions", "scope", "features", "unconfigured"]
        .into_iter()
        .map(|name| {
            RouteContract::get(
                format!("Check {name}"),
                "Session authorization fixture",
                format!("/policy/{name}"),
            )
            .with_auth(AuthConfig::jwt(JwtAuthConfig::SignatureKey))
            .with_access(if name == "scope" || name == "features" {
                AccessLevel::Authenticated
            } else {
                AccessLevel::Protected
            })
            .with_response(200, Some(BodyContract::json("PolicyResponse")))
        })
        .collect()
}
pub fn register(mut router: ForkLaunchRouter) -> ForkLaunchRouter {
    let config = RouterConfig::default()
        .with_authenticator(|request, _| async move {
            let header = request
                .headers
                .get("authorization")
                .ok_or(AuthError::MissingCredentials)?;
            let token = parse_bearer(header).ok_or(AuthError::InvalidCredentials)?;
            let principal = verify_jwt_hs256(token, SECRET)?;
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
