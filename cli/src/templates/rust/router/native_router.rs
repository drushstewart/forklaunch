use forklaunch_axum::ForkLaunchRouter;
use forklaunch_contract::{BodyContract, BodyKind, RouteContract, SchemaRef};

pub const ROUTE_NAME: &str = "{{{router_name}}}";

pub fn contract() -> RouteContract {
    RouteContract::get("Get {{{route_title}}}", "Custom module route", format!("/{ROUTE_NAME}"))
        .with_response(200, Some(BodyContract { kind: BodyKind::Json, content_type: Some("application/json".into()), schema: SchemaRef::named("{{{response_name}}}").with_json_schema(
            r#"{"type":"object","required":["route","status"],"properties":{"route":{"type":"string"},"status":{"type":"string"}}}"#,
        ) }))
}

pub fn contracts() -> Vec<RouteContract> { vec![contract()] }

pub fn register(router: ForkLaunchRouter) -> ForkLaunchRouter {
    router.get(contract(), || async {
        axum::Json(serde_json::json!({"route": ROUTE_NAME, "status": "ok"}))
    })
}
