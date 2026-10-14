use super::*;
use crate::{http::ExternalJwtSigner, keyring::custom::JwtSigner, store::memory::MemoryAuthStore};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

fn app() -> AuthHttp<MemoryAuthStore> {
    AuthHttp::new(
        Arc::new(MemoryAuthStore::default()),
        "http://localhost:3000".into(),
        vec![42; 32],
        vec![],
    )
    .unwrap()
}
struct NeverSign;
impl JwtSigner for NeverSign {
    async fn sign(&self, _: Value) -> BlueprintResult<String> {
        panic!("JWKS must never invoke remote signing")
    }
}
#[tokio::test]
async fn fixed_jwks_paths_relocate_get_and_schema_preserving_method_and_remote_rules() {
    for path in ["/jwks", "/.well-known/jwks.json", "/keys/public", "/"] {
        for remote in [false, true] {
            let mut app = app().with_jwks_path(path).unwrap();
            assert_eq!(app.jwks_path(), path);
            if remote {
                app = app.with_external_jwt_signer(
                    ExternalJwtSigner::new("https://keys.example.test/public", "EdDSA", NeverSign)
                        .unwrap(),
                );
            }
            let router = Router::new().nest(
                if path == "/" {
                    "/api/auth/"
                } else {
                    "/api/auth"
                },
                app.router(),
            );
            for method in ["GET", "HEAD", "POST"] {
                let response = router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method)
                            .uri(format!("/api/auth{path}"))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    response.status().as_u16(),
                    if method == "GET" && !remote { 200 } else { 404 },
                    "{path} {method} remote={remote}"
                );
                if method == "HEAD" {
                    assert!(
                        to_bytes(response.into_body(), 65536)
                            .await
                            .unwrap()
                            .is_empty()
                    );
                }
            }
            if path != "/jwks" {
                let response = router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri("/api/auth/jwks")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status().as_u16(), 404);
            }
            let response = router
                .oneshot(
                    Request::builder()
                        .uri("/api/auth/open-api/generate-schema")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), 200);
            let schema: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap())
                    .unwrap();
            assert!(schema["paths"].get(path).is_some());
            if path != "/jwks" {
                assert!(schema["paths"].get("/jwks").is_none());
            }
        }
    }
}

#[test]
fn unsupported_and_owned_namespaces_fail_before_router_construction() {
    for path in [
        "",
        "keys",
        "/a..b",
        "/keys/",
        "/keys//public",
        "/keys/./public",
        "/:key",
        "/{key}",
        "/%6beys",
        "/keys?x=1",
        "/keys#f",
        "/token",
        "/token/child",
        "/organization/keys",
        "/callback/google",
        "/test-callback",
        "/user/custom",
        "/compliance/custom",
        "/discovery/custom",
        "/relay/handoff",
    ] {
        assert!(app().with_jwks_path(path).is_err(), "{path}");
    }
    assert!(
        app()
            .with_base_path("/")
            .unwrap()
            .with_jwks_path("/")
            .is_err()
    );
    assert!(
        app()
            .with_jwks_path("/")
            .unwrap()
            .with_base_path("/")
            .is_err()
    );
    assert!(
        app()
            .with_base_path("/")
            .unwrap()
            .with_jwks_path("/keys/public")
            .is_ok()
    );
}

#[cfg(feature = "application-http")]
#[tokio::test]
async fn root_auth_custom_jwks_preserves_application_authentication_and_namespace_checks() {
    use forklaunch_auth::jwt::{Algorithm, RemoteJwks, RemoteJwksConfig, Validation};
    use forklaunch_axum::module_auth::HmacJwksAuth;
    use forklaunch_core::ServiceInfo;
    let auth = HmacJwksAuth::new(
        vec![42; 32],
        RemoteJwks::new(RemoteJwksConfig::new("https://keys.example.test/public")).unwrap(),
        Validation::new(Algorithm::EdDSA),
    )
    .unwrap();
    let router = app()
        .with_base_path("/")
        .unwrap()
        .with_jwks_path("/keys/public")
        .unwrap()
        .application_router(auth, ServiceInfo::new("paths", "1"))
        .into_router();
    for (path, status) in [
        ("/keys/public", 200),
        ("/jwks", 404),
        ("/user/u/surface-roles", 401),
    ] {
        let response = router
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status, "{path}");
    }
}

#[cfg(feature = "application-http")]
#[tokio::test]
async fn root_jwks_owns_only_trailing_auth_root_and_preserves_protected_business_routes() {
    use forklaunch_auth::jwt::{Algorithm, RemoteJwks, RemoteJwksConfig, Validation};
    use forklaunch_axum::module_auth::HmacJwksAuth;
    use forklaunch_core::ServiceInfo;
    for remote in [false, true] {
        let auth = HmacJwksAuth::new(
            vec![42; 32],
            RemoteJwks::new(RemoteJwksConfig::new("https://keys.example.test/public")).unwrap(),
            Validation::new(Algorithm::EdDSA),
        )
        .unwrap();
        let mut service = app().with_jwks_path("/").unwrap();
        if remote {
            service = service.with_external_jwt_signer(
                ExternalJwtSigner::new("https://keys.example.test/public", "EdDSA", NeverSign)
                    .unwrap(),
            );
        }
        let router = service
            .application_router(auth, ServiceInfo::new("root-jwks", "1"))
            .into_router();
        for (method, path, status) in [
            ("GET", "/api/auth/", if remote { 404 } else { 200 }),
            ("HEAD", "/api/auth/", 404),
            ("POST", "/api/auth/", 404),
            ("GET", "/api/auth", 404),
            ("GET", "/api/auth/jwks", 404),
            ("GET", "/api/auth/unknown", 404),
            ("GET", "/unknown", 404),
            ("GET", "/api/auth/token", 401),
            ("GET", "/api/auth/get-session", 200),
            ("GET", "/user/u/surface-roles", 401),
            ("GET", "/api/auth/open-api/generate-schema", 200),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status().as_u16(),
                status,
                "{method} {path} remote={remote}"
            );
            if path.ends_with("generate-schema") {
                let schema: Value = serde_json::from_slice(
                    &to_bytes(response.into_body(), 1_000_000).await.unwrap(),
                )
                .unwrap();
                assert!(schema["paths"].get("/").is_some());
                assert!(schema["paths"].get("/jwks").is_none());
            }
        }
    }
}

#[tokio::test]
async fn raw_root_jwks_is_relative_but_does_not_accept_bare_nested_alias() {
    for (router, path, status) in [
        (app().with_jwks_path("/").unwrap().router(), "/", 200),
        (
            Router::new().nest("/custom", app().with_jwks_path("/").unwrap().router()),
            "/custom",
            404,
        ),
        (
            Router::new().nest("/custom/", app().with_jwks_path("/").unwrap().router()),
            "/custom/",
            200,
        ),
    ] {
        let response = router
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status, "{path}");
    }
}

#[cfg(feature = "application-http")]
#[test]
fn root_jwks_still_reserves_the_complete_auth_namespace() {
    use forklaunch_auth::jwt::{Algorithm, RemoteJwks, RemoteJwksConfig, Validation};
    use forklaunch_axum::module_auth::HmacJwksAuth;
    use forklaunch_core::ServiceInfo;
    let auth = HmacJwksAuth::new(
        vec![42; 32],
        RemoteJwks::new(RemoteJwksConfig::new("https://keys.example.test/public")).unwrap(),
        Validation::new(Algorithm::EdDSA),
    )
    .unwrap();
    let router = app()
        .with_jwks_path("/")
        .unwrap()
        .application_router(auth, ServiceInfo::new("namespace", "1"))
        .nest_internal(
            "/api/auth/business",
            Router::new().route(
                "/private",
                axum::routing::get(|| async { "must not be exposed" }),
            ),
        );
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| router.into_router())).is_err()
    );
}
