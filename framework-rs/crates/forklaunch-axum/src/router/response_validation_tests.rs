use super::*;
use forklaunch_contract::{BodyContract, HttpMethod};
use forklaunch_http::RequestContext;
use std::{
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll},
};

fn route() -> RouteContract {
    RouteContract::get("Response", "Response", "/response")
        .with_response(200, Some(BodyContract::json("Output")))
}
fn request() -> FrameworkRequest {
    FrameworkRequest::new(
        HttpMethod::Get,
        "/response",
        RequestContext::new("response-test"),
    )
}

#[test]
fn registry_rejects_unsupported_and_stale_contracts() {
    let mut validators = ResponseValidators::default();
    for kind in [
        BodyKind::File,
        BodyKind::Binary,
        BodyKind::Multipart,
        BodyKind::ServerSentEvent,
        BodyKind::Unknown,
        BodyKind::UrlEncoded,
    ] {
        let mut contract = route();
        contract.responses[0].body.as_mut().unwrap().kind = kind;
        assert!(
            validators
                .insert(&contract, 10, Duration::from_secs(1), |_, _| None)
                .is_err()
        );
    }
    assert!(
        validators
            .insert(&route(), 0, Duration::from_secs(1), |_, _| None)
            .is_err()
    );
    assert!(
        validators
            .insert(&route(), 10, Duration::ZERO, |_, _| None)
            .is_err()
    );
    validators
        .insert(&route(), 10, Duration::from_secs(1), |_, _| None)
        .unwrap();
    assert!(
        validators
            .insert(&route(), 10, Duration::from_secs(1), |_, _| None)
            .is_err()
    );
    assert!(validators.validate_routes(&[]).is_err());
    assert!(validators.validate_routes(&[route(), route()]).is_err());
    assert!(
        validators
            .validate_routes(&[route().with_response(500, None)])
            .is_err()
    );
    assert!(validators.validate_routes(&[route()]).is_ok());
}

#[tokio::test]
async fn success_preserves_headers_extensions_and_bytes() {
    let mut validators = ResponseValidators::default();
    validators
        .insert(&route(), 64, Duration::from_secs(1), |_, response| {
            assert_eq!(response.body.as_text(), Some("{\"ok\":true}"));
            None
        })
        .unwrap();
    let mut response = Response::new(Body::from("{\"ok\":true}"));
    response
        .headers_mut()
        .append("set-cookie", "a=1".parse().unwrap());
    response
        .headers_mut()
        .append("set-cookie", "b=2".parse().unwrap());
    response.extensions_mut().insert(42_u64);
    let response = validators.validate(&route(), &request(), response).await;
    assert_eq!(response.headers().get_all("set-cookie").iter().count(), 2);
    assert_eq!(response.extensions().get::<u64>(), Some(&42));
    assert_eq!(
        to_bytes(response.into_body(), 64).await.unwrap(),
        "{\"ok\":true}"
    );
}

#[tokio::test]
async fn warning_none_and_unregistered_responses_are_not_buffered() {
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let mut validators = ResponseValidators::default();
    validators
        .insert(&route(), 1, Duration::from_millis(1), move |_, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            None
        })
        .unwrap();
    for mode in [ValidationMode::Warning, ValidationMode::None] {
        let mut contract = route();
        contract.options.response_validation = mode;
        let response = validators
            .validate(
                &contract,
                &request(),
                Response::new(Body::from(vec![255_u8; 10])),
            )
            .await;
        assert_eq!(response.status(), 200);
        assert_eq!(to_bytes(response.into_body(), 10).await.unwrap().len(), 10);
    }
    let response = ResponseValidators::default()
        .validate(
            &route(),
            &request(),
            Response::new(Body::from(vec![255_u8])),
        )
        .await;
    assert_eq!(response.status(), 200);
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

struct Unreadable(bool);
impl http_body::Body for Unreadable {
    type Data = bytes::Bytes;
    type Error = std::io::Error;
    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        if self.0 {
            Poll::Pending
        } else {
            Poll::Ready(Some(Err(std::io::Error::other("PRIVATE_RESPONSE_DETAIL"))))
        }
    }
}

#[tokio::test]
async fn read_limit_utf8_io_and_timeout_failures_are_safe() {
    let mut validators = ResponseValidators::default();
    validators
        .insert(&route(), 4, Duration::from_millis(5), |_, _| {
            panic!("failed read reached validator")
        })
        .unwrap();
    for body in [
        Body::from("PRIVATE_PAYLOAD"),
        Body::from(vec![255_u8]),
        Body::new(Unreadable(false)),
        Body::new(Unreadable(true)),
    ] {
        let response = validators
            .validate(&route(), &request(), Response::new(body))
            .await;
        assert_eq!(response.status(), 500);
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("internal server error"));
        assert!(text.contains("response-test"));
        assert!(!text.contains("PRIVATE"));
    }
}

#[tokio::test]
async fn replacement_preserves_metadata_without_stale_body_headers() {
    let mut validators = ResponseValidators::default();
    validators
        .insert(&route(), 64, Duration::from_secs(1), |_, _| {
            Some(FrameworkResponse::text(500, "public diagnostic"))
        })
        .unwrap();
    let mut response = Response::new(Body::from("42"));
    response
        .headers_mut()
        .insert("x-result", "valid".parse().unwrap());
    response
        .headers_mut()
        .insert("content-length", "2".parse().unwrap());
    response
        .headers_mut()
        .insert("etag", "old".parse().unwrap());
    for name in [
        "digest",
        "content-digest",
        "repr-digest",
        "content-md5",
        "trailer",
        "last-modified",
    ] {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(name),
            "stale".parse().unwrap(),
        );
    }
    response
        .headers_mut()
        .append("set-cookie", "a=1".parse().unwrap());
    response
        .headers_mut()
        .append("set-cookie", "b=2".parse().unwrap());
    response.extensions_mut().insert(42_u64);
    let response = validators.validate(&route(), &request(), response).await;
    assert_eq!(response.status(), 500);
    for name in [
        "digest",
        "content-digest",
        "repr-digest",
        "content-md5",
        "trailer",
        "last-modified",
    ] {
        assert!(!response.headers().contains_key(name));
    }
    assert_eq!(response.headers().get_all("set-cookie").iter().count(), 2);
    assert_eq!(response.headers()["x-result"], "valid");
    assert!(!response.headers().contains_key("content-length"));
    assert!(!response.headers().contains_key("etag"));
    assert_eq!(response.extensions().get::<u64>(), Some(&42));
}

#[tokio::test]
async fn head_preserves_get_headers_without_validating_a_stripped_body() {
    use crate::{ForkLaunchRouter, RouterConfig};
    use forklaunch_core::ServiceInfo;
    use tower::ServiceExt;
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let mut validators = ResponseValidators::default();
    validators
        .insert(&route(), 64, Duration::from_secs(1), move |_, response| {
            count.fetch_add(1, Ordering::SeqCst);
            (response.body.as_text() != Some("payload"))
                .then(|| FrameworkResponse::text(500, "missing body"))
        })
        .unwrap();
    let router = ForkLaunchRouter::new(ServiceInfo::new("head", "1"))
        .get(route(), || async { ([("x-result", "valid")], "payload") })
        .with_config(RouterConfig {
            response_validators: validators,
            ..RouterConfig::default()
        })
        .into_router();
    for method in ["HEAD", "GET"] {
        let response = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method(method)
                    .uri("/response")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["x-result"], "valid");
        let bytes = to_bytes(response.into_body(), 64).await.unwrap();
        assert_eq!(
            bytes.as_ref(),
            if method == "HEAD" {
                b"".as_slice()
            } else {
                b"payload".as_slice()
            }
        );
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(method == "GET"));
    }
}

#[tokio::test]
async fn bodyless_statuses_bypass_callbacks_in_standalone_and_mixed_contracts() {
    use crate::{ForkLaunchRouter, RouterConfig};
    use forklaunch_core::ServiceInfo;
    use tower::ServiceExt;
    for status in [204, 205, 304] {
        for mixed in [false, true] {
            let contract = if mixed {
                route()
            } else {
                RouteContract::get("Response", "Response", "/response")
            }
            .with_response(status, None);
            let mut validators = ResponseValidators::default();
            validators
                .insert(&contract, 64, Duration::from_secs(1), |_, _| {
                    panic!("bodyless status reached callback")
                })
                .unwrap();
            let router = ForkLaunchRouter::new(ServiceInfo::new("bodyless", "1"))
                .get(contract, move || async move {
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        [("x-result", "valid")],
                    )
                })
                .with_config(RouterConfig {
                    response_validators: validators,
                    ..RouterConfig::default()
                })
                .into_router();
            let response = router
                .oneshot(
                    axum::http::Request::builder()
                        .uri("/response")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), status);
            assert_eq!(response.headers()["x-result"], "valid");
            assert!(to_bytes(response.into_body(), 64).await.unwrap().is_empty());
        }
    }
}
