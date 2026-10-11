//! Native session behavior bridge; credentials are seeded from actual fixtures.
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::BlueprintResult;
use forklaunch_iam_better_auth::{
    cookies,
    sessions::SessionService,
    store::{AuthStore, AuthTransaction, Model, memory::MemoryAuthStore},
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead},
    sync::Arc,
};

const SECRET: &[u8] = b"better-auth-protocol-fixture-secret-32-characters";
#[tokio::main]
async fn main() {
    let mut store = Arc::new(MemoryAuthStore::default());
    for line in io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let now = DateTime::<Utc>::from_timestamp_millis(input["now"].as_i64().unwrap()).unwrap();
        let cookie = input["cookie"].as_str().unwrap_or("");
        let signed = cookies::cookie_value(cookie, "better-auth.session_token");
        let token = signed
            .as_deref()
            .and_then(|value| cookies::verify_cookie_value(value, SECRET))
            .unwrap_or("");
        let transient = cookies::cookie_value(cookie, "better-auth.dont_remember")
            .is_some_and(|value| cookies::verify_cookie_value(&value, SECRET).is_some());
        let service = SessionService::new(store.clone());
        let result: BlueprintResult<Value> = match input["op"].as_str().unwrap() {
            "seed" => {
                store = Arc::new(MemoryAuthStore::default());
                let mut tx = store.begin(true).await.unwrap();
                for (model, field) in [(Model::User, "users"), (Model::Session, "sessions")] {
                    for row in input[field].as_array().unwrap() {
                        tx.insert(model, row.clone()).await.unwrap();
                    }
                }
                tx.commit().await.unwrap();
                Ok(json!(true))
            }
            "get" => service
                .get(
                    token,
                    transient,
                    input["disableRefresh"].as_bool().unwrap_or(false),
                    now,
                )
                .await
                .map(|view| view.map_or(Value::Null, |view| view.body())),
            "list" => service.list(token, transient, now).await,
            "one" => {
                service
                    .revoke_one(token, input["target"].as_str().unwrap(), transient, now)
                    .await
            }
            "all" => service.revoke_all(token, transient, now).await,
            "others" => service.revoke_others(token, transient, now).await,
            "signout" => service.sign_out(token).await.map(|()| json!(true)),
            "create" => {
                service
                    .create(
                        input["userId"].as_str().unwrap(),
                        "fixture-ip",
                        "fixture-agent",
                        now,
                    )
                    .await
            }
            "records" => {
                let mut tx = store.begin(false).await.unwrap();
                let rows = tx.list(Model::Session).await.unwrap();
                tx.commit().await.unwrap();
                Ok(json!(rows))
            }
            operation => panic!("unknown operation {operation}"),
        };
        println!(
            "{}",
            match result {
                Ok(body) => json!({"body":body}),
                Err(error) => json!({"error":error.code}),
            }
        );
    }
}
