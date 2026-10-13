//! Live counterpart of cli/tests/fixtures/canonical-iam/service.ts.
use forklaunch_auth::{AuthError, parse_bearer, verify_jwt_hs256};
use forklaunch_axum::{RouterConfig, session::AuthenticatedSession};
use forklaunch_blueprint_sample_service::iam_router;
use forklaunch_iam_base::InMemoryIamService;
use forklaunch_iam_interface::User;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = Arc::new(InMemoryIamService::new(vec![
        User::new("user_1", "ada@example.com"),
        User::new("user_2", "grace@example.com"),
    ]));
    let config = RouterConfig::default().with_authenticator(|request, _| async move {
        let token = request
            .headers
            .get("authorization")
            .and_then(|header| parse_bearer(header))
            .ok_or(AuthError::MissingCredentials)?;
        let principal = verify_jwt_hs256(token, b"test-only-signing-key")?;
        Ok(AuthenticatedSession::new(
            principal,
            serde_json::json!({"sub":"tester"}),
        ))
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    println!("READY http://{}", listener.local_addr()?);
    axum::serve(
        listener,
        iam_router(&service).with_config(config).into_router(),
    )
    .with_graceful_shutdown(async {
        #[cfg(unix)]
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
        #[cfg(not(unix))]
        tokio::signal::ctrl_c()
            .await
            .expect("install shutdown handler");
    })
    .await?;
    Ok(())
}
