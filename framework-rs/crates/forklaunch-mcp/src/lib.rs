#![forbid(unsafe_code)]

//! Model Context Protocol (MCP) tool generation from `ForkLaunch` contracts.
//!
//! Eligible HTTP routes (those with `options.mcp == true`) are projected into
//! MCP tool descriptors whose JSON-Schema `inputSchema`/`outputSchema` are
//! derived from the contract's params/query/headers/body and `schemars`
//! metadata. Additional tools can be registered manually, and a transport
//! authentication callback gates access. The `rmcp`-backed server runtime is
//! reserved behind the `rmcp-server` feature.

use forklaunch_common::{JsonField, json_array, json_object, json_string};
use forklaunch_contract::{ContractRegistry, FieldContract, RouteContract, SchemaRef};

/// An MCP tool projected from an HTTP contract (or registered manually).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolDefinition {
    pub name: String,
    pub summary: String,
    pub http_method: String,
    pub path: String,
    /// JSON Schema describing the tool's input arguments.
    pub input_schema_json: String,
    /// JSON Schema describing the tool's success output, when known.
    pub output_schema_json: Option<String>,
}

impl McpToolDefinition {
    /// Builds a manually registered tool with an explicit input schema.
    #[must_use]
    pub fn manual(
        name: impl Into<String>,
        summary: impl Into<String>,
        input_schema_json: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            summary: summary.into(),
            http_method: "POST".to_owned(),
            path: String::new(),
            input_schema_json: input_schema_json.into(),
            output_schema_json: None,
        }
    }

    /// Serializes the tool as an MCP `tools/list` entry.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut fields = vec![
            JsonField::new("name", json_string(&self.name)),
            JsonField::new("description", json_string(&self.summary)),
            JsonField::new("inputSchema", self.input_schema_json.clone()),
        ];
        if let Some(output_schema) = &self.output_schema_json {
            fields.push(JsonField::new("outputSchema", output_schema.clone()));
        }
        json_object(fields)
    }
}

/// Returns the names of the MCP-eligible tools in a registry.
#[must_use]
pub fn tool_names(registry: &ContractRegistry) -> Vec<String> {
    mcp_tools(registry)
        .into_iter()
        .map(|tool| tool.name)
        .collect()
}

/// Projects MCP-eligible HTTP contracts into tool definitions.
///
/// Routes opt out by setting `options.mcp = false`.
#[must_use]
pub fn mcp_tools(registry: &ContractRegistry) -> Vec<McpToolDefinition> {
    registry
        .routes()
        .iter()
        .filter(|route| route.options.mcp)
        .map(tool_from_route)
        .collect()
}

fn tool_from_route(route: &RouteContract) -> McpToolDefinition {
    McpToolDefinition {
        name: route.operation_name.clone(),
        summary: route.summary.clone(),
        http_method: route.method.as_str().to_owned(),
        path: route.path.clone(),
        input_schema_json: input_schema(route),
        output_schema_json: output_schema(route),
    }
}

fn input_schema(route: &RouteContract) -> String {
    let mut properties = Vec::new();
    let mut required = Vec::new();

    if !route.request.params.is_empty() {
        properties.push(JsonField::new(
            "params",
            fields_schema(&route.request.params),
        ));
        // path params are always required
        required.push(json_string("params"));
    }
    if !route.request.query.is_empty() {
        properties.push(JsonField::new("query", fields_schema(&route.request.query)));
        if route.request.query.iter().any(|field| field.required) {
            required.push(json_string("query"));
        }
    }
    if !route.request.headers.is_empty() {
        properties.push(JsonField::new(
            "headers",
            fields_schema(&route.request.headers),
        ));
    }
    if let Some(body) = &route.request.body {
        properties.push(JsonField::new("body", schema_for_name(&body.schema)));
        required.push(json_string("body"));
    }

    let mut fields = vec![
        JsonField::new("type", json_string("object")),
        JsonField::new("properties", json_object(properties)),
    ];
    if !required.is_empty() {
        fields.push(JsonField::new("required", json_array(required)));
    }
    json_object(fields)
}

fn output_schema(route: &RouteContract) -> Option<String> {
    route
        .responses
        .iter()
        .find(|response| (200..300).contains(&response.status_code) && response.body.is_some())
        .and_then(|response| response.body.as_ref())
        .map(|body| schema_for_name(&body.schema))
}

fn fields_schema(fields: &[FieldContract]) -> String {
    let properties = json_object(
        fields
            .iter()
            .map(|field| JsonField::new(&field.name, schema_for_name(&field.schema))),
    );
    let required = fields
        .iter()
        .filter(|field| field.required)
        .map(|field| json_string(&field.name))
        .collect::<Vec<_>>();

    let mut fields = vec![
        JsonField::new("type", json_string("object")),
        JsonField::new("properties", properties),
    ];
    if !required.is_empty() {
        fields.push(JsonField::new("required", json_array(required)));
    }
    json_object(fields)
}

fn schema_for_name(schema: &SchemaRef) -> String {
    // Prefer inline `schemars` metadata when the contract carries it.
    if let Some(json_schema) = &schema.json_schema {
        return json_schema.clone();
    }
    json_object([
        JsonField::new("type", json_string(json_type_for(&schema.name))),
        JsonField::new("title", json_string(&schema.name)),
    ])
}

fn json_type_for(schema_name: &str) -> &'static str {
    match schema_name {
        "String" | "Uuid" | "DateTime" => "string",
        "Integer" => "integer",
        "Number" | "Float" => "number",
        "Boolean" => "boolean",
        "Array" => "array",
        _ => "object",
    }
}

/// A callback the MCP transport invokes to authorize a connection.
pub trait McpAuthCallback {
    /// Returns whether the presented bearer token (if any) is authorized.
    fn authorize(&self, token: Option<&str>) -> bool;
}

/// Authorizes every connection (useful for local/dev transports).
#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllAuth;

impl McpAuthCallback for AllowAllAuth {
    fn authorize(&self, _token: Option<&str>) -> bool {
        true
    }
}

/// A collection of MCP tools combining contract-derived and manual tools.
#[derive(Debug, Clone, Default)]
pub struct McpToolRegistry {
    tools: Vec<McpToolDefinition>,
}

impl McpToolRegistry {
    /// Builds a tool registry from the MCP-eligible routes of a contract registry.
    #[must_use]
    pub fn from_registry(registry: &ContractRegistry) -> Self {
        Self {
            tools: mcp_tools(registry),
        }
    }

    /// Registers an additional, manually defined tool.
    #[must_use]
    pub fn with_tool(mut self, tool: McpToolDefinition) -> Self {
        self.tools.push(tool);
        self
    }

    /// Returns the registered tools.
    #[must_use]
    pub fn tools(&self) -> &[McpToolDefinition] {
        &self.tools
    }

    /// Serializes the registry as an MCP `tools/list` response payload.
    #[must_use]
    pub fn to_tools_list_json(&self) -> String {
        json_object([JsonField::new(
            "tools",
            json_array(self.tools.iter().map(McpToolDefinition::to_json)),
        )])
    }
}

#[cfg(test)]
mod tests {
    use forklaunch_contract::{
        BodyContract, FieldContract, HttpMethod, RouteContract, RouteOptions,
    };

    use super::*;

    #[test]
    fn maps_routes_to_mcp_tool_definitions() {
        let mut registry = ContractRegistry::new();
        registry.register(
            RouteContract::new("Get User", "Get a user", HttpMethod::Get, "/users/:id")
                .with_param(FieldContract::required("id", "String"))
                .with_response(200, Some(BodyContract::json("User"))),
        );

        let tools = mcp_tools(&registry);

        assert_eq!(tools[0].name, "getUser");
        assert_eq!(tools[0].http_method, "GET");
        assert_eq!(tools[0].path, "/users/:id");
        // input schema describes the required path param
        assert!(tools[0].input_schema_json.contains("\"params\""));
        assert!(tools[0].input_schema_json.contains("\"id\""));
        assert!(
            tools[0]
                .input_schema_json
                .contains("\"required\":[\"params\"]")
        );
        // output schema derived from the 200 response body
        assert!(
            tools[0]
                .output_schema_json
                .as_deref()
                .is_some_and(|schema| schema.contains("User"))
        );
    }

    #[test]
    fn respects_per_route_opt_out() {
        let mut registry = ContractRegistry::new();
        registry.register(
            RouteContract::new("Hidden", "Hidden route", HttpMethod::Get, "/internal")
                .with_options(RouteOptions {
                    mcp: false,
                    ..RouteOptions::default()
                }),
        );
        registry.register(RouteContract::new(
            "Visible",
            "Visible route",
            HttpMethod::Get,
            "/public",
        ));

        let names = tool_names(&registry);
        assert_eq!(names, vec!["visible".to_owned()]);
    }

    #[test]
    fn registry_includes_manual_tools_in_tools_list() {
        let mut registry = ContractRegistry::new();
        registry.register(RouteContract::new(
            "List Users",
            "List users",
            HttpMethod::Get,
            "/users",
        ));

        let tool_registry =
            McpToolRegistry::from_registry(&registry).with_tool(McpToolDefinition::manual(
                "search",
                "Full-text search",
                "{\"type\":\"object\",\"properties\":{\"q\":{\"type\":\"string\"}}}",
            ));

        assert_eq!(tool_registry.tools().len(), 2);
        let payload = tool_registry.to_tools_list_json();
        assert!(payload.contains("\"listUsers\""));
        assert!(payload.contains("\"search\""));
        assert!(payload.contains("\"inputSchema\""));
    }

    #[test]
    fn auth_callback_gates_connections() {
        assert!(AllowAllAuth.authorize(Some("token")));
        assert!(AllowAllAuth.authorize(None));
    }
}
