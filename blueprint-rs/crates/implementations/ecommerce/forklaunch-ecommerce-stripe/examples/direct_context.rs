//! Owned protocol oracle; no external provider endpoint is used.
use forklaunch_blueprint_core::provider_http::ProviderHttpClient;
use forklaunch_ecommerce_stripe::StripeClient;
use std::time::Duration;
#[tokio::main]
async fn main() {
    let origin = std::env::args().nth(1).expect("owned HTTP origin");
    let client = StripeClient::with_transport(
        "owned-context-secret".into(),
        ProviderHttpClient::new(&origin, Duration::from_secs(2), 4096).unwrap(),
    )
    .unwrap();
    let context = client
        .verify_direct_context("acct_owned", false)
        .await
        .unwrap();
    let intent = client
        .retrieve_intent_in_context(&context, "pi_owned")
        .await
        .unwrap();
    println!(
        "{}",
        serde_json::json!({"account":context.account_id(),"livemode":context.livemode(),"intent":intent["id"]})
    );
}
