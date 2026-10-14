use super::*;
use crate::{http::AuthRateLimitOptions, store::memory::MemoryAuthStore};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
    routing::get,
};
use serde_json::json;
use std::sync::Mutex;
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

#[tokio::test]
async fn api_error_observer_sees_rate_failures_once_and_isolates_panics() {
    for panic in [false, true] {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let options = AuthRateLimitOptions::default()
            .with_dynamic_path("/**", |_, _| async {
                Err(BlueprintError::new("PRIVATE_CODE", "PRIVATE_PASSWORD"))
            })
            .unwrap();
        let router = app()
            .with_rate_limiting(true)
            .with_rate_limit_options(options)
            .with_api_error_observer(move |event| {
                captured.lock().unwrap().push(event.clone());
                assert!(!panic, "owned observer failure");
            })
            .router();
        let response = Router::new()
            .nest("/api/auth", router)
            .oneshot(
                Request::builder()
                    .uri("/api/auth/reset-password/PRIVATE_TOKEN?token=PRIVATE_QUERY")
                    .header("cookie", "PRIVATE_COOKIE")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 500);
        assert!(response.extensions().get::<ErrorMetadata>().is_none());
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            json!({"code":"INTERNAL_SERVER_ERROR","message":"Authentication operation failed"})
        );
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].route, "/api/auth/reset-password/{token}");
        assert_eq!(events[0].status, 500);
        assert!(events[0].database.is_empty());
        assert!(!format!("{events:?}").contains("PRIVATE"));
    }
}

#[tokio::test]
async fn api_error_observer_skips_expected_errors_and_success() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let router = app()
        .with_rate_limiting(false)
        .with_api_error_observer(move |event| captured.lock().unwrap().push(event.clone()))
        .router();
    for (path, status) in [("/get-session", 200), ("/token", 401), ("/missing", 404)] {
        let response = router
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
    assert!(events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn api_error_observer_selects_structural_metadata_and_covers_empty_failures() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let app = Arc::new(
        app().with_api_error_observer(move |event| captured.lock().unwrap().push(event.clone())),
    );
    let router = Router::new()
        .route(
            "/fixture/{token}",
            get(|| async {
                super::super::failure(
                    &BlueprintError::new("better_auth_storage_error", "PRIVATE_PASSWORD")
                        .with_detail("code", json!("23502"))
                        .with_detail("table", json!("auth_records"))
                        .with_detail("column", json!("PRIVATE INVALID VALUE"))
                        .with_detail("constraint", json!("auth_records_pkey"))
                        .with_detail("detail", json!("PRIVATE_ROW")),
                )
            }),
        )
        .route(
            "/empty",
            get(|| async { axum::http::StatusCode::INTERNAL_SERVER_ERROR }),
        )
        .layer(axum::middleware::from_fn_with_state(
            app,
            observe::<MemoryAuthStore>,
        ));
    for path in ["/fixture/PRIVATE_TOKEN", "/empty"] {
        assert_eq!(
            router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            500
        );
    }
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].route, "/fixture/{token}");
    assert_eq!(
        events[0].database,
        BTreeMap::from([
            ("code".into(), "23502".into()),
            ("table".into(), "auth_records".into()),
            ("constraint".into(), "auth_records_pkey".into())
        ])
    );
    assert!(!format!("{events:?}").contains("PRIVATE"));
    assert_eq!(events[1].route, "/empty");
    assert!(events[1].database.is_empty());
}
