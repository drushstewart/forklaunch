//! Native credential behavior bridge for the actual configured email handlers.
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::BlueprintResult;
use forklaunch_iam_better_auth::{
    accounts::{CredentialService, SigninInput, SignupInput},
    sessions::SessionService,
    store::{AuthStore, AuthTransaction, Model, memory::MemoryAuthStore},
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead},
    sync::Arc,
};

#[tokio::main]
async fn main() {
    let store = Arc::new(MemoryAuthStore::default());
    let service = CredentialService::new(store.clone());
    let sessions = SessionService::new(store.clone());
    for line in io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let now = DateTime::<Utc>::from_timestamp_millis(input["now"].as_i64().unwrap()).unwrap();
        let result: BlueprintResult<Value> =
            match input["op"].as_str().unwrap() {
                "signup" => {
                    let payload: SignupInput =
                        serde_json::from_value(input["payload"].clone()).unwrap();
                    service.sign_up(payload, "", "", now).await.map(
                        |grant| json!({"body":grant.signup_body(),"persistent":grant.persistent}),
                    )
                }
                "signin" => {
                    let payload: SigninInput =
                        serde_json::from_value(input["payload"].clone()).unwrap();
                    service.sign_in(payload, "", "", now).await.map(
                        |grant| json!({"body":grant.signin_body(),"persistent":grant.persistent}),
                    )
                }
                "get" => sessions
                    .get(
                        input["token"].as_str().unwrap(),
                        input["transient"].as_bool().unwrap_or(false),
                        false,
                        now,
                    )
                    .await
                    .map(|view| json!({"body":view.map_or(Value::Null,|view|view.body())})),
                "records" => {
                    let mut tx = store.begin(false).await.unwrap();
                    let users = tx.list(Model::User).await.unwrap();
                    let accounts = tx.list(Model::Account).await.unwrap();
                    let sessions = tx.list(Model::Session).await.unwrap();
                    tx.commit().await.unwrap();
                    Ok(json!({"users":users,"accounts":accounts,"sessions":sessions}))
                }
                operation => panic!("unknown operation {operation}"),
            };
        println!(
            "{}",
            match result {
                Ok(value) => value,
                Err(error) => json!({"error":error.code}),
            }
        );
    }
}
