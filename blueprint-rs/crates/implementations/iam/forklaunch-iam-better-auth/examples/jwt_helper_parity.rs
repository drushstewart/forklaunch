//! Server-only JWT helpers against an owned persistent JSON-lines fixture.
#[path = "../src/ecmascript/json.rs"]
mod literal_json;
use chrono::{DateTime, Utc};
use forklaunch_iam_better_auth::{
    keyring::KeyRing,
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
        let now = DateTime::<Utc>::from_timestamp(input["now"].as_i64().unwrap(), 0).unwrap();
        let service = KeyRing::new(store.clone(), "http://localhost:3000");
        let result = match input["op"].as_str().unwrap() {
            "seed" => {
                let mut tx = store.begin(true).await.unwrap();
                tx.insert(Model::Jwks, input["row"].clone()).await.unwrap();
                tx.commit().await.unwrap();
                json!({"ok":true})
            }
            "replace" => {
                let mut tx = store.begin(true).await.unwrap();
                tx.replace(Model::Jwks, input["row"].clone()).await.unwrap();
                tx.commit().await.unwrap();
                json!({"ok":true})
            }
            "verify" => match service.verify_jwt(input.get("body"), now).await {
                Ok(body) => json!({"body":body}),
                Err(error) => json!({"error":{"code":error.code,"message":error.message}}),
            },
            op => panic!("unknown operation {op}"),
        };
        println!("{result}");
    }
}
