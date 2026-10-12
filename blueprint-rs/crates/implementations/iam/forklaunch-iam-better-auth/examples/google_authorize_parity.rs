//! Actual provider URL conformance with no network boundary required.
#[path = "../src/ecmascript/json.rs"]
mod literal_json;
use forklaunch_iam_better_auth::oauth::google_authorize::GoogleAuthorization;
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let input: Value = literal_json::parse(&line.unwrap()).unwrap();
        let text = |field: &str| input[field].as_str().unwrap_or("");
        let provider = GoogleAuthorization::new(
            text("clientId").into(),
            text("clientSecret").into(),
            text("redirectURI").into(),
        );
        let scopes: Vec<_> = input["scopes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|value| value.as_str().unwrap().to_string())
            .collect();
        let params: Vec<_> = input["additionalParams"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(key, value)| (key.clone(), value.as_str().unwrap().to_string()))
            .collect();
        let result = match provider.authorization_url(
            text("state"),
            text("codeVerifier"),
            &scopes,
            input["loginHint"].as_str(),
            &params,
        ) {
            Ok(url) => json!({"url":url}),
            Err(error) => json!({"error":error.message}),
        };
        println!("{result}");
    }
}
