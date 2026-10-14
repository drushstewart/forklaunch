use super::*;
use crate::{
    http::AuthRateLimitOptions,
    store::{AuthCursor, AuthPage, AuthTransaction, AuthWindow, Model, memory::MemoryAuthStore},
};
#[path = "fault_store.rs"]
mod fault_store;
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

struct TraceCounter(Arc<std::sync::atomic::AtomicUsize>);
impl tracing::Subscriber for TraceCounter {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if event.metadata().target() == "forklaunch_iam_better_auth"
            && *event.metadata().level() == tracing::Level::ERROR
        {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[tokio::test]
async fn unexpected_mode_uses_private_origin_not_status_code_or_details() {
    for mode in [
        AuthApiErrorObserverMode::AllServerErrors,
        AuthApiErrorObserverMode::UnexpectedOnly,
    ] {
        let trace_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let _trace_guard = tracing::subscriber::set_default(TraceCounter(trace_count.clone()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let capture = events.clone();
        let app = Arc::new(
            app()
                .with_api_error_observer_mode(mode)
                .with_api_error_observer(move |event| capture.lock().unwrap().push(event.clone())),
        );
        let router = Router::new()
            .route(
                "/handled",
                get(|| async {
                    super::super::failure(&crate::error_origin::handled(BlueprintError::new(
                        "HTTP_INTERNAL_MESSAGE_ONLY",
                        "owned failure",
                    )))
                }),
            )
            .route(
                "/unexpected",
                get(|| async {
                    super::super::failure(
                        &BlueprintError::new("HTTP_INTERNAL_MESSAGE_ONLY", "owned failure")
                            .with_detail("origin", json!("HandledApiError"))
                            .with_detail("handled", json!(true))
                            .with_diagnostic_context("HandledApiError"),
                    )
                }),
            )
            .layer(axum::middleware::from_fn_with_state(
                app,
                observe::<MemoryAuthStore>,
            ));
        let mut responses = Vec::new();
        for path in ["/handled", "/unexpected"] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), 500);
            assert!(response.extensions().get::<HandledResponse>().is_none());
            assert!(response.extensions().get::<ErrorMetadata>().is_none());
            let headers = response.headers().clone();
            responses.push((headers, to_bytes(response.into_body(), 4096).await.unwrap()));
        }
        assert_eq!(
            responses[0], responses[1],
            "private provenance must not alter wire responses"
        );
        assert_eq!(
            trace_count.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "safe tracing is independent of callback dispatch mode"
        );
        let events = events.lock().unwrap();
        assert_eq!(
            events.len(),
            if mode == AuthApiErrorObserverMode::AllServerErrors {
                2
            } else {
                1
            }
        );
        assert_eq!(events.last().unwrap().route, "/unexpected");
    }
}

#[tokio::test]
async fn unexpected_mode_matches_session_and_revocation_catch_boundaries() {
    use chrono::{DateTime, Utc};
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let store = Arc::new(MemoryAuthStore::default());
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"owner","name":"Owner","email":"owner@example.test"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let service = crate::sessions::SessionService::new(store.clone());
    let current = service.create("owner", "", "", now).await.unwrap();
    let target = service.create("owner", "", "", now).await.unwrap();
    let token = current["token"].as_str().unwrap();
    let cookie = format!(
        "better-auth.session_token={}",
        crate::cookies::sign_cookie_value(token, &[42; 32])
    );
    for (read, delete, path, body, expected) in [
        (
            Some(Model::User),
            None,
            "/sign-in/email",
            Some(json!({"email":"owner@example.test","password":"owned-password"})),
            1,
        ),
        (Some(Model::Session), None, "/get-session", None, 0),
        (
            None,
            Some(Model::Session),
            "/revoke-session",
            Some(json!({"token":target["token"]})),
            0,
        ),
        (
            None,
            Some(Model::Session),
            "/revoke-sessions",
            Some(json!({})),
            0,
        ),
        (
            None,
            Some(Model::Session),
            "/revoke-other-sessions",
            Some(json!({})),
            1,
        ),
    ] {
        let events = Arc::new(Mutex::new(Vec::new()));
        let capture = events.clone();
        let router = AuthHttp::new(
            Arc::new(fault_store::FaultStore {
                store: store.clone(),
                read,
                delete,
            }),
            "http://localhost:3000".into(),
            vec![42; 32],
            vec![],
        )
        .unwrap()
        .with_clock(move || now)
        .with_rate_limiting(false)
        .with_api_error_observer_mode(AuthApiErrorObserverMode::UnexpectedOnly)
        .with_api_error_observer(move |event| capture.lock().unwrap().push(event.clone()))
        .router();
        let request = Request::builder()
            .method(if body.is_some() { "POST" } else { "GET" })
            .uri(path)
            .header("origin", "http://localhost:3000")
            .header("cookie", &cookie)
            .header("content-type", "application/json")
            .body(Body::from(
                body.map_or(String::new(), |body| body.to_string()),
            ))
            .unwrap();
        let response = router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 500, "{path}");
        assert_eq!(events.lock().unwrap().len(), expected, "{path}");
    }
}

#[tokio::test]
async fn unexpected_mode_distinguishes_permission_json_from_handled_schema_failure() {
    use chrono::{DateTime, Utc};
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let store = Arc::new(MemoryAuthStore::default());
    let mut tx = store.begin(true).await.unwrap();
    for (model, row) in [
        (
            Model::User,
            json!({"id":"owner","name":"Owner","email":"owner@example.test"}),
        ),
        (
            Model::Organization,
            json!({"id":"organization","name":"Owned","slug":"owned"}),
        ),
        (
            Model::Member,
            json!({"id":"member","organizationId":"organization","userId":"owner","role":"owner"}),
        ),
        (
            Model::OrganizationRole,
            json!({"id":"broken","organizationId":"organization","role":"broken","permission":"null"}),
        ),
    ] {
        tx.insert(model, row).await.unwrap();
    }
    tx.commit().await.unwrap();
    let session = crate::sessions::SessionService::new(store.clone())
        .create("owner", "", "", now)
        .await
        .unwrap();
    let cookie = format!(
        "better-auth.session_token={}",
        crate::cookies::sign_cookie_value(session["token"].as_str().unwrap(), &[42; 32])
    );
    let events = Arc::new(Mutex::new(Vec::new()));
    let capture = events.clone();
    let router = AuthHttp::new(
        store.clone(),
        "http://localhost:3000".into(),
        vec![42; 32],
        vec![],
    )
    .unwrap()
    .with_clock(move || now)
    .with_rate_limiting(false)
    .with_api_error_observer_mode(AuthApiErrorObserverMode::UnexpectedOnly)
    .with_api_error_observer(move |event| capture.lock().unwrap().push(event.clone()))
    .router();
    for (permission, expected) in [("null", 0), ("{", 1)] {
        let mut tx = store.begin(true).await.unwrap();
        let mut role = tx
            .get(Model::OrganizationRole, "broken")
            .await
            .unwrap()
            .unwrap();
        role["permission"] = json!(permission);
        tx.replace(Model::OrganizationRole, role).await.unwrap();
        tx.commit().await.unwrap();
        let response=router.clone().oneshot(Request::builder().method("POST").uri("/organization/has-permission").header("origin","http://localhost:3000").header("cookie",&cookie).header("content-type","application/json")
            .body(Body::from(json!({"organizationId":"organization","permissions":{"platform":["platform:read"]}}).to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), 500);
        assert_eq!(events.lock().unwrap().len(), expected, "{permission}");
    }
}
