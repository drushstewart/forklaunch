//! Owned model gateway wire fixture; no provider credentials or external calls.
use forklaunch_blueprint_core::{
    instance_gateway::InstanceGatewayClient, model_gateway::ModelGatewayClient,
};
use serde_json::{Value, json};
use std::io::{self, Read};

#[tokio::main]
async fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    let input: Value = serde_json::from_str(&input).unwrap();
    let transport = InstanceGatewayClient::new(
        input["base"].as_str().unwrap(),
        "model-fixture".into(),
        "model-only-fixture-secret".into(),
    )
    .unwrap();
    let client = ModelGatewayClient::new(transport);
    let mut results = Vec::new();
    for action in input["actions"].as_array().unwrap() {
        let result = match action["kind"].as_str().unwrap() {
            "models" => client.models().await,
            "chat" => client.chat_completion(&action["body"]).await,
            _ => panic!("unknown fixture action"),
        };
        results.push(match result {
            Ok(value) => json!({"ok":value}),
            Err(error) => json!({"error":true,"status":error.status,
                "retryAfterSeconds":error.retry_after_seconds,"code":error.code,
                "description":error.to_string()}),
        });
    }
    println!("{}", json!(results));
}
