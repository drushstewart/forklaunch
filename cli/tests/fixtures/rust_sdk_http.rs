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
