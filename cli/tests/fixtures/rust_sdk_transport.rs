use forklaunch_contract::{BodyContract, BodyKind, FieldContract, RouteContract, SchemaRef};
use forklaunch_http::{FrameworkResponse, LocalRouter};

pub fn contracts() -> Vec<RouteContract> {
    vec![
        RouteContract::delete("Delete User", "Empty 204 response", "/users/:id")
            .with_param(FieldContract {
                name: "id".into(),
                required: true,
                schema: SchemaRef::named("String").with_json_schema(r#"{"type":"string"}"#),
            })
            .with_response(204, None),
        RouteContract::get("Get Empty", "Empty 200 response", "/empty").with_response(200, None),
        RouteContract::head("Head Health", "HEAD has no response body", "/health")
            .with_response(200, None),
        RouteContract::get("Get Null", "JSON null differs from no body", "/null-json")
            .with_response(
                200,
                Some(BodyContract {
                    kind: BodyKind::Json,
                    schema: SchemaRef::named("NullResponse").with_json_schema(r#"{"type":"null"}"#),
                    content_type: Some("application/json".into()),
                }),
            ),
    ]
}

pub fn register(router: &mut LocalRouter) {
    for contract in contracts() {
        let path = contract.path.clone();
        let status = if path.starts_with("/users/") {
            204
        } else {
            200
        };
        router.register(contract, move |_| {
            Ok(if path == "/null-json" {
                FrameworkResponse::json(status, "null")
            } else {
                FrameworkResponse::new(status)
            })
        });
    }
}
