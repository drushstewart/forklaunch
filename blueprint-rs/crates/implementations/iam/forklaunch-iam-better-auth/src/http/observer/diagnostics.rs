use super::*;
use chrono::{DateTime, Utc};
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn configured_traces_preserve_catches_and_skip_silent_handled_failures() {
    const CHILD: &str = "FORKLAUNCH_CONFIGURED_DIAGNOSTICS_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "http::observer::tests::diagnostics::configured_traces_preserve_catches_and_skip_silent_handled_failures", "--test-threads=1"])
            .env(CHILD, "1").output().unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{text} {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            text.contains("running 1 test") && text.contains("1 passed; 0 failed"),
            "{text}"
        );
        return;
    }
    let trace_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let dispatcher = tracing::Dispatch::new(TraceCounter(trace_count.clone()));
    for trace_mode in [
        AuthApiErrorTraceMode::AllServerErrors,
        AuthApiErrorTraceMode::Configured,
    ] {
        for (kind, expected_callback, configured_trace) in [(0, 1, 1), (1, 0, 0), (2, 0, 1)] {
            let observed = Arc::new(Mutex::new(Vec::new()));
            let capture = observed.clone();
            let app = Arc::new(
                app()
                    .with_api_error_trace_mode(trace_mode)
                    .with_api_error_observer_mode(AuthApiErrorObserverMode::UnexpectedOnly)
                    .with_api_error_observer(move |event| {
                        capture.lock().unwrap().push(event.clone());
                    }),
            );
            let router = Router::new()
                .route(
                    "/owned",
                    get(move || async move {
                        let error = BlueprintError::new("HTTP_INTERNAL_EMPTY", "PRIVATE_SECRET");
                        let error = match kind {
                            1 => crate::error_origin::handled(error),
                            2 => crate::error_origin::internal_catch(error),
                            _ => error,
                        };
                        super::super::super::failure(&error)
                    }),
                )
                .layer(axum::middleware::from_fn_with_state(app, observe));
            trace_count.store(0, std::sync::atomic::Ordering::SeqCst);
            let response = router
                .oneshot(
                    Request::builder()
                        .uri("/owned")
                        .body(Body::empty())
                        .unwrap(),
                )
                .with_subscriber(dispatcher.clone())
                .await
                .unwrap();
            assert_eq!(response.status(), 500);
            assert!(
                to_bytes(response.into_body(), 4096)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(observed.lock().unwrap().len(), expected_callback);
            assert_eq!(
                trace_count.load(std::sync::atomic::Ordering::SeqCst),
                if trace_mode == AuthApiErrorTraceMode::Configured {
                    configured_trace
                } else {
                    1
                }
            );
            assert!(!format!("{:?}", observed.lock().unwrap()).contains("PRIVATE"));
        }
    }
}

#[tokio::test]
async fn session_page_failure_is_caught_after_authentication_without_changing_default_callback() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let store = Arc::new(MemoryAuthStore::default());
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"owner","email":"owner@example.test"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let session = crate::sessions::SessionService::new(store.clone())
        .create("owner", "", "", now)
        .await
        .unwrap();
    let cookie = format!(
        "better-auth.session_token={}",
        crate::cookies::sign_cookie_value(session["token"].as_str().unwrap(), &[42; 32])
    );
    for mode in [
        AuthApiErrorObserverMode::AllServerErrors,
        AuthApiErrorObserverMode::UnexpectedOnly,
    ] {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let capture = observed.clone();
        let router = AuthHttp::new(
            Arc::new(fault_store::FaultStore {
                store: store.clone(),
                page: Some(Model::Session),
                read: None,
                delete: None,
                insert: None,
                begin: false,
                commit: false,
            }),
            "http://localhost:3000".into(),
            vec![42; 32],
            vec![],
        )
        .unwrap()
        .with_clock(move || now)
        .with_rate_limiting(false)
        .with_api_error_trace_mode(AuthApiErrorTraceMode::Configured)
        .with_api_error_observer_mode(mode)
        .with_api_error_observer(move |event| capture.lock().unwrap().push(event.clone()))
        .router();
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/list-sessions")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 500);
        assert!(response.headers().get("set-cookie").is_none());
        assert!(response.headers().get("content-type").is_none());
        assert!(
            to_bytes(response.into_body(), 4096)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            observed.lock().unwrap().len(),
            usize::from(mode == AuthApiErrorObserverMode::AllServerErrors)
        );
    }
}
