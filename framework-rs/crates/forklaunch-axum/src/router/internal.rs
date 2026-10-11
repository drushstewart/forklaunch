//! Application-owned HTTP mounts, independent of exported route contracts.
use super::{ForkLaunchRouter, RouterConfig};
use axum::Router;
use forklaunch_common::join_base_path;
use forklaunch_contract::ContractRegistry;

pub(super) struct InternalMount {
    pub(super) prefix: String,
    pub(super) router: Router,
}

impl ForkLaunchRouter {
    /// Mounts an application-owned HTTP API under a fixed, reserved prefix.
    /// Handlers own authentication and validation. Shared body limits, CORS,
    /// request IDs and telemetry still apply. These routes have no contract
    /// entries and therefore create no `OpenAPI`, SDK or MCP operations.
    ///
    /// # Panics
    /// Panics if the prefix is empty, root, parameterized or otherwise invalid.
    /// Building the router also rejects overlapping namespaces, contracts and
    /// documentation endpoints.
    #[must_use]
    pub fn nest_internal(mut self, prefix: impl AsRef<str>, router: Router) -> Self {
        let prefix = prefix.as_ref();
        assert!(
            fixed_prefix(prefix),
            "internal HTTP mount requires a fixed absolute prefix"
        );
        let prefix = join_base_path(&self.base_path, prefix);
        assert!(prefix != "/", "internal HTTP mount cannot reserve the root");
        self.internal_mounts.push(InternalMount { prefix, router });
        self
    }
}

fn fixed_prefix(prefix: &str) -> bool {
    prefix.starts_with('/')
        && !prefix.chars().any(|character| {
            character.is_control()
                || character.is_whitespace()
                || matches!(character, '{' | '}' | ':' | '*' | '?' | '#' | '%' | '\\')
        })
        && !prefix.split('/').any(|part| matches!(part, "." | ".."))
}

pub(super) fn contains_path(prefix: &str, path: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn intersects_route(prefix: &str, route: &str) -> bool {
    let mut route = route.split('/').filter(|segment| !segment.is_empty());
    for expected in prefix.split('/').filter(|segment| !segment.is_empty()) {
        let Some(segment) = route.next() else {
            return false;
        };
        if segment.starts_with("{*") {
            return true;
        }
        if segment != expected && !(segment.starts_with('{') && segment.ends_with('}')) {
            return false;
        }
    }
    true
}

pub(super) fn validate(
    mounts: &[InternalMount],
    registry: &ContractRegistry,
    config: &RouterConfig,
) -> Result<(), &'static str> {
    let docs = [
        config
            .openapi_enabled
            .then_some(config.openapi_path.as_str()),
        (config.openapi_enabled && config.docs_enabled).then_some(config.docs_path.as_str()),
        (config.openapi_enabled && config.swagger_enabled).then_some(config.swagger_path.as_str()),
    ];
    for (index, mount) in mounts.iter().enumerate() {
        if !fixed_prefix(&mount.prefix) || mount.prefix == "/" {
            return Err("internal HTTP mount requires a fixed non-root absolute prefix");
        }
        if mounts[..index].iter().any(|other| {
            contains_path(&mount.prefix, &other.prefix)
                || contains_path(&other.prefix, &mount.prefix)
        }) {
            return Err("overlapping internal HTTP namespaces");
        }
        if registry
            .routes()
            .iter()
            .any(|route| intersects_route(&mount.prefix, &route.openapi_path))
        {
            return Err("internal HTTP namespace conflicts with a registered contract");
        }
        if docs
            .iter()
            .flatten()
            .any(|path| intersects_route(&mount.prefix, path))
        {
            return Err("internal HTTP namespace conflicts with documentation");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        routing::{get, post},
    };
    use forklaunch_contract::RouteContract;
    use tower::ServiceExt;

    #[tokio::test]
    async fn nested_internal_mounts_keep_raw_bodies_limits_and_request_ids() {
        let internal = Router::new()
            .route("/session", get(|| async { "session" }))
            .route("/echo", post(|body: axum::body::Bytes| async { body }));
        let child = ForkLaunchRouter::anonymous()
            .with_base_path("/v1")
            .nest_internal("/auth", internal);
        let config = RouterConfig {
            body_limit: 8,
            ..RouterConfig::default()
        };
        let app = ForkLaunchRouter::anonymous()
            .with_base_path("/platform")
            .nest("/tenant", child)
            .with_config(config);
        assert!(app.registry().routes().is_empty());
        let app = app.into_router();
        let session = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/platform/tenant/v1/auth/session")
                    .header("x-request-id", "internal-request")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(session.status(), StatusCode::OK);
        assert_eq!(session.headers()["x-request-id"], "internal-request");
        let binary = vec![0, 255, 1, 42];
        let echoed = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/platform/tenant/v1/auth/echo")
                    .body(Body::from(binary.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(echoed.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(echoed.into_body(), 8).await.unwrap().as_ref(),
            binary
        );
        let limited = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/platform/tenant/v1/auth/echo")
                    .header("content-length", "9")
                    .body(Body::from("too large"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(limited.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn namespaces_reject_overlapping_contracts_docs_and_other_mounts() {
        for route in ["/auth", "/auth/token", "/{group}/token", "/{group}"] {
            let app = ForkLaunchRouter::anonymous()
                .nest_internal("/auth", Router::new())
                .get(RouteContract::get("Overlap", "Overlap", route), || async {
                    "overlap"
                });
            assert!(
                validate(&app.internal_mounts, &app.registry, &app.config).is_err(),
                "{route}"
            );
        }
        let app = ForkLaunchRouter::anonymous()
            .nest_internal("/auth", Router::new())
            .nest_internal("/auth/deep", Router::new());
        assert!(validate(&app.internal_mounts, &app.registry, &app.config).is_err());
        let config = RouterConfig {
            openapi_path: "/auth/openapi.json".into(),
            ..RouterConfig::default()
        };
        let app = ForkLaunchRouter::anonymous()
            .nest_internal("/auth", Router::new())
            .with_config(config);
        assert!(validate(&app.internal_mounts, &app.registry, &app.config).is_err());
        let app = ForkLaunchRouter::anonymous()
            .nest_internal("/auth", Router::new())
            .nest_internal("/authentication", Router::new())
            .get(
                RouteContract::get("Other", "Other", "/authenticator"),
                || async { "other" },
            );
        assert!(validate(&app.internal_mounts, &app.registry, &app.config).is_ok());
    }

    #[test]
    fn parent_base_paths_cannot_parameterize_internal_namespaces() {
        let child = ForkLaunchRouter::anonymous().nest_internal("/auth", Router::new());
        let app = ForkLaunchRouter::anonymous().nest("/{tenant}", child);
        assert!(validate(&app.internal_mounts, &app.registry, &app.config).is_err());
        let app = ForkLaunchRouter::anonymous()
            .with_base_path("/{tenant}")
            .nest_internal("/auth", Router::new());
        assert!(validate(&app.internal_mounts, &app.registry, &app.config).is_err());
    }

    #[cfg(feature = "mcp")]
    #[test]
    fn mcp_endpoints_cannot_claim_internal_namespaces() {
        for path in ["/auth", "/auth/mcp", "/auth/nested/tools"] {
            let app = ForkLaunchRouter::anonymous().nest_internal("/auth", Router::new());
            let mut options = forklaunch_mcp::server::McpServerOptions::new("internal", "1");
            options.path = path.into();
            assert!(app.into_routers_with_mcp(options).is_err(), "{path}");
        }
    }
}
