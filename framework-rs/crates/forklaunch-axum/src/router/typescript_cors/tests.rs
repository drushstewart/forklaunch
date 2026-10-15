use super::*;

#[test]
#[should_panic(expected = "mutually exclusive")]
fn conflicting_native_profile_rejected() {
    let _ = wrap(
        Router::new(),
        Some(tower_http::cors::CorsLayer::permissive()),
        Some(TypeScriptCors::default()),
    );
}

#[test]
#[should_panic(expected = "must be a final HTTP status")]
fn invalid_preflight_status_rejected() {
    let _ = wrap(
        Router::new(),
        None,
        Some(TypeScriptCors {
            options_success_status: StatusCode::CONTINUE,
            ..TypeScriptCors::default()
        }),
    );
}

use crate::{ForkLaunchRouter, ParserErrorFormat, RouterConfig, RouterOriginFormat};
use axum::body::{Body, to_bytes};
use forklaunch_contract::{HttpMethod, RouteContract};
use tower::ServiceExt;

#[tokio::test]
async fn configured_final_preflight_statuses_keep_empty_cors_response() {
    for status in [200, 205, 299, 300, 304, 400, 418, 500, 599] {
        let app = wrap(
            Router::new(),
            None,
            Some(TypeScriptCors {
                options_success_status: StatusCode::from_u16(status).unwrap(),
                ..TypeScriptCors::default()
            }),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/missing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        assert!(
            to_bytes(response.into_body(), 4096)
                .await
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn preflight_interception_disabled_continue_and_options_routing_are_explicit() {
    for policy in [
        None,
        Some(TypeScriptCors::default()),
        Some(TypeScriptCors {
            origin: TypeScriptCorsOrigin::Disabled,
            ..TypeScriptCors::default()
        }),
        Some(TypeScriptCors {
            preflight_continue: true,
            ..TypeScriptCors::default()
        }),
    ] {
        let intercept = policy
            .as_ref()
            .is_some_and(|config| !config.disabled() && !config.preflight_continue);
        let app = ForkLaunchRouter::anonymous()
            .get(
                RouteContract::get("Read", "Read", "/items/:id").with_response(200, None),
                || async { "ok" },
            )
            .route(
                RouteContract::new("Options", "Options", HttpMethod::Options, "/explicit")
                    .with_response(202, None),
                || async { (StatusCode::ACCEPTED, "explicit") },
            )
            .with_config(RouterConfig {
                typescript_cors: policy.clone(),
                router_origin_format: RouterOriginFormat::TypeScriptHtml,
                parser_error_format: ParserErrorFormat::TypeScriptText,
                ..RouterConfig::default()
            })
            .into_router();
        for (path, downstream_status) in [
            ("/items/one", 200),
            ("/items/%GG", 400),
            ("/explicit", 202),
            ("/missing", 404),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("OPTIONS")
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status().as_u16(),
                if intercept { 204 } else { downstream_status }
            );
            if !intercept && path == "/items/one" {
                assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
                assert_eq!(response.headers()[header::CONTENT_TYPE], "text/plain");
                assert_eq!(
                    to_bytes(response.into_body(), 4096).await.unwrap(),
                    "GET, HEAD"
                );
            }
        }
    }
}

#[tokio::test]
async fn prepared_headers_support_explicit_vary_merge_and_preserve_overrides() {
    let app = Router::new().route(
        "/",
        axum::routing::get(
            |axum::Extension(prepared): axum::Extension<TypeScriptCorsHeaders>| async move {
                let mut response = "ok".into_response();
                response.headers_mut().extend(prepared.0);
                response
                    .headers_mut()
                    .append(header::VARY, HeaderValue::from_static("Accept-Encoding"));
                response.headers_mut().insert(
                    header::ACCESS_CONTROL_ALLOW_ORIGIN,
                    HeaderValue::from_static("application"),
                );
                response
            },
        ),
    );
    let app = wrap(
        app,
        None,
        Some(TypeScriptCors {
            origin: TypeScriptCorsOrigin::Reflect,
            ..TypeScriptCors::default()
        }),
    );
    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::ORIGIN, "https://allowed.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "application"
    );
    assert_eq!(
        response
            .headers()
            .get_all(header::VARY)
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["Origin", "Accept-Encoding"]
    );
}

#[test]
#[should_panic(expected = "JavaScript-safe integer")]
fn max_age_that_javascript_would_round_is_rejected() {
    let _ = wrap(
        Router::new(),
        None,
        Some(TypeScriptCors {
            max_age: Some(u64::MAX),
            ..TypeScriptCors::default()
        }),
    );
}
