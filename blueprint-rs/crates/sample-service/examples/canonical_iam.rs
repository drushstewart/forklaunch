//! Live counterpart of cli/tests/fixtures/canonical-iam/service.ts.
use forklaunch_auth::{
    JwtClaims, issue_jwt_hs256, typescript::jwt::verify_jwt_resource, verify_jwt_hs256,
};
use forklaunch_axum::{
    AuthErrorFormat, ForkLaunchRouter, HandlerErrorFormat, RouterConfig,
    session::AuthenticatedSession,
};
use forklaunch_blueprint_core::{
    contract::RouteContract,
    http::{FrameworkRequest, LocalRouter},
};
use forklaunch_blueprint_sample_service::iam_router;
use forklaunch_core::{ServiceInfo, common::FrameworkError};
use forklaunch_iam_base::InMemoryIamService;
use forklaunch_iam_interface::User;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = Arc::new(InMemoryIamService::new(vec![
        User::new("user_1", "ada@example.com"),
        User::new("user_2", "grace@example.com"),
    ]));
    let config = RouterConfig {
        auth_error_format: AuthErrorFormat::TypeScriptText,
        handler_error_format: HandlerErrorFormat::TypeScriptText,
        ..RouterConfig::default()
    }
    .with_response_authenticator(|request, _| async move {
        let claims = verify_jwt_resource(
            request.headers.get("authorization").map(String::as_str),
            None,
            |token| async move {
                let principal = verify_jwt_hs256(token, b"test-only-signing-key")
                    .map_err(|error| error.to_error_response())?;
                Ok(serde_json::to_value(principal).expect("serialize verified principal"))
            },
        )
        .await?;
        let principal = serde_json::from_value(claims).expect("verified principal shape");
        Ok(AuthenticatedSession::new(
            principal,
            serde_json::json!({"sub":"tester"}),
        ))
    });
    // Optional fixture output demonstrates framework-native issuance. This is
    // a test-only key/identity, never an application credential endpoint.
    if let Some(output) = std::env::args().nth(1) {
        let claims = JwtClaims {
            sub: "tester".into(),
            exp: 4_102_444_800,
            roles: vec!["reader".into()],
            permissions: vec!["users:read".into()],
            scopes: vec!["profile".into()],
            features: vec!["reports".into()],
            active_subscription: true,
        };
        let token = issue_jwt_hs256(&claims, b"test-only-signing-key")?;
        std::fs::write(
            output,
            serde_json::to_vec(&serde_json::json!({"token":token,"claims":claims}))?,
        )?;
    }
    let mut failure = RouteContract::get("Failure", "Failure", "/throw");
    failure.options.openapi = false;
    let mut handlers = LocalRouter::new();
    handlers.register(failure.clone(), |_: FrameworkRequest| {
        Err(FrameworkError::new(
            "fixture_internal",
            "secret fixture detail",
        ))
    });
    let errors =
        ForkLaunchRouter::from_local_router(ServiceInfo::new("errors", "1"), [failure], handlers);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    println!("READY http://{}", listener.local_addr()?);
    axum::serve(
        listener,
        iam_router(&service)
            .nest("/errors", errors)
            .with_config(config)
            .into_router(),
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
