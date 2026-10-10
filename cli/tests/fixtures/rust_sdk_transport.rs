use forklaunch_contract::{BodyContract, BodyKind, FieldContract, RouteContract, SchemaRef};
use forklaunch_http::{Body, FrameworkRequest, FrameworkResponse, LocalRouter};

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
                content_type: Some("application/problem+json".into()),
                ..json_body("InvalidMode", r#"{"type":"object","required":["errorCode","message"],"properties":{"errorCode":{"type":"string"},"message":{"type":"string"}}}"#)
            }))
            .with_response(409, Some(json_body("Conflict", r#"{"type":"object","required":["errorCode","retryAfter"],"properties":{"errorCode":{"type":"string"},"retryAfter":{"type":"integer"}}}"#))),
        RouteContract::get("Get Text", "UTF8 text", "/text").with_response(200, Some(BodyContract {
            kind: BodyKind::Text, schema: SchemaRef::named("Text"), content_type: Some("text/plain; charset=utf-8".into()),
        })),
        RouteContract::get("Get Binary", "Binary download", "/binary").with_response(200, Some(BodyContract {
            kind: BodyKind::Binary, schema: SchemaRef::named("Bytes"), content_type: Some("application/octet-stream".into()),
        })),
        RouteContract::get("Get Unknown", "Unknown JSON", "/unknown").with_response(200, Some(json_body("Unknown", "{}"))),
        RouteContract::get("Get Collision", "Wire name collisions", "/collision").with_response(200, Some(json_body("Collision", r#"{"type": "object", "required": ["fooBar", "foo_bar", "foo_bar2", "foo_bar_2", "type", "type_", "gen", "_", "quote\"\\"], "properties": {"fooBar": {"type": "object", "required": ["first"], "properties": {"first": {"type": "string"}}}, "foo_bar": {"type": "object", "required": ["second"], "properties": {"second": {"type": "integer"}}}, "foo_bar2": {"type": "object", "required": ["third"], "properties": {"third": {"type": "boolean"}}}, "foo_bar_2": {"type": "object", "required": ["fourth"], "properties": {"fourth": {"type": "string"}}}, "type": {"type": "string"}, "type_": {"type": "integer"}, "gen": {"type": "string"}, "_": {"type": "string"}, "quote\"\\": {"type": "string"}}}"#))),
        RouteContract::get("Get Shape", "Nullable schemas and dictionaries", "/shape/:mode")
            .with_param(FieldContract { name: "mode".into(), required: true, schema: SchemaRef::named("String").with_json_schema(r#"{"type":"string"}"#) })
            .with_response(200, Some(json_body("Shape", r#"{"type": "object", "required": ["label", "numbers", "metadata", "open", "constrained", "additional_properties", "any", "list"], "properties": {"label": {"type": "string", "nullable": true}, "numbers": {"type": "object", "additionalProperties": {"type": ["integer", "null"]}}, "metadata": {"anyOf": [{"type": "object", "required": ["id"], "properties": {"id": {"type": "string"}}, "additionalProperties": false}, {"type": "null"}]}, "open": {"type": "object", "required": ["id"], "properties": {"id": {"type": "string"}}}, "constrained": {"type": "object", "required": ["count"], "properties": {"count": {"type": "integer"}}, "additionalProperties": {"type": "string"}}, "additional_properties": {"type": "string"}, "any": {"type": ["string", "integer"]}, "list": {"type": "array"}}}"#))),
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
                    Some("conflict") => {
                        FrameworkResponse::json(409, r#"{"errorCode":"conflict","retryAfter":30}"#)
                    }
                    _ => FrameworkResponse::json(
                        400,
                        r#"{"errorCode":"invalid_mode","message":"Unknown mode"}"#,
                    )
                    .with_header("content-type", "application/problem+json"),
                }
            } else if path == "/shape/:mode" {
                FrameworkResponse::json(200, if request.params.get("mode").map(String::as_str) == Some("full") { r#"{"label": "present", "numbers": {"one": 1, "missing": null}, "metadata": {"id": "meta"}, "open": {"id": "open", "extra": {"nested": true}}, "constrained": {"count": 2, "extra": "text"}, "additional_properties": "explicit", "unknownExtra": [1, false], "any": "seven", "list": [1, "two", null]}"# } else { r#"{"label": null, "numbers": {"one": 1, "missing": null}, "metadata": null, "open": {"id": "open", "extra": {"nested": true}}, "constrained": {"count": 2, "extra": "text"}, "additional_properties": "explicit", "unknownExtra": [1, false], "any": 7, "list": [1, "two", null]}"# })
            } else if path == "/collision" {
                FrameworkResponse::json(200, r#"{"fooBar": {"first": "one"}, "foo_bar": {"second": 2}, "foo_bar2": {"third": true}, "foo_bar_2": {"fourth": "four"}, "type": "kind", "type_": 9, "gen": "future", "_": "underscore", "quote\"\\": "escaped"}"#)
            } else if path == "/text" {
                FrameworkResponse::text(200, "Hello\nRust 🦀")
            } else if path == "/binary" {
                FrameworkResponse::new(200)
                    .with_header("content-type", "application/octet-stream")
                    .with_body(Body::Binary(vec![0, 255, 13, 10, 42]))
            } else if path == "/unknown" {
                FrameworkResponse::json(200, r#"[1,"two",{"three":true}]"#)
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
