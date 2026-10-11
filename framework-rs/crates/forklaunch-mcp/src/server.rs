//! Streamable HTTP MCP transport using the official Rust SDK. Tool execution
//! stays inside the supplied HTTP router, preserving its validation, auth,
//! tenant binding, telemetry and handlers without making loopback requests.
use crate::{AllowAllAuth, McpAuthCallback, mcp_tools};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, HeaderName, HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use base64::Engine;
use forklaunch_contract::{BodyKind, ContractRegistry, RouteContract};
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ResourceContents, ServerCapabilities,
        ServerConfig, Tool,
    },
    service::RequestContext,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tower::ServiceExt;

const URI_COMPONENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'!')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// Transport options. Route authentication still applies independently to each
/// tool call. Default Host/Origin checks restrict access to local clients.
pub struct McpServerOptions {
    pub path: String,
    pub name: String,
    pub version: String,
    pub transport: StreamableHttpServerConfig,
    pub authorize: Arc<dyn McpAuthCallback + Send + Sync>,
    pub call_timeout: Duration,
    pub max_response_bytes: usize,
}
impl McpServerOptions {
    #[must_use]
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            path: "/mcp".into(),
            name: name.into(),
            version: version.into(),
            transport: StreamableHttpServerConfig::default()
                .enforce_origin_validation()
                .with_legacy_session_mode(false)
                .with_json_response(true)
                .with_max_request_body_bytes(2 * 1024 * 1024),
            authorize: Arc::new(AllowAllAuth),
            call_timeout: Duration::from_secs(30),
            max_response_bytes: 2 * 1024 * 1024,
        }
    }
}
#[derive(Clone)]
struct ContractServer {
    router: Router,
    tools: Arc<BTreeMap<String, (Tool, RouteContract)>>,
    info: ServerConfig,
    timeout: Duration,
    max_bytes: usize,
}
/// Mounts MCP on the same service router. The dispatched clone excludes MCP,
/// so tools cannot recursively invoke the transport. No session credentials
/// are retained: every request must present its own transport/route credentials.
/// # Errors
/// Rejects invalid paths, bounds, schemas, duplicate tool names and endpoint collisions.
pub fn mount_http(
    router: Router,
    registry: &ContractRegistry,
    options: McpServerOptions,
) -> Result<Router, String> {
    if !options.path.starts_with('/')
        || options.path == "/"
        || options.path.ends_with('/')
        || options.path.contains(['{', '}', ':', '?', '#', '*', '%'])
        || options.path.contains("//")
        || options
            .path
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
        || options.call_timeout.is_zero()
        || options.max_response_bytes == 0
    {
        return Err("Invalid MCP endpoint or resource bounds".into());
    }
    if registry
        .routes()
        .iter()
        .any(|r| r.path == options.path || r.openapi_path == options.path)
    {
        return Err("MCP endpoint conflicts with an HTTP route".into());
    }
    let mut tools = BTreeMap::new();
    for (definition, route) in mcp_tools(registry)
        .into_iter()
        .zip(registry.routes().iter().filter(|r| r.options.mcp))
    {
        let mut tool: Tool =
            serde_json::from_str(&definition.to_json()).map_err(|_| "Invalid MCP tool schema")?;
        // MCP structured outputs must be objects. Array/scalar HTTP bodies are
        // represented as text rather than claiming an invalid output schema.
        if tool
            .output_schema
            .as_ref()
            .is_some_and(|s| s.get("type").and_then(Value::as_str) != Some("object"))
        {
            tool.output_schema = None;
        }
        if route
            .responses
            .iter()
            .filter_map(|r| r.body.as_ref())
            .any(|body| body.kind != BodyKind::Json)
        {
            tool.output_schema = None;
        }
        if tools
            .insert(definition.name, (tool, route.clone()))
            .is_some()
        {
            return Err("Duplicate MCP tool name".into());
        }
    }
    let handler = ContractServer {
        router: router.clone(),
        tools: Arc::new(tools),
        info: ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(options.name, options.version)),
        timeout: options.call_timeout,
        max_bytes: options.max_response_bytes,
    };
    let service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(LocalSessionManager::default()),
        options.transport,
    );
    let authorize = options.authorize;
    let transport = Router::new()
        .route_service(&options.path, service)
        .layer(middleware::from_fn(
            move |request: Request<Body>, next: Next| {
                let authorize = Arc::clone(&authorize);
                async move {
                    let token = request
                        .headers()
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.strip_prefix("Bearer "));
                    if !authorize.authorize(token) {
                        return (StatusCode::UNAUTHORIZED, "MCP authorization required")
                            .into_response();
                    }
                    next.run(request).await
                }
            },
        ));
    Ok(router.merge(transport))
}
impl ServerHandler for ContractServer {
    fn get_info(&self) -> ServerConfig {
        self.info.clone()
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.get(name).map(|(tool, _)| tool.clone())
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(
            self.tools.values().map(|(tool, _)| tool.clone()).collect(),
        ))
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let (_, route) = self
            .tools
            .get(request.name.as_ref())
            .ok_or_else(|| ErrorData::invalid_params("Unknown tool", None))?;
        let args = request.arguments.unwrap_or_default();
        let headers = context
            .extensions
            .get::<axum::http::request::Parts>()
            .map(|parts| &parts.headers);
        let http_request = build_request(route, &args, headers)
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        let response = tokio::time::timeout(self.timeout, async {
            let response: Response = self
                .router
                .clone()
                .oneshot(http_request)
                .await
                .expect("Axum router is infallible");
            let status = response.status();
            let mime = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_owned();
            let body = to_bytes(response.into_body(), self.max_bytes)
                .await
                .map_err(|_| "HTTP response exceeds MCP limit")?;
            Ok::<_, &str>((status, mime, body))
        })
        .await;
        let (status, mime, body) = match response {
            Ok(Ok(value)) => value,
            Ok(Err(message)) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(message)]).into());
            }
            Err(_) => {
                return Ok(
                    CallToolResult::error(vec![ContentBlock::text("HTTP tool timed out")]).into(),
                );
            }
        };
        if status.is_success() && mime.split(';').next() == Some("application/octet-stream") {
            return Ok(CallToolResult::success(vec![ContentBlock::resource(
                ResourceContents::blob(
                    base64::engine::general_purpose::STANDARD.encode(&body),
                    format!("forklaunch://tool/{}", route.operation_name),
                )
                .with_mime_type(mime),
            )])
            .into());
        }
        let text = String::from_utf8_lossy(&body).into_owned();
        let mut result = if status.is_success() {
            CallToolResult::success(vec![ContentBlock::text(&text)])
        } else {
            CallToolResult::error(vec![ContentBlock::text(format!("HTTP {status}: {text}"))])
        };
        if status.is_success()
            && mime.split(';').next() == Some("application/json")
            && let Ok(value @ Value::Object(_)) = serde_json::from_slice(&body)
        {
            result.structured_content = Some(value);
        }
        Ok(result.into())
    }
}
fn scalar(value: &Value) -> Result<String, String> {
    match value {
        Value::String(v) => Ok(v.clone()),
        Value::Bool(_) | Value::Number(_) => Ok(value.to_string()),
        _ => Err("Expected a string, number or boolean".into()),
    }
}
fn build_request(
    route: &RouteContract,
    args: &Map<String, Value>,
    transport_headers: Option<&HeaderMap>,
) -> Result<Request<Body>, String> {
    let mut path = String::new();
    for part in route.path.split('/') {
        if !path.is_empty() || route.path.starts_with('/') {
            path.push('/');
        }
        if part.is_empty() {
            continue;
        }
        let name = part
            .strip_prefix(':')
            .or_else(|| part.strip_prefix('{').and_then(|s| s.strip_suffix('}')));
        if let Some(name) = name {
            let value = args
                .get("params")
                .and_then(|p| p.get(name))
                .ok_or_else(|| format!("Missing path parameter {name}"))?;
            path.push_str(
                &percent_encoding::utf8_percent_encode(&scalar(value)?, URI_COMPONENT).to_string(),
            );
        } else {
            path.push_str(part);
        }
    }
    // split('/') has a leading empty component; normalize only that separator.
    if path.starts_with("//") {
        path.remove(0);
    }
    if let Some(query) = args.get("query") {
        let query = query.as_object().ok_or("query must be an object")?;
        let mut encoded = form_urlencoded::Serializer::new(String::new());
        for (name, value) in query {
            // TypeScript safeStringify preserves strings and serializes JSON
            // composites into one query value, including empty arrays.
            let value = value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned);
            encoded.append_pair(name, &value);
        }
        let query = encoded.finish();
        if !query.is_empty() {
            path.push('?');
            path.push_str(&query);
        }
    }
    let mut headers = HeaderMap::new();
    // The MCP envelope's Authorization is useful for JWT routes. Signed HTTP
    // credentials may instead be supplied explicitly in the tool arguments.
    if let Some(authorization) = transport_headers.and_then(|h| h.get("authorization")) {
        headers.insert("authorization", authorization.clone());
    }
    if let Some(input) = args.get("headers") {
        for (name, value) in input.as_object().ok_or("headers must be an object")? {
            let name = HeaderName::try_from(name.as_str()).map_err(|_| "Invalid header name")?;
            if matches!(
                name.as_str(),
                "host" | "content-length" | "transfer-encoding" | "connection" | "upgrade"
            ) {
                return Err("Unsupported HTTP transport header".into());
            }
            let value =
                HeaderValue::try_from(scalar(value)?).map_err(|_| "Invalid header value")?;
            headers.insert(name, value);
        }
    }
    let body = encode_body(route, args, &mut headers)?;
    let mut request = Request::builder()
        .method(route.method.as_str())
        .uri(path)
        .body(Body::from(body))
        .map_err(|_| "Invalid HTTP tool request")?;
    *request.headers_mut() = headers;
    Ok(request)
}

fn encode_body(
    route: &RouteContract,
    args: &Map<String, Value>,
    headers: &mut HeaderMap,
) -> Result<Vec<u8>, String> {
    let body = if let Some(contract) = &route.request.body {
        let body = args.get("body").ok_or("Missing body")?;
        let mime = args.get("contentType").and_then(Value::as_str).unwrap_or(
            contract
                .content_type
                .as_deref()
                .unwrap_or("application/json"),
        );
        if mime
            != contract
                .content_type
                .as_deref()
                .unwrap_or("application/json")
        {
            return Err("Content type does not match route contract".into());
        }
        headers.insert(
            "content-type",
            HeaderValue::try_from(mime).map_err(|_| "Invalid content type")?,
        );
        match contract.kind {
            BodyKind::Json | BodyKind::Unknown => {
                serde_json::to_vec(body).map_err(|_| "Invalid JSON body")?
            }
            BodyKind::UrlEncoded => {
                let mut encoded = form_urlencoded::Serializer::new(String::new());
                for (name, value) in body.as_object().ok_or("Form body must be an object")? {
                    let values = value
                        .as_array()
                        .map_or_else(|| vec![value], |v| v.iter().collect());
                    for value in values {
                        encoded.append_pair(name, &scalar(value)?);
                    }
                }
                encoded.finish().into_bytes()
            }
            BodyKind::Text => body
                .as_str()
                .ok_or("Text body must be a string")?
                .as_bytes()
                .to_vec(),
            _ => return Err("This body media type requires an explicit MCP adapter".into()),
        }
    } else {
        Vec::new()
    };
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use forklaunch_contract::{FieldContract, RouteContract};
    #[test]
    fn raw_text_json_media_type_preserves_signed_bytes() {
        let raw = "{\n  \"id\": \"evt_💬\"\n}";
        let mut route = RouteContract::post("Webhook", "Webhook", "/webhook");
        route.request.body = Some(forklaunch_contract::BodyContract {
            kind: BodyKind::Text,
            schema: forklaunch_contract::SchemaRef::named("String"),
            content_type: Some("application/json".into()),
        });
        let args = serde_json::json!({"body":raw});
        let mut headers = HeaderMap::new();
        assert_eq!(
            encode_body(&route, args.as_object().unwrap(), &mut headers).unwrap(),
            raw.as_bytes()
        );
        assert_eq!(headers["content-type"], "application/json");
        let invalid = serde_json::json!({"body":{"id":"evt"}});
        assert!(encode_body(&route, invalid.as_object().unwrap(), &mut headers).is_err());
        route.request.body.as_mut().unwrap().kind = BodyKind::Json;
        assert_eq!(
            encode_body(&route, invalid.as_object().unwrap(), &mut headers).unwrap(),
            br#"{"id":"evt"}"#
        );
    }
    #[test]
    fn native_query_transport_preserves_json_values_and_single_escaping() {
        let route = RouteContract::get("Read Query", "Read query", "/query/:id")
            .with_param(FieldContract::required("id", "String"));
        let args = serde_json::json!({"params":{"id":"a/b + 水"},"query":{"ids":["a,b","水"],"empty":[],"filter":{"active":false}}});
        let request = build_request(&route, args.as_object().unwrap(), None).unwrap();
        let values: BTreeMap<_, _> =
            form_urlencoded::parse(request.uri().query().unwrap().as_bytes())
                .into_owned()
                .collect();
        assert_eq!(values["ids"], r#"["a,b","水"]"#);
        assert_eq!(values["empty"], "[]");
        assert_eq!(values["filter"], r#"{"active":false}"#);
        assert_eq!(request.uri().path(), "/query/a%2Fb%20%2B%20%E6%B0%B4");
        if let (Ok(input), Ok(output)) = (
            std::env::var("FORKLAUNCH_TEST_QUERY_TRANSPORT_INPUT"),
            std::env::var("FORKLAUNCH_TEST_QUERY_TRANSPORT_OUTPUT"),
        ) {
            let cases: Vec<Value> = serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
            let targets: Vec<_> = cases
                .iter()
                .map(|args| {
                    build_request(&route, args.as_object().unwrap(), None)
                        .unwrap()
                        .uri()
                        .to_string()
                })
                .collect();
            std::fs::write(
                std::path::Path::new(&output).join("mcp-targets.json"),
                serde_json::to_vec(&targets).unwrap(),
            )
            .unwrap();
        }
    }
}
