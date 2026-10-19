//! Framework-owned public contract documentation routes.
use axum::{Router, http::header, routing::get};
use forklaunch_core::contract::{ContractRegistry, HttpMethod};

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
    if docs_path == "/openapi.json-hash"
        || !docs_path.starts_with('/')
        || docs_path.contains(['{', '}', '*', '?', '#'])
        || docs_path.split('/').any(|segment| segment.starts_with(':'))
        || docs_path.chars().any(char::is_control)
    {
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
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hasher::write(&mut hasher, document.as_bytes());
    let fingerprint = format!("{:016x}", std::hash::Hasher::finish(&hasher));
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
    use forklaunch_core::contract::RouteContract;
    use tower::ServiceExt as _;

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
