use forklaunch_contract::{BodyContract, BodyKind, FieldContract, RouteContract, SchemaRef};
use forklaunch_http::{FrameworkRequest, FrameworkResponse, LocalRouter};

pub fn contracts() -> Vec<RouteContract> {
    vec![
        RouteContract::delete("Delete User", "Empty 204 response", "/users/:id")
            .with_param(FieldContract {
                name: "id".into(),
                required: true,
                schema: SchemaRef::named("String").with_json_schema(r#"{"type":"string"}"#),
            })
            .with_response(204, None),
        RouteContract::get("Get Status", "Several success variants", "/status/:mode")
            .with_param(FieldContract {
                name: "mode".into(), required: true,
                schema: SchemaRef::named("String").with_json_schema(r#"{"type":"string"}"#),
            })
            .with_response(200, Some(json_body("ExistingRecord", r#"{"type":"object","required":["id","displayName"],"properties":{"id":{"type":"string"},"displayName":{"type":"string"}}}"#)))
            .with_response(201, Some(json_body("CreatedRecord", r#"{"type":"object","required":["id","createdAt"],"properties":{"id":{"type":"string"},"createdAt":{"type":"string"}}}"#)))
            .with_response(202, Some(json_body("QueuedRecord", r#"{"type":"object","required":["jobId"],"properties":{"jobId":{"type":"string"}}}"#)))
            .with_response(204, None)
            .with_response(205, None)
            .with_response(400, Some(BodyContract {
                kind: BodyKind::Text,
                schema: SchemaRef::named("ErrorText").with_json_schema(r#"{"type":"string"}"#),
                content_type: Some("text/plain".into()),
            })),
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
        router.register(contract, move |request: FrameworkRequest| {
            Ok(if path == "/status/:mode" {
                match request.params.get("mode").map(String::as_str) {
                    Some("existing") => FrameworkResponse::json(
                        200,
                        r#"{"id":"existing","displayName":"Existing"}"#,
                    ),
                    Some("created") => FrameworkResponse::json(
                        201,
                        r#"{"id":"created","createdAt":"2026-10-09T00:00:00Z"}"#,
                    ),
                    Some("queued") => FrameworkResponse::json(202, r#"{"jobId":"queued-1"}"#),
                    Some("empty") => FrameworkResponse::new(204),
                    Some("reset") => FrameworkResponse::new(205),
                    _ => FrameworkResponse::text(400, "Unknown mode"),
                }
            } else if path == "/null-json" {
                FrameworkResponse::json(status, "null")
            } else {
                FrameworkResponse::new(status)
            })
        });
    }
}

fn json_body(name: &str, schema: &str) -> BodyContract {
    BodyContract {
        kind: BodyKind::Json,
        schema: SchemaRef::named(name).with_json_schema(schema),
        content_type: Some("application/json".into()),
    }
}
