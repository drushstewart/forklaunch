//! Installed-registry smoke service; no repository path dependencies.
use axum::Json;
use forklaunch_axum::ForkLaunchRouter;
use forklaunch_contract::{BodyContract, RouteContract};
use forklaunch_core::ServiceInfo;
use serde_json::{Value, json};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    for literal in [
        "$serde_json::private::Number",
        "$serde_json::private::RawValue",
    ] {
        let expected = json!({literal: "literal user data"});
        assert!(expected.get(literal).is_some());
        let roundtrip: Value = serde_json::from_str(&expected.to_string())?;
        assert_eq!(roundtrip, expected, "required parser patch was not applied");
    }
    let host = std::env::var("SERVICE_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("SERVICE_PORT").unwrap_or_else(|_| "0".into());
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}")).await?;
    let router = ForkLaunchRouter::new(ServiceInfo::new("registry-smoke", "0.1.0")).get(
        RouteContract::get("Health", "Registry-installed service health", "/health")
            .with_response(200, Some(BodyContract::json("Health"))),
        || async { Json(json!({"ok": true})) },
    );
    println!(
        "registry-smoke listening on http://{}",
        listener.local_addr()?
    );
    router.serve_with_default_mcp(listener, shutdown()).await?;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .expect("install shutdown handler");
}
