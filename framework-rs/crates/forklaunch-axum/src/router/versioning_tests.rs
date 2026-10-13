use super::*;
use axum::{Extension, body::Body, http::Request};
use forklaunch_contract::{BodyContract, RequestContract, ResponseContract, VersionedContract};
use forklaunch_http::SelectedVersion;
use serde_json::json;
use tower::ServiceExt;

fn versioned_router() -> ForkLaunchRouter {
    let mut contract = RouteContract::post("Versioned", "Versioned", "/versioned");
    for (name, min, status) in [("z-first", 0, 201), ("a-last", 10, 202)] {
        let mut body = BodyContract::json("Input");
        body.schema=body.schema.with_json_schema(json!({"type":"object","required":["value"],"properties":{"value":{"type":"integer","minimum":min}}}).to_string());
        contract = contract.with_version(VersionedContract {
            version: name.into(),
            request: RequestContract::default().with_body(body),
            responses: vec![ResponseContract::new(status, None)],
        });
    }
    let service = ServiceInfo::new("versioned", "1");
    ForkLaunchRouter::new(service).post(
        contract,
        |Extension(version): Extension<SelectedVersion>| async move {
            (
                if version.version == "a-last" {
                    StatusCode::ACCEPTED
                } else {
                    StatusCode::CREATED
                },
                version.version,
            )
        },
    )
}

#[tokio::test]
async fn versioned_http_dispatch_selects_last_match_and_its_response_contract() {
    let router = versioned_router().into_router();
    for (value, status, name) in [(1, 201, "z-first"), (11, 202, "a-last"), (-1, 400, "")] {
        let response = router
            .clone()
            .oneshot(
                Request::post("/versioned")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"value":value}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        if status != 400 {
            assert_eq!(to_bytes(response.into_body(), 1024).await.unwrap(), name);
        }
    }
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn versioned_http_and_mcp_start_discover_invoke_and_shutdown() {
    let app = versioned_router().into_router_with_default_mcp().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let client = reqwest::Client::new();
    let base = format!("http://{address}");
    let response = client
        .post(format!("{base}/versioned"))
        .json(&json!({"value":11}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 202);
    assert_eq!(response.text().await.unwrap(), "a-last");
    for (method, params) in [
        (
            "initialize",
            json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"version-test","version":"1"}}),
        ),
        ("tools/list", json!({})),
        (
            "tools/call",
            json!({"name":"versioned [vz-first]","arguments":{"body":{"value":11}}}),
        ),
        (
            "tools/call",
            json!({"name":"versioned [va-last]","arguments":{"body":{"value":11}}}),
        ),
    ] {
        let response = client
            .post(format!("{base}/mcp"))
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-11-25")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        let payload: serde_json::Value = response.json().await.unwrap();
        assert!(payload.get("error").is_none(), "{payload}");
        if method == "tools/list" {
            let tools = payload["result"]["tools"].as_array().unwrap();
            assert_eq!(tools.len(), 2);
            assert!(
                tools
                    .iter()
                    .any(|tool| tool["name"] == "versioned [vz-first]")
            );
            assert!(
                tools
                    .iter()
                    .any(|tool| tool["name"] == "versioned [va-last]")
            );
        }
        if method == "tools/call" {
            assert_eq!(payload["result"]["isError"], false, "{payload}");
            assert!(
                payload["result"]["content"].to_string().contains("a-last"),
                "{payload}"
            );
        }
    }
    stop.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn versioned_text_keeps_empty_body_and_uses_the_first_declared_parser() {
    use forklaunch_contract::{BodyKind, SchemaRef};
    let mut contract = RouteContract::post("Text", "Text", "/text");
    for (name, min, status) in [("v1", 0, 201), ("v2", 2, 202)] {
        let body = BodyContract {
            kind: BodyKind::Text,
            schema: SchemaRef::named("Text")
                .with_json_schema(json!({"type":"string","minLength":min}).to_string()),
            content_type: Some("text/plain".into()),
        };
        contract = contract.with_version(VersionedContract {
            version: name.into(),
            request: RequestContract::default().with_body(body),
            responses: vec![ResponseContract::new(status, None)],
        });
    }
    let app = ForkLaunchRouter::new(ServiceInfo::new("text", "1"))
        .post(
            contract,
            |Extension(version): Extension<SelectedVersion>, body: String| async move {
                (
                    if version.version == "v1" {
                        StatusCode::CREATED
                    } else {
                        StatusCode::ACCEPTED
                    },
                    format!("{}:{body}", version.version),
                )
            },
        )
        .into_router();
    for (body, status, expected) in [("", 201, "v1:"), ("a", 201, "v1:a"), ("水a", 202, "v2:水a")]
    {
        let response = app
            .clone()
            .oneshot(
                Request::post("/text")
                    .header("content-type", "text/plain")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        assert_eq!(
            to_bytes(response.into_body(), 1024).await.unwrap(),
            expected
        );
    }
    let response = app
        .oneshot(
            Request::post("/text")
                .header("content-type", "text/plain")
                .body(Body::from(vec![255]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn versioned_files_preserve_non_utf8_empty_and_json_looking_bytes() {
    let mut contract = RouteContract::post("File", "File", "/file");
    for name in ["v1", "v2"] {
        let body = BodyContract {
            kind: BodyKind::File,
            schema: forklaunch_contract::SchemaRef::named("File")
                .with_json_schema(json!({"type":"string","format":"binary"}).to_string()),
            content_type: Some("application/octet-stream".into()),
        };
        contract = contract.with_version(VersionedContract {
            version: name.into(),
            request: RequestContract::default().with_body(body),
            responses: vec![ResponseContract::new(202, None)],
        });
    }
    let app = ForkLaunchRouter::new(ServiceInfo::new("file", "1"))
        .post(
            contract,
            |Extension(version): Extension<SelectedVersion>, body: bytes::Bytes| async move {
                assert_eq!(version.version, "v2");
                assert_eq!(version.matched_versions, ["v1", "v2"]);
                (StatusCode::ACCEPTED, body)
            },
        )
        .into_router();
    for bytes in [
        vec![],
        vec![0, 255, 128],
        b"{\"value\":1}".to_vec(),
        "水".as_bytes().to_vec(),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post("/file")
                    .header("content-type", "application/octet-stream; charset=binary")
                    .body(Body::from(bytes.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 202);
        assert_eq!(
            to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
            bytes
        );
    }
}

#[tokio::test]
async fn versioned_multipart_keeps_wire_bytes_and_respects_shared_limits() {
    let mut contract = RouteContract::post("Multipart", "Multipart", "/multipart");
    for (name, minimum, status) in [("v1", 1, 201), ("v2", 2, 202)] {
        let body = BodyContract {
            kind: BodyKind::Multipart,
            schema: forklaunch_contract::SchemaRef::named("Multipart").with_json_schema(
                json!({"type":"object","required":["label"],"properties":{"label":{"type":"string","minLength":minimum}}}).to_string()),
            content_type: Some("multipart/form-data".into()),
        };
        contract = contract.with_version(VersionedContract {
            version: name.into(),
            request: RequestContract::default().with_body(body),
            responses: vec![ResponseContract::new(status, None)],
        });
    }
    let app = ForkLaunchRouter::new(ServiceInfo::new("multipart", "1"))
        .post(
            contract,
            |Extension(version): Extension<SelectedVersion>, body: bytes::Bytes| async move {
                (
                    if version.version == "v1" {
                        StatusCode::CREATED
                    } else {
                        StatusCode::ACCEPTED
                    },
                    body,
                )
            },
        )
        .into_router();
    for (text, status) in [("a", 201), ("水a", 202), ("", 400)] {
        let wire = format!(
            "--b\r\nContent-Disposition: form-data; name=\"label\"\r\n\r\n{text}\r\n--b--\r\n"
        );
        let request = || {
            Request::post("/multipart")
                .header("content-type", "multipart/form-data; boundary=b")
                .body(Body::from(wire.clone()))
                .unwrap()
        };
        let response = app.clone().oneshot(request()).await.unwrap();
        assert_eq!(response.status().as_u16(), status);
        if status != 400 {
            assert_eq!(to_bytes(response.into_body(), 1024).await.unwrap(), wire);
        }
        let limited = app
            .clone()
            .layer(Extension(forklaunch_http::MultipartLimits {
                parts: 1,
                field_bytes: 1,
                total_bytes: 1,
            }));
        let response = limited.oneshot(request()).await.unwrap();
        assert_eq!(
            response.status().as_u16(),
            if text == "a" {
                201
            } else if text.is_empty() {
                400
            } else {
                413
            }
        );
    }
}

#[tokio::test]
async fn parser_and_transport_limits_remain_fatal_when_validation_is_disabled() {
    let mut route = RouteContract::post("Parse", "Parse", "/parse");
    route.options.request_validation = ValidationMode::None;
    route = route.with_version(VersionedContract {
        version: "v1".into(),
        request: RequestContract::default().with_body(BodyContract::json("NamedInput")),
        responses: vec![],
    });
    let app = ForkLaunchRouter::new(ServiceInfo::new("parse", "1"))
        .with_config(RouterConfig {
            body_limit: 8,
            ..Default::default()
        })
        .post(route, || async { StatusCode::IM_A_TEAPOT })
        .into_router();
    for (wire, status) in [("{", 400), ("0123456789", 413)] {
        let response = app
            .clone()
            .oneshot(
                Request::post("/parse")
                    .header("content-type", "application/json")
                    .body(Body::from(wire))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
    }
    let response = body_read_error_response(&axum::Error::new(std::io::Error::other(
        "fixture private I/O failure",
    )));
    assert_eq!(response.status(), 400);
    let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("private"));
}
