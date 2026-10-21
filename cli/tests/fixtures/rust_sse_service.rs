use std::sync::atomic::{AtomicUsize, Ordering};

use axum::{Json, body::Body, extract::Path, response::Response};
use forklaunch_core::auth::{AuthError, Principal};
use forklaunch_axum::{ForkLaunchRouter, RouterConfig, session::AuthenticatedSession};
use forklaunch_core::contract::{
    AccessLevel, BodyContract, BodyKind, FieldContract, RouteContract, SchemaRef,
};
static STARTED: AtomicUsize = AtomicUsize::new(0);
static CLOSED: AtomicUsize = AtomicUsize::new(0);
struct Active;
impl Drop for Active {
    fn drop(&mut self) {
        CLOSED.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn contracts() -> Vec<RouteContract> {
    vec![RouteContract::get("Watch Events", "Pull-driven event stream", "/events/:mode")
        .with_param(FieldContract::required("mode", "String"))
        .with_access(AccessLevel::Authenticated)
        .with_response(200, Some(BodyContract {
            kind: BodyKind::ServerSentEvent,
            schema: SchemaRef::named("StreamEvent").with_json_schema(r#"{"type":"object","required":["data"],"properties":{"data":{},"id":{"type":"string"}}}"#),
            content_type: Some("text/event-stream".into()),
        }))]
}

pub fn register(router: ForkLaunchRouter) -> ForkLaunchRouter {
    router.get(contracts().remove(0), |Path(mode): Path<String>| async move {
        STARTED.fetch_add(1, Ordering::SeqCst);
        let active = Active;
        if mode == "wrong" {
            drop(active);
            return Response::builder().header("content-type", "application/json").body(Body::from("{}")).unwrap();
        }
        if matches!(mode.as_str(), "finite" | "slow" | "broken") {
            let source = futures_util::stream::unfold((0, mode, active), |(position, mode, active)| async move {
                if position >= 2 {
                    if mode == "slow" { std::future::pending::<()>().await; }
                    if mode == "broken" && position == 2 {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        return Some((Err(std::io::Error::other("owned fixture stream failure")), (position+1, mode, active)));
                    }
                    return None;
                }
                let data = if position == 0 { serde_json::json!({"label":"水 🦀"}) } else { serde_json::json!(2) };
                Some((Ok(forklaunch_axum::sse::SseFrame::new("first", data)), (position+1, mode, active)))
            });
            return forklaunch_axum::sse::respond_with(source, forklaunch_axum::sse::SseOptions::default(), |_| true).await.unwrap();
        }
        let bytes = match mode.as_str() {
            "oversize" => format!("data: \"{}\"\n\n", "x".repeat(4096)).into_bytes(),
            "late-oversize" => format!("data: {{\"label\":\"水 🦀\"}}\n\ndata: \"{}\"\n\n", "x".repeat(4096)).into_bytes(),
            "truncated" => b"data: {\"incomplete\":true}".to_vec(),
            "multiline" => "\u{feff}id: first\r\nevent: update\r\ndata: {\r\ndata:  \"label\": \"水 🦀\"\r\ndata: }\r\n\r\nid:\rdata: false\r\r".as_bytes().to_vec(),
            _ => "id: first\ndata: {\"label\":\"水 🦀\"}\n\ndata: 2\n\n".as_bytes().to_vec(),
        };
        let stream = futures_util::stream::unfold((bytes, 0, mode, active), |(bytes, position, mode, active)| async move {
            if position == bytes.len() {
                if mode == "slow" { std::future::pending::<()>().await; }
                if mode == "broken" {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    return Some((Err(std::io::Error::other("owned fixture stream failure")), (bytes, position + 1, mode, active)));
                }
                return None;
            }
            if position > bytes.len() { return None; }
            tokio::task::yield_now().await;
            let byte = bytes[position];
            Some((Ok(vec![byte]), (bytes, position + 1, mode, active)))
        });
        Response::builder().header("content-type", "text/event-stream; charset=utf-8").body(Body::from_stream(stream)).unwrap()
    }).get(RouteContract::get("Stream Stats", "Closed stream count", "/stream-stats").with_response(200, Some(BodyContract::json("Stats"))), || async {
        Json(serde_json::json!({"started": STARTED.load(Ordering::SeqCst), "closed": CLOSED.load(Ordering::SeqCst)}))
    }).with_config(RouterConfig::default().with_authenticator(|request, _| async move {
        if request.headers.get("authorization").map(String::as_str) != Some("Bearer stream-secret") { return Err(AuthError::MissingCredentials); }
        Ok(AuthenticatedSession::new(Principal::new("stream-reader"), serde_json::json!({"sub":"stream-reader"})))
    }))
}
