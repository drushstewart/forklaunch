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
