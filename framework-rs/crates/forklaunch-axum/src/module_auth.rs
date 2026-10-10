//! Shared application-owned HMAC and asymmetric JWKS authentication policy.
use crate::session::AuthenticatedSession;
use forklaunch_auth::{
    AuthError, JwtClaims, Principal,
    jwt::RemoteJwks,
    parse_bearer,
    request_hmac::{HmacAuthorization, SignedBody},
};
use forklaunch_contract::{AuthConfig, RouteContract};
use forklaunch_http::{Body, FrameworkRequest};
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
pub struct HmacJwksAuth {
    secret: Vec<u8>,
    jwks: RemoteJwks,
    validation: Validation,
    roles: RolesResolver,
    max_skew: Duration,
}

impl HmacJwksAuth {
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

    /// Verifies the credential declared by the selected route.
    /// # Errors
    /// Returns a sanitized credential/policy error.
    pub async fn authenticate(
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

#[cfg(test)]
mod tests {
    use super::*;
    use forklaunch_auth::jwt::RemoteJwksConfig;
    const SECRET: &[u8] = b"module-test-secret";
    fn jwks() -> RemoteJwks {
        RemoteJwks::new(RemoteJwksConfig::new("http://127.0.0.1:9/jwks")).unwrap()
    }
    #[tokio::test]
    async fn role_resolver_retains_application_claims_for_organization_scoping() {
        let config = HmacJwksAuth::new(SECRET.to_vec(), jwks(), Validation::new(Algorithm::RS256))
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
            HmacJwksAuth::new(SECRET.to_vec(), jwks(), Validation::new(Algorithm::HS256)).is_err()
        );
        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_exp = false;
        assert!(HmacJwksAuth::new(SECRET.to_vec(), jwks(), validation).is_err());
    }
}

#[cfg(test)]
mod head_auth_tests {
    use super::*;
    use crate::{ForkLaunchRouter, RouterConfig};
    use axum::{
        body::{Body as AxumBody, to_bytes},
        http::Request,
    };
    use forklaunch_auth::{jwt::RemoteJwksConfig, request_hmac::generate_hmac_auth_header};
    use forklaunch_contract::{HmacAuth, RouteContract};
    use forklaunch_core::ServiceInfo;
    use forklaunch_http::{FrameworkResponse, LocalRouter};
    use tower::ServiceExt;
    const SECRET: &[u8] = b"head-hmac-fixture-secret";
    fn route() -> RouteContract {
        RouteContract::get("Read", "Read", "/read")
            .with_auth(AuthConfig::Hmac(HmacAuth::new()))
            .with_response(200, None)
    }
    fn sign(method: &str) -> String {
        let now = chrono::DateTime::<chrono::Utc>::from(SystemTime::now())
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        generate_hmac_auth_header(
            SECRET,
            "default",
            method,
            "/read",
            SignedBody::Absent,
            &now,
            "nonce",
        )
        .unwrap()
    }
    fn config() -> RouterConfig {
        let auth = Arc::new(
            HmacJwksAuth::new(
                SECRET.to_vec(),
                RemoteJwks::new(RemoteJwksConfig::new("http://127.0.0.1:9/jwks")).unwrap(),
                Validation::new(Algorithm::RS256),
            )
            .unwrap(),
        );
        RouterConfig::default().with_authenticator(move |request, contract| {
            let auth = Arc::clone(&auth);
            async move { auth.authenticate(request, contract).await }
        })
    }
    #[tokio::test]
    async fn implicit_head_signs_head_for_native_and_local_handlers() {
        let native = ForkLaunchRouter::anonymous()
            .get(route(), || async { "response" })
            .with_config(config())
            .into_router();
        let mut local = LocalRouter::new();
        local.register(route(), |request: FrameworkRequest| {
            assert!(matches!(
                request.method,
                forklaunch_contract::HttpMethod::Get | forklaunch_contract::HttpMethod::Head
            ));
            Ok(FrameworkResponse::text(200, "response"))
        });
        let local =
            ForkLaunchRouter::from_local_router(ServiceInfo::new("head", "1"), [route()], local)
                .with_config(config())
                .into_router();
        for app in [native, local] {
            for (method, signature, expected) in [
                ("HEAD", "HEAD", 200),
                ("HEAD", "GET", 401),
                ("GET", "GET", 200),
                ("GET", "HEAD", 401),
            ] {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method)
                            .uri("/read")
                            .header("authorization", sign(signature))
                            .header("x-expected-method", method)
                            .body(AxumBody::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status().as_u16(), expected);
                let body = to_bytes(response.into_body(), 4096).await.unwrap();
                if method == "HEAD" {
                    assert!(body.is_empty());
                }
            }
        }
    }
}
