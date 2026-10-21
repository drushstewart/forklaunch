// Current service-local SDK API. Legacy compiler assertions live in rust_sdk_http.rs.
use std::time::Duration;

use accounts_sdk::{Client, GetHealthResponses, ListUsersResponses};

#[tokio::main]
async fn main() {
    let base = std::env::args().nth(1).expect("service URL");
    tokio::time::timeout(Duration::from_secs(10), verify(&base))
        .await
        .expect("Rust SDK roundtrip deadline");
    println!("PASS Rust SDK live HTTP roundtrip");
}

async fn verify(base: &str) {
    let confidential =
        Client::new("https://url-user:url-secret@example.test/path?token=query-secret")
            .with_default_header("authorization", "Bearer header-secret")
            .with_bearer_token("auth-secret");
    let mut diagnostic = confidential.get_health_request(Default::default());
    diagnostic
        .query
        .insert("token".into(), "query-secret".into());
    diagnostic.body = Some("body-secret".into());
    diagnostic.body_bytes = Some(b"binary-secret".to_vec());
    let mut input = accounts_sdk::GetStatusRequest::default();
    input.params.mode = "param-secret".into();
    input.query = diagnostic.query.clone();
    input
        .headers
        .insert("authorization".into(), "header-secret".into());
    let rendered = format!("{confidential:?} {diagnostic:?} {input:?}");
    for secret in [
        "url-user",
        "url-secret",
        "query-secret",
        "body-secret",
        "binary-secret",
        "header-secret",
        "auth-secret",
        "param-secret",
    ] {
        assert!(!rendered.contains(secret), "SDK diagnostic leaked {secret}");
    }
    assert!(rendered.contains("authorization"));
    let error = reqwest::Client::new()
        .get("http://127.0.0.1:1/?token=transport-secret")
        .header("invalid\nname", "x")
        .build()
        .unwrap_err();
    let error: accounts_sdk::SdkError = error.into();
    assert!(!format!("{error:?} {error}").contains("transport-secret"));
    let mut multipart_input = accounts_sdk::GetMultipartResponseRequest::default();
    multipart_input.params.mode = "valid".into();
    let multipart_client = Client::new(base);
    let accounts_sdk::GetMultipartResponseResponses::Status200(form) = multipart_client
        .get_multipart_response(multipart_input.clone())
        .await
        .unwrap();
    assert_eq!(form.parts.len(), 4);
    assert_eq!(
        form.parts
            .iter()
            .map(|part| part.name.as_str())
            .collect::<Vec<_>>(),
        vec!["label", "label", "file", "file"]
    );
    assert_eq!(form.parts[0].bytes, "hello 水".as_bytes());
    assert_eq!(form.parts[1].bytes, b"again");
    assert_eq!(form.parts[2].bytes, vec![0, 255, 128, 13, 10]);
    assert_eq!(form.parts[2].file_name.as_deref(), Some("data.bin"));
    assert!(form.parts[3].bytes.is_empty());
    assert_eq!(form.parts[3].file_name.as_deref(), Some("empty.bin"));
    assert!(
        multipart_client
            .clone()
            .with_multipart_response_limit(0)
            .is_err()
    );
    assert!(
        multipart_client
            .clone()
            .with_multipart_response_limit(16)
            .unwrap()
            .get_multipart_response(multipart_input.clone())
            .await
            .is_err()
    );
    for mode in ["wrong", "missing", "invalid", "truncated", "many"] {
        multipart_input.params.mode = mode.into();
        assert!(
            multipart_client
                .get_multipart_response(multipart_input.clone())
                .await
                .is_err(),
            "accepted {mode}"
        );
    }
    multipart_input.params.mode = "empty".into();
    let accounts_sdk::GetMultipartResponseResponses::Status200(form) = multipart_client
        .get_multipart_response(multipart_input)
        .await
        .unwrap();
    assert!(form.parts.is_empty());
    let client = Client::new(format!("{base}/")).with_default_header("x-sdk-consumer", "rust");
    let GetHealthResponses::Status200(health) = client
        .get_health_with_options(Default::default())
        .await
        .expect("health transport")
    else {
        panic!("health failed");
    };
    assert_eq!(health.status, "ok");
    assert_eq!(health.record_count, 1);
    let mut calls = tokio::task::JoinSet::new();
    for _ in 0..20 {
        let client = client.clone();
        calls.spawn(async move {
            let mut input = accounts_sdk::ListUsersRequest::default();
            input.query.label = Some(Some("hello world & rust".into()));
            input
                .headers
                .insert("x-request-header".into(), "present".into());
            let ListUsersResponses::Status200(users) =
                client.list_users(input).await.expect("users transport")
            else {
                panic!("users failed");
            };
            assert_eq!(users.records[0].id, "welcome");
            assert_eq!(users.summary.total, 1);
        });
    }
    while let Some(result) = calls.join_next().await {
        result.expect("concurrent Rust SDK request");
    }
    for mode in [
        "existing", "created", "queued", "empty", "reset", "invalid", "conflict",
    ] {
        let mut input = accounts_sdk::GetStatusRequest::default();
        input.params.mode = mode.into();
        use accounts_sdk::GetStatusResponses as Status;
        match (mode, client.get_status(input).await.unwrap()) {
            ("existing", Status::Status200(value)) => assert_eq!(value.display_name, "Existing"),
            ("created", Status::Status201(value)) => {
                assert_eq!(value.created_at, "2026-10-09T00:00:00Z")
            }
            ("queued", Status::Status202(value)) => assert_eq!(value.job_id, "queued-1"),
            ("empty", Status::Status204(())) | ("reset", Status::Status205(())) => {}
            ("invalid", Status::Status400(value)) => assert_eq!(value.error_code, "invalid_mode"),
            ("conflict", Status::Status409(value)) => assert_eq!(value.retry_after, 30),
            (_, response) => panic!("Unexpected {mode} response: {response:?}"),
        }
    }
    assert!(
        matches!(client.get_text_with_options(Default::default()).await.unwrap(), accounts_sdk::GetTextResponses::Status200(value) if value == "Hello\nRust 🦀")
    );
    assert!(
        matches!(client.get_binary_with_options(Default::default()).await.unwrap(), accounts_sdk::GetBinaryResponses::Status200(value) if value == vec![0, 255, 13, 10, 42])
    );
    assert!(
        matches!(client.get_unknown_with_options(Default::default()).await.unwrap(), accounts_sdk::GetUnknownResponses::Status200(value) if value == serde_json::json!([1,"two",{"three":true}]))
    );
    let accounts_sdk::GetCollisionResponses::Status200(collision) = client
        .get_collision_with_options(Default::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(collision).unwrap(),
        serde_json::json!({"fooBar": {"first": "one"}, "foo_bar": {"second": 2}, "foo_bar2": {"third": true}, "foo_bar_2": {"fourth": "four"}, "type": "kind", "type_": 9, "gen": "future", "_": "underscore", "quote\"\\": "escaped"})
    );
    for mode in ["null", "full"] {
        let mut input = accounts_sdk::GetShapeRequest::default();
        input.params.mode = mode.into();
        let accounts_sdk::GetShapeResponses::Status200(shape) =
            client.get_shape(input).await.unwrap();
        assert_eq!(shape.numbers.get("one"), Some(&Some(1)));
        assert_eq!(shape.numbers.get("missing"), Some(&None));
        assert_eq!(
            shape.label.as_deref(),
            if mode == "full" {
                Some("present")
            } else {
                None
            }
        );
        let expected = if mode == "full" {
            serde_json::json!({"label": "present", "numbers": {"one": 1, "missing": null}, "metadata": {"id": "meta"}, "open": {"id": "open", "extra": {"nested": true}}, "constrained": {"count": 2, "extra": "text"}, "additional_properties": "explicit", "unknownExtra": [1, false], "any": "seven", "list": [1, "two", null]})
        } else {
            serde_json::json!({"label": null, "numbers": {"one": 1, "missing": null}, "metadata": null, "open": {"id": "open", "extra": {"nested": true}}, "constrained": {"count": 2, "extra": "text"}, "additional_properties": "explicit", "unknownExtra": [1, false], "any": 7, "list": [1, "two", null]})
        };
        assert_eq!(serde_json::to_value(shape).unwrap(), expected);
    }
    let empty_text = accounts_sdk::UploadTextRequest {
        body: Some(String::new()),
        ..Default::default()
    };
    assert!(
        matches!(client.upload_text(empty_text).await.unwrap(), accounts_sdk::UploadTextResponses::Status200(value) if value.is_empty())
    );
    let empty_binary = accounts_sdk::UploadBinaryRequest {
        body: Some(Vec::new()),
        ..Default::default()
    };
    assert!(
        matches!(client.upload_binary(empty_binary).await.unwrap(), accounts_sdk::UploadBinaryResponses::Status200(value) if value.is_empty())
    );
    let text_input = accounts_sdk::UploadTextRequest {
        body: Some("{raw}\n🦀 & text".into()),
        ..Default::default()
    };
    assert!(
        matches!(client.upload_text(text_input).await.unwrap(), accounts_sdk::UploadTextResponses::Status200(value) if value == "{raw}\n🦀 & text")
    );
    let binary_input = accounts_sdk::UploadBinaryRequest {
        body: Some(vec![0, 255, 13, 10, 42]),
        ..Default::default()
    };
    assert!(
        matches!(client.upload_binary(binary_input).await.unwrap(), accounts_sdk::UploadBinaryResponses::Status200(value) if value == vec![0, 255, 13, 10, 42])
    );
    for empty in [false, true] {
        let form = if empty {
            accounts_sdk::MultipartForm::new()
        } else {
            accounts_sdk::MultipartForm::new()
                .text("label", "hello 水")
                .part(
                    "file",
                    accounts_sdk::MultipartPart::bytes(vec![0, 255, 1, 13, 10])
                        .file_name("data.bin")
                        .mime_str("application/octet-stream")
                        .unwrap(),
                )
                .part(
                    "file",
                    accounts_sdk::MultipartPart::bytes(Vec::new()).file_name("empty.bin"),
                )
        };
        let mut input = accounts_sdk::UploadMultipartRequest {
            body: Some(form),
            ..Default::default()
        };
        input.headers.insert(
            "Content-Type".into(),
            "multipart/form-data; boundary=incorrect".into(),
        );
        let accounts_sdk::UploadMultipartResponses::Status200(upload) =
            client.upload_multipart(input).await.unwrap();
        assert!(
            upload
                .content_type
                .starts_with("multipart/form-data; boundary=")
        );
        assert!(!upload.content_type.contains("incorrect"));
        let bytes: Vec<u8> = upload
            .bytes
            .into_iter()
            .map(|b| u8::try_from(b).unwrap())
            .collect();
        if !empty {
            assert!(
                bytes
                    .windows("hello 水".len())
                    .any(|b| b == "hello 水".as_bytes())
            );
            assert!(bytes.windows(5).any(|b| b == [0, 255, 1, 13, 10]));
        }
    }
    let json_input = accounts_sdk::EchoJsonRequest {
        body: Some(serde_json::json!({"value":[1,null,true]})),
        ..Default::default()
    };
    assert!(
        matches!(client.echo_json(json_input).await.unwrap(), accounts_sdk::EchoJsonResponses::Status200(value) if value == serde_json::json!({"value":[1,null,true]}))
    );
    for mode in ["named", "counted", "boolean", "string", "integer"] {
        let mut input = accounts_sdk::GetUnionRequest::default();
        input.params.mode = mode.into();
        let accounts_sdk::GetUnionResponses::Status200(value) =
            client.get_union(input).await.unwrap();
        let expected = match mode {
            "named" => serde_json::json!({"name":"Rust"}),
            "counted" => serde_json::json!({"count":42}),
            "boolean" => serde_json::json!(true),
            "string" => serde_json::json!("union"),
            "integer" => serde_json::json!(7),
            _ => unreachable!(),
        };
        assert_eq!(serde_json::to_value(value).unwrap(), expected);
    }
    let accounts_sdk::GetUnionListResponses::Status200(values) = client
        .get_union_list_with_options(Default::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(values).unwrap(),
        serde_json::json!([{"name":"nested"},"two",3])
    );
    for mode in ["null", "full"] {
        let mut input = accounts_sdk::GetNullableRequest::default();
        input.params.mode = mode.into();
        let accounts_sdk::GetNullableResponses::Status200(value) =
            client.get_nullable(input).await.unwrap();
        assert_eq!(
            value.map(|value| value.name),
            if mode == "full" {
                Some("nullable".into())
            } else {
                None
            }
        );
    }
    let mut deleted = accounts_sdk::DeleteUserRequest::default();
    deleted.params.id = "welcome".into();
    assert!(matches!(
        client.delete_user(deleted).await.unwrap(),
        accounts_sdk::DeleteUserResponses::Status204(())
    ));
    assert!(matches!(
        client
            .get_empty_with_options(Default::default())
            .await
            .unwrap(),
        accounts_sdk::GetEmptyResponses::Status200(())
    ));
    assert!(matches!(
        client
            .head_health_with_options(Default::default())
            .await
            .unwrap(),
        accounts_sdk::HeadHealthResponses::Status200(())
    ));
    assert!(matches!(
        client
            .get_null_with_options(Default::default())
            .await
            .unwrap(),
        accounts_sdk::GetNullResponses::Status200(())
    ));
    let missing = Client::new(format!("{base}/missing"))
        .list_users(Default::default())
        .await;
    assert!(matches!(
        missing,
        Err(accounts_sdk::SdkError::Contract(
            "undeclared response status or content type"
        ))
    ));
    let invalid = Client::new(base)
        .with_auth_hook(|request| request.url = "://invalid".into())
        .list_users(Default::default())
        .await;
    assert!(matches!(
        invalid,
        Err(accounts_sdk::SdkError::TransportPolicy)
    ));
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let unavailable = Client::new(endpoint).list_users(Default::default()).await;
    assert!(matches!(
        unavailable,
        Err(accounts_sdk::SdkError::Transport(_))
    ));
}
