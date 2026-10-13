use axum::{Extension, Json, http::StatusCode};
use forklaunch_axum::ForkLaunchRouter;
use forklaunch_contract::{BodyContract, RequestContract, ResponseContract, RouteContract, VersionedContract};
use forklaunch_http::SelectedVersion;
use serde_json::{Value,json};

pub fn contracts() -> Vec<RouteContract> {
    let mut route=RouteContract::post("Write","Write a versioned value","/versions");
    for (version,kind,status) in [("v1","string",201),("v2","integer",202)] {
        let mut input=BodyContract::json("VersionInput");
        input.schema=input.schema.with_json_schema(json!({"type":"object","additionalProperties":false,"required":["kind","value"],"properties":{"kind":{"type":"string","const":version},"value":{"type":kind}}}).to_string());
        let mut output=BodyContract::json("VersionOutput");
        output.schema=output.schema.with_json_schema(json!({"type":"object","additionalProperties":false,"required":["value"],"properties":{"value":{"type":kind}}}).to_string());
        route=route.with_version(VersionedContract{version:version.into(),request:RequestContract::default().with_body(input),responses:vec![ResponseContract::new(status,Some(output))]});
    }
    vec![route]
}
pub fn register(router:ForkLaunchRouter) -> ForkLaunchRouter {
    router.post(contracts().remove(0),|Extension(version):Extension<SelectedVersion>,Json(body):Json<Value>|async move {
        (if version.version=="v1" {StatusCode::CREATED}else{StatusCode::ACCEPTED},Json(json!({"value":body["value"]})))
    })
}
