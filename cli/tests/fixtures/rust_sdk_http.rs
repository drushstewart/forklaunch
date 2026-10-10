use std::time::Duration;

use accounts_sdk::{
    AccountsClient, AccountsHealthGetHealthResponses, AccountsUsersListUsersResponses,
};

#[tokio::main]
async fn main() {
    let base = std::env::args().nth(1).expect("service URL");
    tokio::time::timeout(Duration::from_secs(10), verify(&base))
        .await
        .expect("Rust SDK roundtrip deadline");
    println!("PASS Rust SDK live HTTP roundtrip");
}

async fn verify(base: &str) {
    let client =
        AccountsClient::new(format!("{base}/")).with_default_header("x-sdk-consumer", "rust");
    let AccountsHealthGetHealthResponses::Status200(health) = client
        .health_get_health(Default::default())
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
            let mut input = accounts_sdk::AccountsUsersListUsersRequest::default();
            input
                .query
                .insert("label".into(), "hello world & rust".into());
            input
                .headers
                .insert("x-request-header".into(), "present".into());
            let AccountsUsersListUsersResponses::Status200(users) = client
                .users_list_users(input)
                .await
                .expect("users transport")
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
        let mut input = accounts_sdk::AccountsStatusGetStatusRequest::default();
        input.params.insert("mode".into(), mode.into());
        use accounts_sdk::AccountsStatusGetStatusResponses as Status;
        match (mode, client.status_get_status(input).await.unwrap()) {
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
        matches!(client.text_get_text(Default::default()).await.unwrap(), accounts_sdk::AccountsTextGetTextResponses::Status200(value) if value == "Hello\nRust 🦀")
    );
    assert!(
        matches!(client.binary_get_binary(Default::default()).await.unwrap(), accounts_sdk::AccountsBinaryGetBinaryResponses::Status200(value) if value == vec![0, 255, 13, 10, 42])
    );
    assert!(
        matches!(client.unknown_get_unknown(Default::default()).await.unwrap(), accounts_sdk::AccountsUnknownGetUnknownResponses::Status200(value) if value == serde_json::json!([1,"two",{"three":true}]))
    );
    let accounts_sdk::AccountsCollisionGetCollisionResponses::Status200(collision) = client
        .collision_get_collision(Default::default())
        .await
        .unwrap()
    else {
        panic!("Expected collision payload")
    };
    assert_eq!(
        serde_json::to_value(collision).unwrap(),
        serde_json::json!({"fooBar": {"first": "one"}, "foo_bar": {"second": 2}, "foo_bar2": {"third": true}, "foo_bar_2": {"fourth": "four"}, "type": "kind", "type_": 9, "gen": "future", "_": "underscore", "quote\"\\": "escaped"})
    );
    for mode in ["null", "full"] {
        let mut input = accounts_sdk::AccountsShapeGetShapeRequest::default();
        input.params.insert("mode".into(), mode.into());
        let accounts_sdk::AccountsShapeGetShapeResponses::Status200(shape) =
            client.shape_get_shape(input).await.unwrap()
        else {
            panic!("Expected shape payload")
        };
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
    let empty_text = accounts_sdk::AccountsTextUploadTextRequest { body: Some(String::new()), ..Default::default() };
    assert!(matches!(client.text_upload_text(empty_text).await.unwrap(), accounts_sdk::AccountsTextUploadTextResponses::Status200(value) if value.is_empty()));
    let empty_binary = accounts_sdk::AccountsBinaryUploadBinaryRequest { body: Some(Vec::new()), ..Default::default() };
    assert!(matches!(client.binary_upload_binary(empty_binary).await.unwrap(), accounts_sdk::AccountsBinaryUploadBinaryResponses::Status200(value) if value.is_empty()));
    let text_input = accounts_sdk::AccountsTextUploadTextRequest {
        body: Some("{raw}\n🦀 & text".into()),
        ..Default::default()
    };
    assert!(
        matches!(client.text_upload_text(text_input).await.unwrap(), accounts_sdk::AccountsTextUploadTextResponses::Status200(value) if value == "{raw}\n🦀 & text")
    );
    let binary_input = accounts_sdk::AccountsBinaryUploadBinaryRequest {
        body: Some(vec![0, 255, 13, 10, 42]),
        ..Default::default()
    };
    assert!(
        matches!(client.binary_upload_binary(binary_input).await.unwrap(), accounts_sdk::AccountsBinaryUploadBinaryResponses::Status200(value) if value == vec![0, 255, 13, 10, 42])
    );
    let json_input = accounts_sdk::AccountsJsonEchoEchoJsonRequest {
        body: Some(r#"{"value":[1,null,true]}"#.into()),
        ..Default::default()
    };
    assert!(
        matches!(client.json_echo_echo_json(json_input).await.unwrap(), accounts_sdk::AccountsJsonEchoEchoJsonResponses::Status200(value) if value == serde_json::json!({"value":[1,null,true]}))
    );
    for mode in ["named", "counted", "boolean", "string", "integer"] {
        let mut input = accounts_sdk::AccountsUnionGetUnionRequest::default();
        input.params.insert("mode".into(), mode.into());
        let accounts_sdk::AccountsUnionGetUnionResponses::Status200(value) =
            client.union_get_union(input).await.unwrap()
        else {
            panic!("Expected union")
        };
        use accounts_sdk::AccountsUnionGetUnionResponse as Union;
        match (mode, value) {
            ("named", Union::Variant1(value)) => assert_eq!(value.name, "Rust"),
            ("counted", Union::Variant2(value)) => assert_eq!(value.count, 42),
            ("boolean", Union::Variant3(value)) => assert!(value),
            ("string", Union::Variant4(value)) => assert_eq!(value, "union"),
            ("integer", Union::Variant5(value)) => assert_eq!(value, 7),
            (_, value) => panic!("Unexpected union branch: {value:?}"),
        }
    }
    let accounts_sdk::AccountsUnionListGetUnionListResponses::Status200(values) = client
        .union_list_get_union_list(Default::default())
        .await
        .unwrap()
    else {
        panic!("Expected union list")
    };
    assert_eq!(
        serde_json::to_value(values).unwrap(),
        serde_json::json!([{"name":"nested"},"two",3])
    );
    for mode in ["null", "full"] {
        let mut input = accounts_sdk::AccountsNullableGetNullableRequest::default();
        input.params.insert("mode".into(), mode.into());
        let accounts_sdk::AccountsNullableGetNullableResponses::Status200(value) =
            client.nullable_get_nullable(input).await.unwrap()
        else {
            panic!("Expected nullable response")
        };
        assert_eq!(
            value.map(|value| value.name),
            if mode == "full" {
                Some("nullable".into())
            } else {
                None
            }
        );
    }
    let mut deleted = accounts_sdk::AccountsUsersDeleteUserRequest::default();
    deleted.params.insert("id".into(), "welcome".into());
    assert!(matches!(
        client.users_delete_user(deleted).await.unwrap(),
        accounts_sdk::AccountsUsersDeleteUserResponses::Status204(())
    ));
    assert!(matches!(
        client.empty_get_empty(Default::default()).await.unwrap(),
        accounts_sdk::AccountsEmptyGetEmptyResponses::Status200(())
    ));
    assert!(matches!(
        client.health_head_health(Default::default()).await.unwrap(),
        accounts_sdk::AccountsHealthHeadHealthResponses::Status200(())
    ));
    assert!(matches!(
        client.null_json_get_null(Default::default()).await.unwrap(),
        accounts_sdk::AccountsNullJsonGetNullResponses::Status200(())
    ));
    let missing = AccountsClient::new(format!("{base}/missing"))
        .users_list_users(Default::default())
        .await
        .expect("HTTP errors remain typed responses");
    assert!(matches!(
        missing,
        AccountsUsersListUsersResponses::Error { status: 404, .. }
    ));
    let invalid = AccountsClient::new(base)
        .with_auth_hook(|request| request.url = "://invalid".into())
        .users_list_users(Default::default())
        .await;
    assert!(matches!(invalid, Err(accounts_sdk::SdkError::Transport(_))));
}
