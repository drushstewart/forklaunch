//! Actual OAuth state/cookie/database conformance bridge; no provider I/O is stubbed.
#[path = "../src/ecmascript/json.rs"]
mod literal_json;
use chrono::{DateTime, Utc};
use forklaunch_iam_better_auth::{
    oauth::state::StateService,
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
    for line in io::stdin().lock().lines() {
        let input: Value = literal_json::parse(&line.unwrap()).unwrap();
        let now = DateTime::<Utc>::from_timestamp_millis(input["now"].as_i64().unwrap()).unwrap();
        let service = StateService::new(
            store.clone(),
            b"better-auth-protocol-fixture-secret-32-characters".to_vec(),
            input["secure"] == true,
        )
        .unwrap();
        let result = match input["op"].as_str().unwrap() {
            "seed" => {
                let mut tx = store.begin(true).await.unwrap();
                for row in tx.list(Model::Verification).await.unwrap() {
                    tx.delete(Model::Verification, row["id"].as_str().unwrap())
                        .await
                        .unwrap();
                }
                for row in input["rows"].as_array().unwrap() {
                    tx.insert(Model::Verification, row.clone()).await.unwrap();
                }
                tx.commit().await.unwrap();
                json!({"ok":true})
            }
            "records" => {
                let mut tx = store.begin(false).await.unwrap();
                let rows = tx.list(Model::Verification).await.unwrap();
                tx.commit().await.unwrap();
                json!(rows)
            }
            "consume" => match service
                .consume(
                    input["state"].as_str().unwrap_or(""),
                    input["cookie"].as_str(),
                    now,
                )
                .await
            {
                Ok(value) => json!({"body":value.data,"cookies":[value.clear_cookie]}),
                Err(error) => {
                    let mut body = json!({"code":error.code});
                    if let Some(url) = error.details.get("errorURL") {
                        body["errorURL"] = url.clone();
                    }
                    let cookies = if error.details.get("clearCookie") == Some(&json!(true)) {
                        vec![format!(
                            "{}better-auth.state=; Max-Age=0; Path=/; HttpOnly;{} SameSite=Lax",
                            if input["secure"] == true {
                                "__Secure-"
                            } else {
                                ""
                            },
                            if input["secure"] == true {
                                " Secure;"
                            } else {
                                ""
                            }
                        )]
                    } else {
                        vec![]
                    };
                    json!({"error":body,"cookies":cookies})
                }
            },
            _ => panic!("Unknown OAuth state operation"),
        };
        println!("{result}");
    }
}
