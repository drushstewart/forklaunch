//! Framework-owned public contract documentation routes.
use axum::{Router, http::header, response::Html, routing::get};
use forklaunch_core::contract::{ContractRegistry, HttpMethod};

/// Built-in HTML interface for the public `OpenAPI` document.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DocumentationUi {
    /// Scalar API Reference.
    #[default]
    Scalar,
    /// Swagger UI.
    Swagger,
}

/// Mount Scalar HTML documentation and the canonical public `OpenAPI` document.
///
/// The HTML page reads `/openapi.json`; `/openapi.json-hash` allows SDK clients
/// to detect changes. Private operations are excluded from the document.
/// Apply the application's normal security middleware to the returned router.
///
/// # Errors
/// Rejects invalid paths and collisions with declared GET/HEAD operations or
/// canonical document paths. Existing unmanaged route collisions retain Axum's
/// startup panic behavior.
pub fn with_docs<S>(
    router: Router<S>,
    registry: &ContractRegistry,
    docs_path: &str,
) -> Result<Router<S>, &'static str>
where
    S: Clone + Send + Sync + 'static,
{
    with_docs_ui(router, registry, docs_path, DocumentationUi::default())
}

/// Mount the selected HTML documentation UI and canonical public `OpenAPI` routes.
///
/// # Errors
/// Rejects invalid paths and declared GET/HEAD collisions, as [`with_docs`] does.
pub fn with_docs_ui<S>(
    router: Router<S>,
    registry: &ContractRegistry,
    docs_path: &str,
    ui: DocumentationUi,
) -> Result<Router<S>, &'static str>
where
    S: Clone + Send + Sync + 'static,
{
    validate_path(docs_path)?;
    if matches!(docs_path, "/openapi.json" | "/openapi.json-hash") {
        return Err("HTML documentation cannot replace the canonical OpenAPI routes");
    }
    if registry.routes().iter().any(|route| {
        matches!(route.method, HttpMethod::Get | HttpMethod::Head) && route.path == docs_path
    }) {
        return Err("documentation path collides with a declared operation");
    }
    let title = registry
        .service()
        .map_or("API Reference", |service| service.name.as_str());
    let html = match ui {
        DocumentationUi::Scalar => crate::docs::scalar_html(title, "/openapi.json", &[]),
        DocumentationUi::Swagger => crate::docs::swagger_html(title, "/openapi.json"),
    };
    Ok(with_openapi(router, registry, "/openapi.json")?.route(
        docs_path,
        get(move || std::future::ready(Html(html.clone()))),
    ))
}

fn validate_path(path: &str) -> Result<(), &'static str> {
    if !path.starts_with('/')
        || path.contains(['{', '}', '*', '?', '#'])
        || path.split('/').any(|segment| segment.starts_with(':'))
        || path.chars().any(char::is_control)
    {
        return Err("documentation alias must be a fixed absolute HTTP path");
    }
    Ok(())
}

/// Mount the public `OpenAPI` document at `/openapi.json` and a configured alias.
/// Private operations remain excluded by the contract exporter. The alias must
/// be a fixed absolute path; this helper never mounts application handlers.
///
/// # Errors
/// Rejects invalid aliases and collisions with declared GET/HEAD operations.
/// Existing unmanaged route collisions retain Axum's startup panic behavior.
pub fn with_openapi<S>(
    router: Router<S>,
    registry: &ContractRegistry,
    docs_path: &str,
) -> Result<Router<S>, &'static str>
where
    S: Clone + Send + Sync + 'static,
{
    validate_path(docs_path)?;
    if docs_path == "/openapi.json-hash" {
        return Err("documentation alias must be a fixed absolute HTTP path");
    }
    if registry.routes().iter().any(|route| {
        matches!(route.method, HttpMethod::Get | HttpMethod::Head)
            && [
                "/openapi.json".to_owned(),
                docs_path.to_owned(),
                "/openapi.json-hash".to_owned(),
                format!("{docs_path}-hash"),
            ]
            .contains(&route.path)
    }) {
        return Err("documentation path collides with a declared operation");
    }
    let document = forklaunch_core::openapi::generate_portable_openapi_json(registry)
        .map_err(|_| "contract cannot produce checked SDK documentation")?;
    let fingerprint = crate::docs::document_hash(&document);
    let hash = get(move || {
        let fingerprint = fingerprint.clone();
        async move { fingerprint }
    });
    let docs = get(move || {
        let document = document.clone();
        async move { ([(header::CONTENT_TYPE, "application/json")], document) }
    });
    let router = router
        .route("/openapi.json", docs.clone())
        .route("/openapi.json-hash", hash.clone());
    Ok(if docs_path == "/openapi.json" {
        router
    } else {
        router
            .route(docs_path, docs)
            .route(&format!("{docs_path}-hash"), hash)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use forklaunch_core::contract::{RouteContract, ServiceContract};
    use tower::ServiceExt as _;

    #[tokio::test]
    async fn native_html_uis_share_checked_public_document_and_escape_titles() {
        let mut registry = ContractRegistry::for_service(ServiceContract::new("notes", "1.0.0"));
        registry.register(RouteContract::new(
            "read",
            "Read",
            HttpMethod::Get,
            "/notes",
        ));
        let mut private = RouteContract::new("internal", "Internal", HttpMethod::Get, "/internal");
        private.options.openapi = false;
        registry.register(private);
        let expected = forklaunch_core::openapi::generate_portable_openapi_json(&registry).unwrap();
        for ui in [DocumentationUi::Scalar, DocumentationUi::Swagger] {
            let router = with_docs_ui(Router::new(), &registry, "/reference", ui).unwrap();
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/reference")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "text/html; charset=utf-8"
            );
            let html = String::from_utf8(
                to_bytes(response.into_body(), 64 * 1024)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap();
            assert!(html.starts_with("<!doctype html>"));
            assert!(html.contains("/openapi.json"));
            assert!(html.contains("<title>notes</title>"));
            assert!(html.contains(match ui {
                DocumentationUi::Scalar => "Scalar.createApiReference",
                DocumentationUi::Swagger => "SwaggerUIBundle",
            }));
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/openapi.json")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
            let document = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
            assert_eq!(document.as_ref(), expected.as_bytes());
            let document: serde_json::Value = serde_json::from_slice(&document).unwrap();
            assert!(document["paths"].get("/internal").is_none());
            assert_eq!(
                document["paths"]["/notes"]["get"]["x-forklaunch-operation"]["format"],
                "forklaunch.sdk-operation.v1"
            );
            let response = router
                .oneshot(
                    Request::builder()
                        .uri("/openapi.json-hash")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(!to_bytes(response.into_body(), 64).await.unwrap().is_empty());
        }
        assert!(with_docs(Router::<()>::new(), &registry, "/reference").is_ok());
        for html in [
            crate::docs::scalar_html("</title><script>unsafe()</script>", "/openapi.json", &[]),
            crate::docs::swagger_html("</title><script>unsafe()</script>", "/openapi.json"),
        ] {
            assert!(html.contains("&lt;/title&gt;&lt;script&gt;unsafe()&lt;/script&gt;"));
            assert!(!html.contains("<script>unsafe()"));
        }
    }

    #[test]
    fn html_paths_reject_invalid_reserved_and_declared_routes() {
        let mut registry = ContractRegistry::new();
        registry.register(RouteContract::new(
            "read",
            "Read",
            HttpMethod::Get,
            "/notes",
        ));
        registry.register(RouteContract::new(
            "head",
            "Head",
            HttpMethod::Head,
            "/headers",
        ));
        for path in [
            "relative",
            "/{dynamic}",
            "/:dynamic",
            "/wild*",
            "/docs?query",
            "/docs#fragment",
            "/docs\n",
            "/openapi.json",
            "/openapi.json-hash",
            "/notes",
            "/headers",
        ] {
            assert!(
                with_docs(Router::<()>::new(), &registry, path).is_err(),
                "accepted {path:?}"
            );
        }
        for path in ["/openapi.json", "/openapi.json-hash"] {
            registry.register(RouteContract::new(path, path, HttpMethod::Get, path));
            assert!(with_docs(Router::<()>::new(), &registry, "/docs").is_err());
        }
    }

    #[tokio::test]
    async fn documentation_uses_managed_security_for_html_and_json() {
        let router = with_docs(Router::new(), &ContractRegistry::new(), "/docs").unwrap();
        let policy = crate::native::NativeHttpPolicy::new("https://notes.example").unwrap();
        let router = crate::native::secure(router, policy);
        for path in ["/docs", "/openapi.json", "/openapi.json-hash"] {
            for (origin, status) in [
                ("https://notes.example", StatusCode::OK),
                ("https://hostile.example", StatusCode::FORBIDDEN),
            ] {
                let response = router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri(path)
                            .header("origin", origin)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), status);
                assert_eq!(
                    response.headers()[header::CACHE_CONTROL],
                    "private, no-store"
                );
                assert_eq!(response.headers()["x-content-type-options"], "nosniff");
            }
        }
    }

    #[tokio::test]
    async fn configured_alias_preserves_public_document_and_existing_routes() {
        let mut registry = ContractRegistry::new();
        registry.register(RouteContract::new(
            "read",
            "read",
            HttpMethod::Get,
            "/notes",
        ));
        let mut private = RouteContract::new("internal", "internal", HttpMethod::Post, "/internal");
        private.options.openapi = false;
        registry.register(private);
        let router = with_openapi(
            Router::new().route("/health", get(|| async { "ok" })),
            &registry,
            "/docs",
        )
        .unwrap();
        for path in ["/openapi.json", "/docs"] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
            let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
            let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                document["paths"]["/notes"]["get"]["x-forklaunch-operation"]["format"],
                "forklaunch.sdk-operation.v1"
            );
            assert!(document["paths"].get("/internal").is_none());
        }
        let mut hashes = Vec::new();
        for path in ["/openapi.json-hash", "/docs-hash"] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            hashes.push(to_bytes(response.into_body(), 64).await.unwrap());
        }
        assert_eq!(hashes[0], hashes[1]);
        assert!(!hashes[0].is_empty());
        assert!(with_openapi(Router::<()>::new(), &registry, "/openapi.json-hash").is_err());
        assert_eq!(
            router
                .oneshot(
                    Request::builder()
                        .uri("/health")
                        .body(Body::empty())
                        .unwrap()
                )
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert!(with_openapi(Router::<()>::new(), &registry, "/openapi.json").is_ok());
        assert!(with_openapi(Router::<()>::new(), &registry, "/notes").is_err());
        assert!(with_openapi(Router::<()>::new(), &registry, "/{dynamic}").is_err());
        assert!(with_openapi(Router::<()>::new(), &registry, "relative").is_err());
    }
}
