//! Persistent in-process JSON-lines fixture for key lifecycle conformance.
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
        let input: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let now = DateTime::<Utc>::from_timestamp(input["now"].as_i64().unwrap(), 0).unwrap();
        // Reconstruct the service on every call to verify persistence ownership.
        let ring = KeyRing::new(store.clone(), "https://auth.fixture.test");
        let result = match input["op"].as_str().unwrap() {
            "jwks" => ring.jwks(now).await.unwrap(),
            "rotate" => ring.rotate(now).await.unwrap(),
            "issue" => json!(
                ring.issue(
                    "fixture-user",
                    "fixture@example.test",
                    Some("fixture-org"),
                    now
                )
                .await
                .unwrap()
            ),
            "records" => {
                let mut tx = store.begin(false).await.unwrap();
                let rows = tx.list(Model::Jwks).await.unwrap();
                tx.commit().await.unwrap();
                json!(rows)
            }
            "retire" => {
                let mut tx = store.begin(true).await.unwrap();
                for mut row in tx.list(Model::Jwks).await.unwrap() {
                    row["expiresAt"] = json!(now.to_rfc3339());
                    tx.replace(Model::Jwks, row).await.unwrap();
                }
                tx.commit().await.unwrap();
                json!(true)
            }
            "seed" => {
                let mut tx = store.begin(true).await.unwrap();
                tx.insert(Model::Jwks, input["row"].clone()).await.unwrap();
                tx.commit().await.unwrap();
                json!(true)
            }
            operation => panic!("unknown operation {operation}"),
        };
        println!("{result}");
    }
}
