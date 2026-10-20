//! Authenticated runtime for the current CAC skeleton. No clinical entities are
//! registered upstream yet: authorized compliance requests therefore return 404.
#![forbid(unsafe_code)]
use axum::{
    Json,
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use forklaunch_axum::{ForkLaunchRouter, RouterConfig};
use forklaunch_cac_interface::{CodeSetProvider, ProcedureCodeLookupDto, contracts};
use forklaunch_core::ServiceInfo;
use std::sync::Arc;

pub use forklaunch_axum::module_auth::HmacJwksAuth as CacAuthConfig;

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
    use forklaunch_cac_base::MockProcedureCodeProvider;
    use forklaunch_core::auth::{jwt::RemoteJwks, request_hmac::SignedBody};
    use forklaunch_core::auth::{jwt::RemoteJwksConfig, request_hmac::generate_hmac_auth_header};
    use jsonwebtoken::{Algorithm, Validation};
    use std::time::SystemTime;
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
}
