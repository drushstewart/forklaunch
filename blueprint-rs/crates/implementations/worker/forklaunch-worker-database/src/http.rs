//! Optional health and bearer-authenticated process-local worker controls.
use crate::{
    control::{WorkerControl, WorkerState},
    retention::{self, RetainedRunReport, RetentionPolicy},
    runner::{self, JobHandler, RunnerConfig},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use forklaunch_worker_interface::AsyncWorkerAdmin;
use serde_json::{Value, json};
use std::{
    future::{Future, IntoFuture},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::sync::watch;

/// No HTTP listener is created unless this configuration is supplied.
/// A missing token enables health/readiness only. Keep the configured secret out
/// of logs; this type intentionally does not implement Debug.
pub struct HttpConfig {
    pub address: SocketAddr,
    pub control_token: Option<String>,
}
impl HttpConfig {
    /// # Errors
    /// Rejects empty, short, oversized, or whitespace-bearing control tokens.
    pub fn validate(&self) -> BlueprintResult<()> {
        if self.control_token.as_ref().is_some_and(|token| {
            !(32..=4096).contains(&token.len())
                || token.bytes().any(|byte| !byte.is_ascii_graphic())
        }) {
            return Err(BlueprintError::new(
                "worker_invalid_http_config",
                "Worker control token must contain 32..=4096 printable ASCII characters without whitespace",
            ));
        }
        Ok(())
    }
}
#[derive(Clone)]
struct HttpState {
    control: WorkerControl,
    token: Option<Arc<str>>,
}
fn snapshot(control: &WorkerControl) -> Value {
    json!({"state":match control.state() {
        WorkerState::Running => "running",
        WorkerState::Paused => "paused",
        WorkerState::Draining => "draining",
    }})
}
async fn health(State(state): State<HttpState>) -> Json<Value> {
    Json(snapshot(&state.control))
}
async fn ready(State(state): State<HttpState>) -> Response {
    let status = if state.control.state() == WorkerState::Running {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(snapshot(&state.control))).into_response()
}
async fn command(
    State(state): State<HttpState>,
    Path(action): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(token) = state.token else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let values = headers.get_all(header::AUTHORIZATION);
    let mut values = values.iter();
    let bearer = values
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if values.next().is_some()
        || !bearer.is_some_and(|value| bool::from(value.as_bytes().ct_eq(token.as_bytes())))
    {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
        )
            .into_response();
    }
    let accepted = match action.as_str() {
        "pause" => state.control.pause(),
        "resume" => state.control.resume(),
        "drain" => {
            state.control.drain();
            true
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    (
        if accepted {
            StatusCode::ACCEPTED
        } else {
            StatusCode::CONFLICT
        },
        Json(snapshot(&state.control)),
    )
        .into_response()
}
/// Builds internal health/readiness/control routes for one process. Readiness
/// describes consumer state, not database connectivity. No payloads are exposed.
/// # Errors
/// Rejects invalid control token configuration.
pub fn router(control: &WorkerControl, config: &HttpConfig) -> BlueprintResult<Router> {
    config.validate()?;
    Ok(Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/control/{action}", post(command))
        .with_state(HttpState {
            control: control.clone(),
            token: config.control_token.as_deref().map(Arc::from),
        }))
}
fn server_error() -> BlueprintError {
    BlueprintError::new("worker_http_error", "Worker HTTP server failed")
}

/// Runs optional HTTP alongside controlled consumers and retention. The listener
/// stays available during handler drain, with readiness returning 503. Server
/// errors stop claims and drain work; HTTP shutdown is bounded to five seconds.
/// # Errors
/// Returns configuration, bind/server, queue, or retention failures after drain.
pub async fn run<Q, H, F>(
    queue: &Q,
    handler: &H,
    config: &RunnerConfig,
    policy: Option<&RetentionPolicy>,
    http: Option<&HttpConfig>,
    shutdown: F,
) -> BlueprintResult<RetainedRunReport>
where
    Q: AsyncWorkerAdmin,
    H: JobHandler,
    F: Future<Output = ()> + Send,
{
    run_with_control(
        queue,
        handler,
        config,
        policy,
        http,
        &WorkerControl::new(),
        shutdown,
    )
    .await
}

/// Runs worker, retention, and HTTP management with a shared caller-owned control.
/// Distributed signal forwarding can use this same control while HTTP commands
/// retain their existing process-local behavior. Drain remains irreversible.
/// # Errors
/// Returns configuration, worker, retention or HTTP-server errors after shutdown.
pub async fn run_with_control<Q, H, F>(
    queue: &Q,
    handler: &H,
    config: &RunnerConfig,
    policy: Option<&RetentionPolicy>,
    http: Option<&HttpConfig>,
    control: &WorkerControl,
    shutdown: F,
) -> BlueprintResult<RetainedRunReport>
where
    Q: AsyncWorkerAdmin,
    H: JobHandler,
    F: Future<Output = ()> + Send,
{
    config.validate()?;
    if let Some(policy) = policy {
        policy.validate()?;
    }
    let server = if let Some(http) = http {
        let router = router(control, http)?;
        let listener = tokio::net::TcpListener::bind(http.address)
            .await
            .map_err(|_| server_error())?;
        Some((listener, router))
    } else {
        None
    };
    let (stop, mut receiver) = watch::channel(false);
    let serving = async {
        if let Some((listener, router)) = server {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    if !*receiver.borrow() {
                        let _ = receiver.changed().await;
                    }
                })
                .into_future()
                .await
                .map_err(|_| server_error())
        } else {
            std::future::pending::<BlueprintResult<()>>().await
        }
    };
    let work = async {
        if let Some(policy) = policy {
            retention::run_worker_controlled(
                queue,
                handler,
                config,
                policy,
                control,
                std::future::pending(),
            )
            .await
        } else {
            runner::run_controlled(queue, handler, config, control, std::future::pending())
                .await
                .map(|worker| RetainedRunReport {
                    worker,
                    ..RetainedRunReport::default()
                })
        }
    };
    tokio::pin!(serving, work, shutdown);
    let mut stopping = false;
    let mut server_done = false;
    let mut first_error = None;
    let result = loop {
        tokio::select! {
            biased;
            () = &mut shutdown, if !stopping => { stopping=true; control.drain(); }
            result = &mut serving, if !server_done => {
                server_done=true;
                stopping=true;
                control.drain();
                first_error=Some(result.err().unwrap_or_else(server_error));
            }
            result = &mut work => break result,
        }
    };
    stop.send_replace(true);
    if http.is_some() && !server_done {
        // Dropping the serve future after this deadline bounds persistent clients.
        if let Ok(Err(error)) = tokio::time::timeout(Duration::from_secs(5), &mut serving).await
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    first_error.map_or(result, Err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    const TOKEN: &str = "test-only-control-token-with-32-characters";
    async fn request(app: &Router, method: &str, path: &str, token: Option<&str>) -> Response {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, token);
        }
        app.clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }
    fn config(token: Option<&str>) -> HttpConfig {
        HttpConfig {
            address: "127.0.0.1:0".parse().unwrap(),
            control_token: token.map(str::to_owned),
        }
    }
    #[tokio::test]
    async fn controls_require_explicit_bearer_and_drain_remains_irreversible() {
        let control = WorkerControl::new();
        let app = router(&control, &config(Some(TOKEN))).unwrap();
        for token in [
            None,
            Some("Bearer wrong"),
            Some(TOKEN),
            Some("Basic invalid"),
        ] {
            let response = request(&app, "POST", "/control/pause", token).await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
            assert_eq!(control.state(), WorkerState::Running);
        }
        let auth = format!("Bearer {TOKEN}");
        for action in ["pause", "resume", "drain"] {
            assert_eq!(
                request(&app, "POST", &format!("/control/{action}"), Some(&auth))
                    .await
                    .status(),
                StatusCode::ACCEPTED
            );
            let ready = request(&app, "GET", "/ready", None).await;
            assert_eq!(
                ready.status(),
                if action == "resume" {
                    StatusCode::OK
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                }
            );
        }
        assert_eq!(
            request(&app, "POST", "/control/resume", Some(&auth))
                .await
                .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            request(&app, "GET", "/control/resume", Some(&auth))
                .await
                .status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            request(&app, "POST", "/control/unknown", Some(&auth))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        let health = request(&app, "GET", "/health", None).await;
        assert_eq!(health.status(), StatusCode::OK);
        let body = to_bytes(health.into_body(), 1024).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({"state":"draining"})
        );
        assert!(!String::from_utf8_lossy(&body).contains(TOKEN));
    }
    #[tokio::test]
    async fn health_only_listener_disables_controls_and_duplicate_credentials_are_rejected() {
        let control = WorkerControl::new();
        let app = router(&control, &config(None)).unwrap();
        assert_eq!(
            request(&app, "GET", "/health", None).await.status(),
            StatusCode::OK
        );
        assert_eq!(
            request(&app, "POST", "/control/drain", Some(TOKEN))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        let app = router(&control, &config(Some(TOKEN))).unwrap();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/control/drain")
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(control.state(), WorkerState::Running);
    }
    #[test]
    fn rejects_unusable_tokens_without_echoing_secrets() {
        for token in [
            String::new(),
            "short".to_owned(),
            "x".repeat(4097),
            format!("{}\n", "x".repeat(32)),
            "é".repeat(32),
        ] {
            let error = config(Some(&token)).validate().unwrap_err();
            assert_eq!(error.code, "worker_invalid_http_config");
        }
        assert!(config(None).validate().is_ok());
        assert!(config(Some(TOKEN)).validate().is_ok());
    }
}
