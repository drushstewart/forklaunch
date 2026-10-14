//! One-shot fixture bridge. Private key export is only for actual JOSE testing.
use base64::{Engine, engine::general_purpose::STANDARD};
use forklaunch_iam_better_auth::jwt::SigningKey;
use serde_json::{Value, json};
use std::io::{self, Read};

fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    let input: Value = serde_json::from_str(&input).unwrap();
    let key = if let Some(encoded) = input["privateKey"].as_str() {
        SigningKey::from_pkcs8(
            "fixture-persistent-key-id",
            &STANDARD.decode(encoded).unwrap(),
        )
        .unwrap()
    } else {
        SigningKey::generate("fixture-persistent-key-id").unwrap()
    };
    let token = key
        .issue_session(
            input["baseURL"].as_str().unwrap(),
            input["userId"].as_str().unwrap(),
            input["email"].as_str().unwrap(),
            input["organizationId"].as_str(),
            input["sessionId"].as_str(),
            input["issuedAt"].as_u64().unwrap(),
        )
        .unwrap();
    println!(
        "{}",
        json!({"token": token, "jwk": key.public_jwk(), "privateKey": STANDARD.encode(key.private_pkcs8())})
    );
}
