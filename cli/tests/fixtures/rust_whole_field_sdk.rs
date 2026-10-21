include!("fields.rs");
#[tokio::test]
async fn checked_request_groups_run_on_actual_wire() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = axum::Router::new().route("/range", axum::routing::get(|| async { "ok" }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = ForkLaunchClient::new(format!("http://{address}"));
    let input = || RangeRequest {
        query: RangeQuery {
            start: Some("a".into()),
            end: Some("b".into()),
        },
        headers: RangeHeaders {
            x_start: Some("a".into()),
            x_end: Some("b".into()),
        },
        ..Default::default()
    };
    assert!(client.range(input()).await.is_ok());
    let mut bad = input();
    bad.query.end = None;
    assert!(matches!(
        client.range(bad).await,
        Err(SdkError::Contract(_))
    ));
    let mut bad = input();
    bad.headers.x_end = None;
    assert!(matches!(
        client.range(bad).await,
        Err(SdkError::Contract(_))
    ));
    let hooked = client.with_auth_hook(|request| {
        request.query.remove("end");
    });
    assert!(matches!(
        hooked.range(input()).await,
        Err(SdkError::Contract(_))
    ));
    server.abort();
}
mod versioned {
    include!("versions.rs");
}
#[tokio::test]
async fn version_groups_select_actual_matched_branch() {
    use versioned::*;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let response = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let state = response.clone();
    let app = axum::Router::new().route(
        "/ranges",
        axum::routing::get(move || {
            let state = state.clone();
            async move {
                if state.load(std::sync::atomic::Ordering::SeqCst) {
                    "first"
                } else {
                    "second"
                }
            }
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = ForkLaunchClient::new(format!("http://{address}"));
    let input = || RangesVersionFirstRequest {
        query: RangesVersionFirstQuery {
            start: Some("a".into()),
            end: None,
        },
        ..Default::default()
    };
    assert!(client.ranges_version_first(input()).await.is_ok());
    response.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(matches!(
        client.ranges_version_first(input()).await,
        Err(SdkError::Contract(_))
    ));
    server.abort();
}
