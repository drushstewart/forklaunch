use super::*;
use forklaunch_contract::{BodyContract, SchemaRef};
use serde_json::json;

fn encoded(case: &Value) -> Value {
    let kind = match case["kind"].as_str().unwrap() {
        "json" => BodyKind::Json,
        "multipart" => BodyKind::Multipart,
        "file" => BodyKind::File,
        "binary" => BodyKind::Binary,
        "text" => BodyKind::Text,
        "form" => BodyKind::UrlEncoded,
        _ => panic!("Unknown fixture body kind"),
    };
    let route = RouteContract::post("Body", "Body", "/body").with_request_body(BodyContract {
        kind,
        schema: SchemaRef::named("Body"),
        content_type: case["contentType"].as_str().map(str::to_owned),
    });
    let mut headers = HeaderMap::new();
    match encode_body(&route, case["arguments"].as_object().unwrap(), &mut headers) {
        Ok(body) => {
            json!({"body":base64::engine::general_purpose::STANDARD.encode(body),"contentType":headers["content-type"].to_str().unwrap()})
        }
        Err(error) => json!({"error":error}),
    }
}
#[test]
fn native_mcp_body_transport_conformance() {
    assert_eq!(
        encoded(&json!({"kind":"file","arguments":{"body":"水💬"}}))["body"],
        base64::engine::general_purpose::STANDARD.encode("水💬")
    );
    let form = encoded(
        &json!({"kind":"form","arguments":{"body":{"ids":["a,b","水"],"zero":0,"flag":false,"empty":[],"null":null}}}),
    );
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(form["body"].as_str().unwrap())
        .unwrap();
    let fields: BTreeMap<_, _> = form_urlencoded::parse(&bytes).into_owned().collect();
    assert_eq!(fields["ids"], r#"["a,b","水"]"#);
    assert_eq!(fields["empty"], "[]");
    assert_eq!(fields["null"], "null");
    assert_eq!(fields["flag"], "false");
    assert_eq!(fields["zero"], "0");
    assert!(
        encoded(&json!({"kind":"multipart","arguments":{"body":{"flag":false}}}))
            .get("error")
            .is_some()
    );
    assert!(
        encoded(&json!({"kind":"json","arguments":{"body":{},"contentType":"text/plain"}}))
            .get("error")
            .is_some()
    );
    if let (Ok(input), Ok(output)) = (
        std::env::var("FORKLAUNCH_TEST_MCP_BODY_INPUT"),
        std::env::var("FORKLAUNCH_TEST_MCP_BODY_OUTPUT"),
    ) {
        let cases: Vec<Value> = serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
        let output_values: Vec<_> = cases.iter().map(encoded).collect();
        std::fs::write(output, serde_json::to_vec(&output_values).unwrap()).unwrap();
    }
}

async fn call(app: &Router, name: &str, body: Value, token: Option<&str>) -> Value {
    let mut request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-11-25");
    if let Some(token) = token {
        request = request.header("authorization", token);
    }
    let response = app.clone().oneshot(request.body(Body::from(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":{"body":body}}}).to_string())).unwrap()).await.unwrap();
    assert_eq!(response.status(), 200);
    serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap()
}
fn authenticated(headers: &HeaderMap) -> Result<(), StatusCode> {
    if headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        == Some("Bearer body-fixture")
    {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
async fn upload(
    headers: HeaderMap,
    mut body: axum::extract::Multipart,
) -> Result<axum::Json<Value>, StatusCode> {
    authenticated(&headers)?;
    let mut fields = BTreeMap::new();
    while let Some(field) = body
        .next_field()
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
    {
        let name = field.name().ok_or(StatusCode::BAD_REQUEST)?.to_owned();
        fields.insert(
            name,
            field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?,
        );
    }
    Ok(axum::Json(json!(fields)))
}
async fn form(
    headers: HeaderMap,
    axum::Form(fields): axum::Form<BTreeMap<String, String>>,
) -> Result<axum::Json<Value>, StatusCode> {
    authenticated(&headers)?;
    Ok(axum::Json(json!(fields)))
}
async fn file(
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, StatusCode> {
    authenticated(&headers)?;
    Ok(([("content-type", "application/octet-stream")], body))
}
#[tokio::test]
async fn multipart_forms_and_file_tools_execute_through_real_http_extractors_and_guards() {
    let mut registry = ContractRegistry::new();
    for (name, path, kind, mime) in [
        (
            "Upload",
            "/upload",
            BodyKind::Multipart,
            "multipart/form-data",
        ),
        (
            "Form",
            "/form",
            BodyKind::UrlEncoded,
            "application/x-www-form-urlencoded",
        ),
        ("File", "/file", BodyKind::File, "application/octet-stream"),
    ] {
        registry.register(
            RouteContract::post(name, name, path).with_request_body(BodyContract {
                kind,
                schema: SchemaRef::named("Body").with_json_schema("{}"),
                content_type: Some(mime.into()),
            }),
        );
    }
    let http = Router::new()
        .route("/upload", axum::routing::post(upload))
        .route("/form", axum::routing::post(form))
        .route("/file", axum::routing::post(file));
    let app = mount_http(http, &registry, McpServerOptions::new("body-fixture", "1")).unwrap();
    let fields = json!({"unicode":"水💬","empty":"","multiline":"a\nb\rc\r\nd"});
    let denied = call(&app, "upload", fields.clone(), None).await;
    assert_eq!(denied["result"]["isError"], true);
    let result = call(&app, "upload", fields, Some("Bearer body-fixture")).await;
    assert_eq!(
        result["result"]["structuredContent"],
        json!({"unicode":"水💬","empty":"","multiline":"a\r\nb\r\nc\r\nd"})
    );
    let result = call(&app, "upload", json!({}), Some("Bearer body-fixture")).await;
    assert_eq!(result["result"]["structuredContent"], json!({}));
    let invalid = call(
        &app,
        "upload",
        json!({"file":{"base64":"AA=="}}),
        Some("Bearer body-fixture"),
    )
    .await;
    assert_eq!(invalid["error"]["code"], -32602);
    let result=call(&app,"form",json!({"ids":["a,b","水"],"empty":[],"filter":{"ok":false},"flag":false,"zero":0,"nil":null}),Some("Bearer body-fixture")).await;
    assert_eq!(
        result["result"]["structuredContent"],
        json!({"ids":"[\"a,b\",\"水\"]","empty":"[]","filter":"{\"ok\":false}","flag":"false","zero":"0","nil":"null"})
    );
    let result = call(
        &app,
        "file",
        json!("水💬\u{0}bytes"),
        Some("Bearer body-fixture"),
    )
    .await;
    assert_eq!(result["result"]["isError"], false);
    assert_eq!(
        result["result"]["content"][0]["resource"]["blob"],
        base64::engine::general_purpose::STANDARD.encode("水💬\0bytes")
    );
}

#[tokio::test]
async fn literal_marker_keys_survive_http_and_mcp_request_and_response_parsing() {
    #[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema, garde::Validate)]
    struct LiteralBody {
        #[garde(skip)]
        body: Value,
    }

    let mut registry = ContractRegistry::new();
    registry.register(
        RouteContract::post("Echo", "Echo", "/echo").with_request_body(BodyContract {
            schema: SchemaRef::named("Body").with_json_schema("{}"),
            ..BodyContract::json("Body")
        }),
    );
    let http = Router::new().route(
        "/echo",
        axum::routing::post(|axum::Json(body): axum::Json<Value>| async move { axum::Json(body) }),
    );
    let mut options = McpServerOptions::new("literal-fixture", "1");
    options.manual_tools.push(
        ManualTool::typed::<LiteralBody, Value, _, _>(
            "literalManual",
            "Literal values",
            |input, _| async move { Ok(input.body) },
        )
        .unwrap(),
    );
    let app = mount_http(http, &registry, options).unwrap();
    for key in [
        "$serde_json::private::Number",
        "$serde_json::private::RawValue",
    ] {
        for value in [json!("123"), json!({"nested":true}), json!(null)] {
            let literal = Value::Object([(key.to_owned(), value)].into_iter().collect());
            let manual = call(&app, "literalManual", literal.clone(), None).await;
            assert_eq!(manual["result"]["isError"], false);
            assert_eq!(manual["result"]["structuredContent"], literal);
            let result = call(&app, "echo", literal.clone(), None).await;
            assert_eq!(result["result"]["isError"], false);
            assert_eq!(result["result"]["structuredContent"], literal);
            assert_eq!(
                serde_json::from_str::<Value>(
                    result["result"]["content"][0]["text"].as_str().unwrap()
                )
                .unwrap(),
                literal
            );
        }
    }
}
