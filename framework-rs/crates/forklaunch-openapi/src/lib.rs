#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use forklaunch_common::{JsonField, json_array, json_object, json_string};
use forklaunch_contract::{
    AuthConfig, BodyContract, BodyKind, ContractRegistry, FieldContract, HttpMethod,
    ResponseContract, RouteContract, SchemaRef,
};

#[must_use]
pub fn operation_listing(registry: &ContractRegistry) -> Vec<String> {
    registry
        .routes()
        .iter()
        .map(|route| format!("{} {}", route.method.as_str(), route.openapi_path))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenApiRoute {
    pub operation_id: String,
    pub method: HttpMethod,
    pub path: String,
    pub summary: String,
    pub request_content_type: Option<String>,
    pub response_status_codes: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenApiServer {
    pub url: String,
    pub description: Option<String>,
}

impl OpenApiServer {
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            description: None,
        }
    }

    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        let mut fields = vec![JsonField::new("url", json_string(&self.url))];
        if let Some(description) = &self.description {
            fields.push(JsonField::new("description", json_string(description)));
        }
        json_object(fields)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenApiOptions {
    pub title: String,
    pub version: String,
    pub description: Option<String>,
    pub servers: Vec<OpenApiServer>,
}

impl OpenApiOptions {
    #[must_use]
    pub fn from_registry(registry: &ContractRegistry) -> Self {
        registry.service().map_or_else(
            || Self {
                title: "ForkLaunch Service".to_owned(),
                version: "0.1.0".to_owned(),
                description: None,
                servers: Vec::new(),
            },
            |service| Self {
                title: service.name.clone(),
                version: service.version.clone(),
                description: None,
                servers: Vec::new(),
            },
        )
    }

    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    #[must_use]
    pub fn with_server(mut self, server: OpenApiServer) -> Self {
        self.servers.push(server);
        self
    }
}

#[must_use]
pub fn openapi_routes(registry: &ContractRegistry) -> Vec<OpenApiRoute> {
    registry
        .routes()
        .iter()
        .filter(|route| route.options.openapi)
        .map(|route| OpenApiRoute {
            operation_id: route.operation_name.clone(),
            method: route.method,
            path: route.openapi_path.clone(),
            summary: route.summary.clone(),
            request_content_type: route
                .request
                .body
                .as_ref()
                .and_then(|body| body.content_type.clone()),
            response_status_codes: route
                .responses
                .iter()
                .map(|response| response.status_code)
                .collect(),
        })
        .collect()
}

#[must_use]
pub fn generate_openapi_json(registry: &ContractRegistry) -> String {
    generate_openapi_json_with_options(registry, &OpenApiOptions::from_registry(registry))
}

#[must_use]
pub fn generate_openapi_json_with_options(
    registry: &ContractRegistry,
    options: &OpenApiOptions,
) -> String {
    let routes = registry
        .routes()
        .iter()
        .filter(|route| route.options.openapi)
        .collect::<Vec<_>>();
    let components = components_json(&routes);

    let mut fields = vec![
        JsonField::new("openapi", json_string("3.1.0")),
        JsonField::new("info", info_json(options)),
        JsonField::new(
            "paths",
            paths_json(&routes, registry.service().map(|service| &service.name)),
        ),
    ];

    if !options.servers.is_empty() {
        fields.push(JsonField::new(
            "servers",
            json_array(options.servers.iter().map(OpenApiServer::to_json)),
        ));
    }

    if !components.is_empty() {
        fields.push(JsonField::new("components", components));
    }

    json_object(fields)
}

/// # Errors
///
/// Returns an error if the parent directory cannot be created or the output file cannot be written.
pub fn write_openapi_json(
    registry: &ContractRegistry,
    options: &OpenApiOptions,
    path: impl AsRef<Path>,
) -> std::io::Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, generate_openapi_json_with_options(registry, options))
}

#[must_use]
pub const fn default_content_type(kind: BodyKind) -> Option<&'static str> {
    match kind {
        BodyKind::Json => Some("application/json"),
        BodyKind::Text => Some("text/plain"),
        BodyKind::File | BodyKind::Binary => Some("application/octet-stream"),
        BodyKind::Multipart => Some("multipart/form-data"),
        BodyKind::UrlEncoded => Some("application/x-www-form-urlencoded"),
        BodyKind::ServerSentEvent => Some("text/event-stream"),
        BodyKind::Unknown => None,
    }
}

fn info_json(options: &OpenApiOptions) -> String {
    let mut fields = vec![
        JsonField::new("title", json_string(&options.title)),
        JsonField::new("version", json_string(&options.version)),
    ];
    if let Some(description) = &options.description {
        fields.push(JsonField::new("description", json_string(description)));
    }
    json_object(fields)
}

fn paths_json(routes: &[&RouteContract], service_name: Option<&String>) -> String {
    let mut routes_by_path = BTreeMap::<String, Vec<&RouteContract>>::new();
    for route in routes {
        routes_by_path
            .entry(route.openapi_path.clone())
            .or_default()
            .push(route);
    }

    json_object(routes_by_path.into_iter().map(|(path, routes)| {
        JsonField::new(
            path,
            json_object(routes.into_iter().map(|route| {
                JsonField::new(
                    route.method.as_str().to_ascii_lowercase(),
                    operation_json(route, service_name),
                )
            })),
        )
    }))
}

fn operation_json(route: &RouteContract, service_name: Option<&String>) -> String {
    let mut fields = vec![
        JsonField::new("operationId", json_string(&route.operation_name)),
        JsonField::new("responses", responses_json(&route.responses)),
        JsonField::new("summary", json_string(&route.summary)),
    ];

    if let Some(service_name) = service_name {
        fields.push(JsonField::new(
            "tags",
            json_array([json_string(service_name)]),
        ));
    }

    let parameters = parameters_json(route);
    if parameters != "[]" {
        fields.push(JsonField::new("parameters", parameters));
    }

    if let Some(body) = &route.request.body {
        fields.push(JsonField::new("requestBody", request_body_json(body)));
    }

    if let Some(auth) = &route.auth {
        fields.push(JsonField::new("security", security_json(auth)));
    }

    if !route.versions.is_empty() {
        fields.push(JsonField::new(
            "x-forklaunch-versions",
            json_array(route.versions.keys().map(|version| json_string(version))),
        ));
    }

    json_object(fields)
}

fn parameters_json(route: &RouteContract) -> String {
    let params = route
        .request
        .params
        .iter()
        .map(|field| parameter_json("path", field, true));
    let query = route
        .request
        .query
        .iter()
        .map(|field| parameter_json("query", field, field.required));
    let headers = route
        .request
        .headers
        .iter()
        .map(|field| parameter_json("header", field, field.required));

    json_array(params.chain(query).chain(headers))
}

fn parameter_json(location: &str, field: &FieldContract, required: bool) -> String {
    json_object([
        JsonField::new("in", json_string(location)),
        JsonField::new("name", json_string(&field.name)),
        JsonField::new("required", required.to_string()),
        JsonField::new("schema", schema_ref_json(&field.schema)),
    ])
}

fn request_body_json(body: &BodyContract) -> String {
    json_object([
        JsonField::new("content", content_json(body)),
        JsonField::new("required", "true"),
    ])
}

fn responses_json(responses: &[ResponseContract]) -> String {
    if responses.is_empty() {
        return json_object([JsonField::new("204", response_json(None, &[]))]);
    }

    json_object(responses.iter().map(|response| {
        JsonField::new(
            response.status_code.to_string(),
            response_json(response.body.as_ref(), &response.headers),
        )
    }))
}

fn response_json(body: Option<&BodyContract>, headers: &[FieldContract]) -> String {
    let mut fields = vec![JsonField::new("description", json_string("Response"))];

    if !headers.is_empty() {
        fields.push(JsonField::new(
            "headers",
            json_object(headers.iter().map(|header| {
                JsonField::new(
                    &header.name,
                    json_object([JsonField::new("schema", schema_ref_json(&header.schema))]),
                )
            })),
        ));
    }

    if let Some(body) = body {
        fields.push(JsonField::new("content", content_json(body)));
    }

    json_object(fields)
}

fn content_json(body: &BodyContract) -> String {
    let content_type = body
        .content_type
        .as_deref()
        .or_else(|| default_content_type(body.kind));

    content_type.map_or_else(
        || json_object([]),
        |content_type| {
            json_object([JsonField::new(
                content_type,
                json_object([JsonField::new("schema", schema_ref_json(&body.schema))]),
            )])
        },
    )
}

fn schema_ref_json(schema: &SchemaRef) -> String {
    json_object([JsonField::new(
        "$ref",
        json_string(&format!("#/components/schemas/{}", schema.name)),
    )])
}

fn components_json(routes: &[&RouteContract]) -> String {
    let schemas = component_schemas_json(routes);
    let security_schemes = security_schemes_json(routes);

    let mut fields = Vec::new();
    if !schemas.is_empty() {
        fields.push(JsonField::new("schemas", schemas));
    }
    if !security_schemes.is_empty() {
        fields.push(JsonField::new("securitySchemes", security_schemes));
    }

    json_object(fields)
}

fn component_schemas_json(routes: &[&RouteContract]) -> String {
    let mut schemas = BTreeMap::<String, String>::new();

    for route in routes {
        collect_request_schemas(&route.request.params, &mut schemas);
        collect_request_schemas(&route.request.query, &mut schemas);
        collect_request_schemas(&route.request.headers, &mut schemas);
        if let Some(body) = &route.request.body {
            collect_body_schema(body, &mut schemas);
        }
        for response in &route.responses {
            collect_request_schemas(&response.headers, &mut schemas);
            if let Some(body) = &response.body {
                collect_body_schema(body, &mut schemas);
            }
        }
    }

    if schemas.is_empty() {
        return String::new();
    }

    json_object(
        schemas
            .into_iter()
            .map(|(name, schema)| JsonField::new(name, schema)),
    )
}

fn collect_request_schemas(fields: &[FieldContract], schemas: &mut BTreeMap<String, String>) {
    for field in fields {
        collect_schema_ref(&field.schema, schemas);
    }
}

fn collect_body_schema(body: &BodyContract, schemas: &mut BTreeMap<String, String>) {
    collect_schema_ref(&body.schema, schemas);
}

fn collect_schema_ref(schema: &SchemaRef, schemas: &mut BTreeMap<String, String>) {
    schemas.entry(schema.name.clone()).or_insert_with(|| {
        schema
            .json_schema
            .clone()
            .unwrap_or_else(default_schema_json)
    });
}

fn default_schema_json() -> String {
    json_object([JsonField::new("type", json_string("object"))])
}

fn security_json(auth: &AuthConfig) -> String {
    match auth {
        AuthConfig::Jwt(_) | AuthConfig::Bearer => {
            json_array([json_object([JsonField::new("bearerAuth", json_array([]))])])
        }
        AuthConfig::Basic(_) => {
            json_array([json_object([JsonField::new("basicAuth", json_array([]))])])
        }
        AuthConfig::Hmac(_) => {
            json_array([json_object([JsonField::new("hmacAuth", json_array([]))])])
        }
    }
}

fn security_schemes_json(routes: &[&RouteContract]) -> String {
    let mut schemes = BTreeSet::<&str>::new();
    let mut hmac_header = None;

    for route in routes {
        match &route.auth {
            Some(AuthConfig::Jwt(_) | AuthConfig::Bearer) => {
                schemes.insert("bearerAuth");
            }
            Some(AuthConfig::Basic(_)) => {
                schemes.insert("basicAuth");
            }
            Some(AuthConfig::Hmac(hmac)) => {
                schemes.insert("hmacAuth");
                hmac_header = hmac.header_name.as_deref();
            }
            None => {}
        }
    }

    if schemes.is_empty() {
        return String::new();
    }

    json_object(schemes.into_iter().map(|scheme| match scheme {
        "bearerAuth" => JsonField::new(
            "bearerAuth",
            json_object([
                JsonField::new("type", json_string("http")),
                JsonField::new("scheme", json_string("bearer")),
            ]),
        ),
        "basicAuth" => JsonField::new(
            "basicAuth",
            json_object([
                JsonField::new("type", json_string("http")),
                JsonField::new("scheme", json_string("basic")),
            ]),
        ),
        _ => JsonField::new(
            "hmacAuth",
            json_object([
                JsonField::new("type", json_string("apiKey")),
                JsonField::new("in", json_string("header")),
                JsonField::new(
                    "name",
                    json_string(hmac_header.unwrap_or("x-forklaunch-signature")),
                ),
            ]),
        ),
    }))
}

#[cfg(test)]
mod tests {
    use forklaunch_contract::{
        AuthConfig, BodyContract, FieldContract, HmacAuth, HttpMethod, RouteContract,
        ServiceContract,
    };

    use super::*;

    #[test]
    fn maps_registered_routes_to_openapi_routes() {
        let mut registry = ContractRegistry::new();
        registry.register(
            RouteContract::new("Create User", "Create a user", HttpMethod::Post, "/users")
                .with_request_body(BodyContract::json("CreateUserBody"))
                .with_response(201, Some(BodyContract::json("User"))),
        );

        let routes = openapi_routes(&registry);

        assert_eq!(routes[0].operation_id, "createUser");
        assert_eq!(routes[0].path, "/users");
        assert_eq!(
            routes[0].request_content_type.as_deref(),
            Some("application/json")
        );
        assert_eq!(routes[0].response_status_codes, vec![201]);
    }

    #[test]
    fn generates_openapi_31_json_from_contract_registry() {
        let mut registry = ContractRegistry::for_service(ServiceContract::new("users", "1.0.0"));
        registry.register(
            RouteContract::post("Create User", "Create a user", "/users")
                .with_request_body(BodyContract::json("CreateUserBody"))
                .with_response(201, Some(BodyContract::json("User")))
                .with_response_header(201, FieldContract::required("etag", "String"))
                .with_auth(AuthConfig::Hmac(
                    HmacAuth::new().with_header_name("x-service-signature"),
                )),
        );

        let json = generate_openapi_json_with_options(
            &registry,
            &OpenApiOptions::from_registry(&registry)
                .with_description("Users service")
                .with_server(
                    OpenApiServer::new("https://api.example.com").with_description("prod"),
                ),
        );

        assert!(json.contains("\"openapi\":\"3.1.0\""));
        assert!(json.contains("\"operationId\":\"createUser\""));
        assert!(json.contains("\"/users\""));
        assert!(json.contains("\"201\""));
        assert!(json.contains("\"hmacAuth\""));
        assert!(json.contains("\"x-service-signature\""));
        assert!(json.contains("\"CreateUserBody\""));
    }

    #[test]
    fn openapi_json_is_stable_for_snapshot_review() {
        let mut registry = ContractRegistry::for_service(ServiceContract::new("users", "1.0.0"));
        registry.register(
            RouteContract::get("Get User", "Get a user", "/users/:id")
                .with_param(FieldContract::required("id", "String"))
                .with_response(200, Some(BodyContract::json("User"))),
        );

        let json = generate_openapi_json(&registry);

        assert_eq!(
            json,
            "{\"components\":{\"schemas\":{\"String\":{\"type\":\"object\"},\"User\":{\"type\":\"object\"}}},\"info\":{\"title\":\"users\",\"version\":\"1.0.0\"},\"openapi\":\"3.1.0\",\"paths\":{\"/users/{id}\":{\"get\":{\"operationId\":\"getUser\",\"parameters\":[{\"in\":\"path\",\"name\":\"id\",\"required\":true,\"schema\":{\"$ref\":\"#/components/schemas/String\"}}],\"responses\":{\"200\":{\"content\":{\"application/json\":{\"schema\":{\"$ref\":\"#/components/schemas/User\"}}},\"description\":\"Response\"}},\"summary\":\"Get a user\",\"tags\":[\"users\"]}}}}"
        );
    }
}
