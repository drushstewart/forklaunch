//! Actual Google ID-token verification conformance; certificate fetching is owned.
#[path = "../src/ecmascript/json.rs"]
mod literal_json;
use chrono::{DateTime, Utc};
use forklaunch_iam_better_auth::oauth::google_verify::verify_google_id_token;
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let input: Value = literal_json::parse(&line.unwrap()).unwrap();
        let now = DateTime::<Utc>::from_timestamp(input["now"].as_i64().unwrap(), 0).unwrap();
        let client_id = input["clientId"].as_str().unwrap();
        let result = json!({"payload":verify_google_id_token(input["token"].as_str().unwrap(), client_id, input["nonce"].as_str(), &input["jwks"], now)});
        println!("{result}");
    }
}
