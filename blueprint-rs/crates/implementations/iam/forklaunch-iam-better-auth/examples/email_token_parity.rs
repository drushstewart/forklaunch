//! JSONL fixture for the native configured email-verification token verifier.
use chrono::{DateTime, Utc};
use forklaunch_iam_better_auth::credentials::verify_email_token;
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let now = DateTime::<Utc>::from_timestamp_millis(input["now"].as_i64().unwrap()).unwrap();
        let result = match verify_email_token(
            input["token"].as_str().unwrap(),
            b"better-auth-protocol-fixture-secret-32-characters",
            now,
        ) {
            Ok(claims) => json!({"claims":claims}),
            Err(error) => json!({"error":error}),
        };
        println!("{result}");
    }
}
