use axum::{Extension, Json};
use forklaunch_auth::{
    AuthError, parse_bearer,
    typescript::{
        TypeScriptRbacPolicy,
        basic::{verify_basic, verify_basic_resource},
        hmac::verify_hmac,
        jwt::verify_jwt_resource,
    },
    verify_jwt_hs256,
};
use forklaunch_axum::{ForkLaunchRouter, RouterConfig, session::AuthenticatedSession};
use forklaunch_contract::{AccessLevel, AuthConfig, BodyContract, JwtAuthConfig, RouteContract};
use forklaunch_http::{ErrorKind, ErrorResponse};
use serde_json::{Value, json};
const SECRET: &[u8] = b"fixture-only-session-authorization-key";
pub fn contracts() -> Vec<RouteContract> {
    [
        "permissions",
        "scope",
        "features",
        "unconfigured",
        "basic",
        "resource",
        "jwt-resource",
        "hmac",
    ]
    .into_iter()
    .map(|name| {
        RouteContract::get(
            format!("Check {name}"),
            "Session authorization fixture",
            format!("/policy/{name}"),
        )
        .with_auth(if name == "hmac" {
            AuthConfig::hmac()
        } else if name == "basic" || name == "resource" {
            AuthConfig::basic()
        } else {
            AuthConfig::jwt(JwtAuthConfig::SignatureKey)
        })
        .with_access(if name == "hmac" {
            AccessLevel::Internal
        } else if name == "scope" || name == "features" || name == "basic" {
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
        .with_response_authenticator(|request, contract| async move {
            if contract.operation_name == "checkHmac" {
                use forklaunch_auth::request_hmac::{HmacAuthorization, SignedBody};
                let keys = std::collections::BTreeMap::from([("peer".to_owned(), "fixture-hmac-secret".to_owned())]);
                verify_hmac(request.headers.get("authorization").map(String::as_str), None, Some(&keys), |credential| async move {
                    let Some(secret) = credential.secret_key else { return Ok(false); };
                    let wire = format!("HMAC keyId={} ts={} nonce={} signature={}", credential.key_id, credential.timestamp, credential.nonce, credential.signature);
                    let Ok(parsed) = HmacAuthorization::parse(&wire) else { return Ok(false); };
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                        .map_err(|_| ErrorResponse::new(ErrorKind::Internal, "clock unavailable"))?;
                    let body = match &request.body {
                        forklaunch_http::Body::Empty => SignedBody::Absent,
                        forklaunch_http::Body::Text(text) => SignedBody::Text(text),
                        forklaunch_http::Body::Binary(bytes) => SignedBody::Bytes(bytes),
                    };
                    Ok(parsed.verify_at(secret.as_bytes(), request.method.as_str(), &request.path, body,
                        i64::try_from(now.as_millis()).unwrap_or(i64::MAX), std::time::Duration::from_secs(60)))
                }).await?;
                return Ok(AuthenticatedSession::new(forklaunch_auth::Principal::new("trusted-peer"), Value::Null));
            }
            if contract.operation_name == "checkJwtResource" {
                let claims = verify_jwt_resource(
                    request.headers.get("authorization").map(String::as_str),
                    None,
                    |token| async move {
                        tokio::task::yield_now().await;
                        let principal = verify_jwt_hs256(token, SECRET).map_err(|_| {
                            ErrorResponse::new(ErrorKind::Internal, "private verification details")
                        })?;
                        Ok(json!({"sub":principal.subject, "roles":principal.roles, "team":"jwt-verified-team"}))
                    },
                ).await?;
                let subject = claims.get("sub").and_then(Value::as_str)
                    .ok_or_else(|| ErrorResponse::new(ErrorKind::Validation, "invalid session"))?;
                let mut principal = forklaunch_auth::Principal::new(subject);
                principal.roles = serde_json::from_value(claims["roles"].clone())
                    .map_err(|_| ErrorResponse::new(ErrorKind::Validation, "invalid session"))?;
                return Ok(AuthenticatedSession::new(principal, claims));
            }
            if contract.operation_name == "checkResource" {
                let claims = verify_basic_resource(
                    request.headers.get("authorization").map(String::as_str),
                    None,
                    |token| async move {
                        tokio::task::yield_now().await;
                        Ok(match token {
                            "owned-opaque-token" => json!({"sub":"resource-user", "role":"admin", "team":"verified-team"}),
                            "owned-reader-token" => json!({"sub":"resource-user", "role":"reader", "team":"verified-team"}),
                            "owned-invalid-session" => json!({"sub":42}),
                            _ => Value::Null,
                        })
                    },
                ).await?;
                let subject = claims.get("sub").and_then(Value::as_str)
                    .ok_or_else(|| ErrorResponse::new(ErrorKind::Validation, "invalid session"))?;
                let mut principal = forklaunch_auth::Principal::new(subject);
                principal.roles = claims.get("role").and_then(Value::as_str).map(|role| vec![role.into()]).unwrap_or_default();
                return Ok(AuthenticatedSession::new(principal, claims));
            }
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
            if contract.operation_name == "checkHmac" { return Ok(()); }
            let policy = match contract.operation_name.as_str() {
                "checkResource" | "checkJwtResource" => TypeScriptRbacPolicy {
                    allowed_roles: Some(vec!["admin".into()]),
                    ..Default::default()
                },
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
                if let Some(team) = session.claims.get("team") {
                    return Json::<Value>(
                        json!({"subject":session.principal.subject, "team":team}),
                    );
                }
                Json::<Value>(json!({"subject":session.principal.subject}))
            },
        );
    }
    router
}
