use crate::AppState;
use forklaunch_contract::{BodyContract, BodyKind, RouteContract, SchemaRef};
use forklaunch_http::{FrameworkResponse, LocalRouter};

pub const ROUTE_NAME: &str = "health";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthResponse {
    pub service: String,
    pub status: String,
    pub record_count: usize,
    pub routes: Vec<&'static str>,
}

pub fn handle(state: &AppState) -> HealthResponse {
    HealthResponse {
        service: state.config().name.clone(),
        status: "ok".to_string(),
        record_count: state.repository().count().unwrap_or_default(),
        routes: crate::route_names(),
    }
}

impl HealthResponse {
    #[must_use]
    pub fn to_json(&self) -> String {
        let routes = forklaunch_common::json_array(
            self.routes
                .iter()
                .map(|route| forklaunch_common::json_string(route)),
        );
        format!(
            "{}\"service\":{},\"status\":{},\"recordCount\":{},\"routes\":{}{}",
            "{",
            forklaunch_common::json_string(&self.service),
            forklaunch_common::json_string(&self.status),
            self.record_count,
            routes,
            "}"
        )
    }
}

#[must_use]
pub fn contract() -> RouteContract {
    RouteContract::get("Get Health", "Reports service health", "/health")
        .with_response(200, Some(health_response_body()))
}

pub fn register(router: &mut LocalRouter, state: AppState) {
    router.register(contract(), move |_request| {
        Ok(FrameworkResponse::json(200, handle(&state).to_json()))
    });
}

fn health_response_body() -> BodyContract {
    BodyContract {
        kind: BodyKind::Json,
        schema: SchemaRef::named("HealthResponse").with_json_schema(
            r#"{"type":"object","required":["service","status","recordCount","routes"],"properties":{"service":{"type":"string"},"status":{"type":"string","const":"ok"},"recordCount":{"type":"integer","minimum":0},"routes":{"type":"array","items":{"type":"string"}}}}"#,
        ),
        content_type: Some("application/json".to_string()),
    }
}
