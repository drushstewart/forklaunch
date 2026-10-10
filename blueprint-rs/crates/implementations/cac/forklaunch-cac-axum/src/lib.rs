//! Authenticated runtime for the current CAC skeleton. No clinical entities are
//! registered upstream yet: authorized compliance requests therefore return 404.
#![forbid(unsafe_code)]
use axum::{
    Json,
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use forklaunch_auth::{
    AuthError, JwtClaims, Principal,
    jwt::RemoteJwks,
    parse_bearer,
    request_hmac::{HmacAuthorization, SignedBody},
};
use forklaunch_axum::{ForkLaunchRouter, RouterConfig, session::AuthenticatedSession};
use forklaunch_blueprint_core::{
    contract::{AuthConfig, RouteContract},
    http::{Body, FrameworkRequest},
};
use forklaunch_cac_interface::{CodeSetProvider, ProcedureCodeLookupDto, contracts};
use forklaunch_core::ServiceInfo;
use jsonwebtoken::{Algorithm, Validation};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

type RolesFuture = Pin<Box<dyn Future<Output = Result<Vec<String>, AuthError>> + Send>>;
type RolesResolver = Arc<dyn Fn(serde_json::Value) -> RolesFuture + Send + Sync>;

/// Trusted server-owned keys and JWT policy. Secret bytes are never formatted.
/// Default roles come only from verified issuer claims. Configure a resolver to
/// surface roles from a trusted IAM service rather than installing a role stub.
pub struct CacAuthConfig {
    secret: Vec<u8>,
    jwks: RemoteJwks,
    validation: Validation,
    roles: RolesResolver,
    max_skew: Duration,
}

impl CacAuthConfig {
    /// # Errors
    /// Requires a nonempty HMAC secret, asymmetric JWT algorithms and expiry
    /// validation. Endpoint/cache policy is owned by the supplied JWKS verifier.
    pub fn new(
        secret: Vec<u8>,
        jwks: RemoteJwks,
        mut validation: Validation,
    ) -> Result<Self, AuthError> {
        if secret.is_empty()
            || !validation.validate_exp
            || validation.algorithms.is_empty()
            || validation.algorithms.iter().any(|algorithm| {
                matches!(
                    algorithm,
                    Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512
                )
            })
        {
            return Err(AuthError::InvalidCredentials);
        }
        validation.required_spec_claims.insert("exp".to_owned());
        Ok(Self {
            secret,
            jwks,
            validation,
            roles: Arc::new(|claims| {
                Box::pin(async move {
                    serde_json::from_value::<JwtClaims>(claims)
                        .map(|claims| claims.roles)
                        .map_err(|_| AuthError::InvalidCredentials)
                })
            }),
            max_skew: Duration::from_secs(300),
        })
    }

    #[must_use]
    pub fn with_role_resolver<F, R>(mut self, resolver: F) -> Self
    where
        F: Fn(serde_json::Value) -> R + Send + Sync + 'static,
        R: Future<Output = Result<Vec<String>, AuthError>> + Send + 'static,
    {
        self.roles = Arc::new(move |claims| Box::pin(resolver(claims)));
        self
    }

    async fn authenticate(
        &self,
        request: FrameworkRequest,
        contract: RouteContract,
    ) -> Result<AuthenticatedSession, AuthError> {
        let header = request
            .headers
            .get("authorization")
            .ok_or(AuthError::MissingCredentials)?;
        if matches!(contract.auth, Some(AuthConfig::Hmac(_))) {
            let authorization = HmacAuthorization::parse(header)?;
            let body = match &request.body {
                Body::Empty => SignedBody::Absent,
                Body::Text(text) => SignedBody::Text(text),
                Body::Binary(bytes) => SignedBody::Bytes(bytes),
            };
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| AuthError::InvalidCredentials)?;
            if authorization.key_id() != "default"
                || !authorization.verify_at(
                    &self.secret,
                    request.method.as_str(),
                    &request.path,
                    body,
                    i64::try_from(now.as_millis()).map_err(|_| AuthError::InvalidCredentials)?,
                    self.max_skew,
                )
            {
                return Err(AuthError::InvalidCredentials);
            }
            return Ok(AuthenticatedSession::new(
                Principal::new("service:default"),
                serde_json::json!({"sub":"service:default"}),
            ));
        }
        let token = parse_bearer(header)
            .filter(|token| token.len() <= 8192)
            .ok_or(AuthError::InvalidCredentials)?;
        let claims: serde_json::Value = self.jwks.verify(token, &self.validation).await?;
        let typed: JwtClaims =
            serde_json::from_value(claims.clone()).map_err(|_| AuthError::InvalidCredentials)?;
        let mut principal = Principal::from_claims(typed);
        principal.roles = (self.roles)(claims.clone()).await?;
        Ok(AuthenticatedSession::new(principal, claims))
    }
}

/// Mounts real provider handlers, protected empty-entity compliance handlers and
/// health/docs from the same registry used for offline contracts and SDKs.
///
/// # Panics
/// Panics if the package's static registry is inconsistent with its handlers.
#[must_use]
pub fn cac_router<P: CodeSetProvider + 'static>(
    provider: Arc<P>,
    auth: CacAuthConfig,
    service: ServiceInfo,
) -> ForkLaunchRouter {
    let registry = contracts::module_registry();
    let route = |name: &str| {
        registry
            .routes()
            .iter()
            .find(|route| route.operation_name == name)
            .expect("registered CAC route")
            .clone()
    };
    let describe = Arc::clone(&provider);
    let auth = Arc::new(auth);
    let config = RouterConfig::default().with_authenticator(move |request, contract| {
        let auth = Arc::clone(&auth);
        async move { auth.authenticate(request, contract).await }
    });
    let service_name = service.name.clone();
    ForkLaunchRouter::new(service)
        .get(route("describeCodeSet"), move || {
            let provider = Arc::clone(&describe);
            async move { Json(provider.describe()) }
        })
        .get(
            route("lookupProcedureCode"),
            move |Path(code): Path<String>| {
                let provider = Arc::clone(&provider);
                async move { lookup(&provider, code).await }
            },
        )
        .get(route("exportUserData"), || async {
            (
                StatusCode::NOT_FOUND,
                "User not found or no PII data to export",
            )
        })
        .delete(route("eraseUserData"), || async {
            (
                StatusCode::NOT_FOUND,
                "User not found or no PII data to erase",
            )
        })
        .get(route("getHealth"), move || {
            let service = service_name.clone();
            async move { Json(serde_json::json!({"service":service,"status":"ok"})) }
        })
        .with_config(config)
}

async fn lookup<P: CodeSetProvider>(provider: &Arc<P>, code: String) -> Response {
    match provider
        .lookup_procedure_code(ProcedureCodeLookupDto { code: code.clone() })
        .await
    {
        Ok(Some(result)) => Json(result).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            format!("Procedure code '{code}' not found"),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Code-set provider unavailable",
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body as AxumBody, http::Request};
    use forklaunch_auth::{jwt::RemoteJwksConfig, request_hmac::generate_hmac_auth_header};
    use forklaunch_cac_base::MockProcedureCodeProvider;
    use tower::ServiceExt;
    const SECRET: &[u8] = b"cac-test-hmac-secret";
    fn jwks() -> RemoteJwks {
        RemoteJwks::new(RemoteJwksConfig::new("http://127.0.0.1:9/jwks")).unwrap()
    }
    fn router() -> axum::Router {
        cac_router(
            Arc::new(MockProcedureCodeProvider::default()),
            CacAuthConfig::new(SECRET.to_vec(), jwks(), Validation::new(Algorithm::RS256)).unwrap(),
            ServiceInfo::new("cac", "0.1.0"),
        )
        .into_router()
    }
    fn header(path: &str) -> String {
        let timestamp = chrono::DateTime::<chrono::Utc>::from(SystemTime::now())
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        generate_hmac_auth_header(
            SECRET,
            "default",
            "GET",
            path,
            SignedBody::Absent,
            &timestamp,
            "test-nonce",
        )
        .unwrap()
    }
    #[tokio::test]
    async fn lookup_routes_require_valid_request_bound_hmac_and_keep_health_public() {
        let app = router();
        for path in ["/codeSet", "/codeSet/PROC-001", "/compliance/export/user"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .body(AxumBody::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        for (path, expected, value) in [
            ("/codeSet", StatusCode::OK, "\"licensed\":false"),
            ("/codeSet/PROC-001", StatusCode::OK, "Office Visit"),
            (
                "/codeSet/unknown",
                StatusCode::NOT_FOUND,
                "Procedure code 'unknown' not found",
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header("authorization", header(path))
                        .body(AxumBody::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            let body = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            assert!(String::from_utf8_lossy(&body).contains(value));
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/codeSet/PROC-002")
                    .header("authorization", header("/codeSet/PROC-001"))
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let stale = generate_hmac_auth_header(
            SECRET,
            "default",
            "GET",
            "/codeSet",
            SignedBody::Absent,
            "2020-01-01T00:00:00Z",
            "old-nonce",
        )
        .unwrap();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/codeSet")
                    .header("authorization", stale)
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    #[tokio::test]
    async fn role_resolver_retains_application_claims_for_organization_scoping() {
        let config = CacAuthConfig::new(SECRET.to_vec(), jwks(), Validation::new(Algorithm::RS256))
            .unwrap()
            .with_role_resolver(|claims| async move {
                assert_eq!(claims["activeOrganizationId"], "org-cac");
                assert_eq!(claims["sub"], "user-cac");
                Ok(vec!["system".to_owned()])
            });
        let roles =
            (config.roles)(serde_json::json!({"sub":"user-cac","activeOrganizationId":"org-cac"}))
                .await
                .unwrap();
        assert_eq!(roles, ["system"]);
    }

    #[test]
    fn jwt_policy_rejects_symmetric_algorithms_and_disabled_expiry() {
        assert!(
            CacAuthConfig::new(SECRET.to_vec(), jwks(), Validation::new(Algorithm::HS256)).is_err()
        );
        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_exp = false;
        assert!(CacAuthConfig::new(SECRET.to_vec(), jwks(), validation).is_err());
    }
}
