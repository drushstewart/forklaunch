use super::*;
use axum::{
    body::{Body, Bytes},
    routing::{get, post},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tower::ServiceExt;

// Deliberately unknown length: exercise streaming enforcement, not Content-Length.
struct UnknownLengthBody(Option<Bytes>);
impl http_body::Body for UnknownLengthBody {
    type Data = Bytes;
    type Error = std::convert::Infallible;
    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Bytes>, Self::Error>>> {
        std::task::Poll::Ready(self.0.take().map(|bytes| Ok(http_body::Frame::data(bytes))))
    }
}

fn policy() -> NativeHttpPolicy {
    NativeHttpPolicy::new("https://notes.example.test").unwrap()
}
fn assert_private(response: &Response) {
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    assert!(response.headers().contains_key("traceparent"));
}

#[test]
fn malformed_configuration_fails_closed() {
    for origin in [
        "*",
        "null",
        "https://",
        "ftp://notes.test",
        "http://notes.test",
        "http://0.0.0.0:8000",
        "http://localhost.evil.test",
        "https://notes.test/",
        "https://notes.test/path",
        "https://notes.test?x=1",
        "https://notes.test#x",
        "https://secret@notes.test",
    ] {
        assert!(NativeHttpPolicy::new(origin).is_err(), "accepted {origin}");
    }
    for origin in [
        "http://localhost:8000",
        "http://127.0.0.1:8000",
        "http://[::1]:8000",
    ] {
        assert!(NativeHttpPolicy::new(origin).is_ok());
    }
    assert!(policy().with_max_body_bytes(0).is_err());
    assert!(policy().with_response_timeout(Duration::ZERO).is_err());
}

#[tokio::test]
async fn rejects_hostile_browser_headers_before_application_execution() {
    let _exclusive = super::super::TRACE_TEST.lock().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let handler_calls = calls.clone();
    let router = secure(
        Router::new().route(
            "/notes",
            post(move || {
                let calls = handler_calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    "ok"
                }
            }),
        ),
        policy(),
    );
    for (name, value) in [
        ("origin", "https://evil.test"),
        ("origin", "null"),
        ("sec-fetch-site", "cross-site"),
        ("sec-fetch-site", "same-site"),
        ("sec-fetch-site", "invalid"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/notes")
                    .header(name, value)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_private(&response);
    }
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/notes")
                .header("origin", "https://notes.example.test")
                .header("origin", "https://evil.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for origin in [None, Some("https://notes.example.test")] {
        let mut request = Request::builder().method("POST").uri("/notes");
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        let response = router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_private(&response);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn body_limits_apply_without_content_length_and_errors_remain_private() {
    let _exclusive = super::super::TRACE_TEST.lock().await;
    let router = secure(
        Router::new().route("/notes", post(|_: Bytes| async { "accepted" })),
        policy().with_max_body_bytes(4).unwrap(),
    );
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/notes")
                .body(Body::new(UnknownLengthBody(Some(Bytes::from_static(
                    b"12345",
                )))))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_private(&response);
    let bytes = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
        serde_json::json!({"error":"Request body too large"})
    );
}

#[tokio::test]
async fn timeout_cancels_handler_but_not_streaming_responses() {
    let _exclusive = super::super::TRACE_TEST.lock().await;
    let router = secure(
        Router::new()
            .route(
                "/pending",
                get(|| async { std::future::pending::<StatusCode>().await }),
            )
            .route(
                "/events",
                get(|| async {
                    (
                        [
                            ("content-type", "text/event-stream"),
                            ("cache-control", "public"),
                        ],
                        "data: ready\n\n",
                    )
                }),
            ),
        policy()
            .with_response_timeout(Duration::from_millis(10))
            .unwrap(),
    );
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/pending")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert_private(&response);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let endpoint = forklaunch_core::contract::endpoint::Endpoint::<
        forklaunch_core::contract::endpoint::NoBody,
        forklaunch_core::contract::endpoint::NoBody,
    >::new(
        "pending",
        "Pending",
        forklaunch_core::contract::HttpMethod::Get,
        "/pending",
        200,
    )
    .unwrap();
    let timeout = endpoint
        .contract()
        .responses
        .into_iter()
        .find(|response| response.status_code == 408)
        .expect("managed timeout must be declared");
    let body_contract = timeout.body.unwrap();
    assert_eq!(
        body_contract.content_type.as_deref(),
        Some("application/json")
    );
    let schema = serde_json::from_str(body_contract.schema.json_schema.as_ref().unwrap()).unwrap();
    let validate = forklaunch_core::contract::schema_validation::compile_checked(&schema).unwrap();
    let actual = serde_json::from_slice(&body).unwrap();
    validate(&actual).unwrap();
    assert_eq!(actual, serde_json::json!({"error":"Request timed out"}));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_private(&response);
    assert_eq!(
        axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap(),
        "data: ready\n\n"
    );
}
