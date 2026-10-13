//! Normalized observations compared against the actual generated TS ORM service.
use chrono::Utc;
use forklaunch_iam_better_auth::{
    cookies::SessionCookie,
    relay::{DefaultRelayHooks, RelayService, RelayTokens, sanitize_redirect_path},
    store::memory::MemoryAuthStore,
};
use serde_json::json;
use std::sync::Arc;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = RelayService::new(
        Arc::new(MemoryAuthStore::default()),
        Arc::new(DefaultRelayHooks),
        SessionCookie { secure: false },
        b"relay-fixture-secret".to_vec(),
    );
    let now = Utc::now();
    let result = service.ingest("once", RelayTokens::default(), now).await?;
    let ticket = result
        .redirect_path
        .strip_prefix("/relay/handoff?ticket=")
        .unwrap();
    let duplicate = service
        .ingest("once", RelayTokens::default(), now)
        .await
        .is_err();
    let (a, b) = tokio::join!(
        service.redeem_ticket(ticket, now),
        service.redeem_ticket(ticket, now)
    );
    let redeemed: Vec<_> = [a?, b?].into_iter().flatten().collect();
    let redemption = &redeemed[0];
    let expired = service
        .ingest("expired", RelayTokens::default(), now)
        .await?;
    let expired = expired
        .redirect_path
        .strip_prefix("/relay/handoff?ticket=")
        .unwrap();
    let redirects = [
        None,
        Some(""),
        Some("https://foreign.test"),
        Some("//foreign.test"),
        Some("/\\foreign.test"),
        Some("/\t/foreign.test"),
        Some("/\n/foreign.test"),
        Some("/\r/foreign.test"),
        Some("/"),
        Some("/dashboard?q=1#part"),
        Some("/path with spaces"),
        Some("/%2F%2Fhost"),
    ]
    .map(sanitize_redirect_path);
    println!(
        "{}",
        json!({
            "redirects": redirects, "duplicateNonce": duplicate,
            "concurrentRedemptions": redeemed.len(),
            "redemption":{"cookie":redemption.cookie,"redirectTo":redemption.redirect_to},
            "replay": service.redeem_ticket(ticket, now).await?.is_none(),
            "unknown": service.redeem_ticket("unknown", now).await?.is_none(),
            "expired": service.redeem_ticket(expired, now + chrono::Duration::minutes(5)).await?.is_none(),
        })
    );
    Ok(())
}
