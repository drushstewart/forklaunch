//! Executes production auth HTTP handlers against an owned fixture store/clock.
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use chrono::{DateTime, Utc};
use forklaunch_iam_better_auth::{
    http::AuthHttp,
    store::{AuthStore, AuthTransaction, Model, memory::MemoryAuthStore},
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead},
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
};
use tower::ServiceExt;

#[tokio::main]
async fn main() {
    let store = Arc::new(MemoryAuthStore::default());
    let clock = Arc::new(AtomicI64::new(0));
    let server_clock = clock.clone();
    let router = AuthHttp::new(
        store.clone(),
        "http://localhost:3000".into(),
        b"better-auth-protocol-fixture-secret-32-characters".to_vec(),
        vec![],
    )
    .unwrap()
    .with_clock(move || {
        DateTime::<Utc>::from_timestamp_millis(server_clock.load(Ordering::SeqCst)).unwrap()
    })
    .router();
    for line in io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line.unwrap()).unwrap();
        clock.store(input["now"].as_i64().unwrap(), Ordering::SeqCst);
        let result = match input["op"].as_str().unwrap() {
            "seed" => {
                let mut tx = store.begin(true).await.unwrap();
                for (field, model) in [
                    ("users", Model::User),
                    ("sessions", Model::Session),
                    ("accounts", Model::Account),
                    ("verifications", Model::Verification),
                ] {
                    for row in input[field].as_array().into_iter().flatten() {
                        tx.insert(model, row.clone()).await.unwrap();
                    }
                }
                tx.commit().await.unwrap();
                json!({"ok":true})
            }
            "records" => {
                let mut tx = store.begin(false).await.unwrap();
                let users = tx.list(Model::User).await.unwrap();
                let accounts = tx.list(Model::Account).await.unwrap();
                let sessions = tx.list(Model::Session).await.unwrap();
                let verifications = if input["withVerifications"].as_bool() == Some(true) {
                    Some(tx.list(Model::Verification).await.unwrap())
                } else {
                    None
                };
                tx.commit().await.unwrap();
                let mut records = json!({"users":users,"accounts":accounts,"sessions":sessions});
                if let Some(verifications) = verifications {
                    records["verifications"] = json!(verifications);
                }
                records
            }
            "request" => {
                let mut request = Request::builder()
                    .method(input["method"].as_str().unwrap())
                    .uri(input["path"].as_str().unwrap())
                    .header("host", "localhost:3000");
                for (name, value) in input["headers"].as_object().unwrap() {
                    request = request.header(name, value.as_str().unwrap());
                }
                let request = request
                    .body(Body::from(input["body"].as_str().unwrap_or("").to_owned()))
                    .unwrap();
                let response = router.clone().oneshot(request).await.unwrap();
                let status = response.status().as_u16();
                let headers: serde_json::Map<String, Value> = response
                    .headers()
                    .keys()
                    .map(|name| {
                        (
                            name.to_string(),
                            json!(
                                response
                                    .headers()
                                    .get_all(name)
                                    .iter()
                                    .map(|value| value
                                        .as_bytes()
                                        .iter()
                                        .copied()
                                        .map(char::from)
                                        .collect::<String>())
                                    .collect::<Vec<_>>()
                            ),
                        )
                    })
                    .collect();
                let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
                let body: Value = serde_json::from_slice(&bytes)
                    .unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)));
                json!({"status":status,"headers":headers,"body":body})
            }
            operation => panic!("unknown operation {operation}"),
        };
        println!("{result}");
    }
}
