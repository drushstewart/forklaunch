#![forbid(unsafe_code)]

pub mod media;
mod models;
pub mod render;
pub mod response;
mod schema;
pub mod websocket;

pub use models::{
    RustModelFieldTemplate, RustModelTemplate, RustTypeAliasTemplate, RustUnionVariantTemplate,
    render_rust_models,
};

/// A bounded schema graph compiled into both client languages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaModels {
    pub rust_models: Vec<RustModelTemplate>,
    pub rust_aliases: Vec<RustTypeAliasTemplate>,
    pub rust_type: String,
    pub typescript_type: String,
    pub typescript_definitions: std::collections::BTreeMap<String, String>,
}

/// Compile local JSON Schema references, retaining guarded recursive graph edges.
///
/// # Errors
/// Rejects unsupported references, unguarded aliases and schema expansion limits.
pub fn compile_schema(root_name: &str, value: &serde_json::Value) -> anyhow::Result<SchemaModels> {
    let value = schema::normalize(
        value,
        &format!("{}Schema", models::rust_type_name(root_name)),
    )?;
    let (rust_models, rust_aliases, rust_type) = models::rust_types_from_schema(root_name, &value);
    let typescript_definitions = value
        .get(schema::DEFINITIONS)
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flatten()
        .map(|(name, value)| (name.clone(), models::ts_type_from_schema(value)))
        .collect();
    Ok(SchemaModels {
        rust_models,
        rust_aliases,
        rust_type,
        typescript_type: models::ts_type_from_schema(&value),
        typescript_definitions,
    })
}

/// Compile a contract schema, using known primitive names when JSON Schema is absent.
///
/// # Errors
/// Returns schema parsing or bounded graph compilation failures.
pub fn compile_schema_ref(
    root_name: &str,
    schema: &forklaunch_contract::SchemaRef,
) -> anyhow::Result<SchemaModels> {
    let value = if let Some(json) = &schema.json_schema {
        if json.len() > 32 * 1024 * 1024 {
            anyhow::bail!("SDK schema graph exceeds 32 MiB");
        }
        serde_json::from_str(json)?
    } else {
        match models::ts_type_from_schema_name(&schema.name).as_str() {
            "string" => serde_json::json!({"type": "string"}),
            "number" => serde_json::json!({"type": "number"}),
            "boolean" => serde_json::json!({"type": "boolean"}),
            _ => serde_json::json!({}),
        }
    };
    compile_schema(root_name, &value)
}

/// Escape data embedded in a single-quoted TypeScript string literal.
#[must_use]
pub fn escape_typescript_string(value: &str) -> String {
    models::escape_ts_string(value)
}

pub use forklaunch_contract::{BodyKind, SchemaRef};
use forklaunch_contract::{ContractRegistry, HttpMethod};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdkTarget {
    TypeScript,
    Rust,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkGenerationPlan {
    pub target: SdkTarget,
    pub operations: Vec<SdkOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkOperation {
    pub name: String,
    pub method: HttpMethod,
    pub path: String,
    pub request_params: Vec<SdkField>,
    pub request_query: Vec<SdkField>,
    pub request_headers: Vec<SdkField>,
    pub request_body: Option<String>,
    pub request_body_json_schema: Option<String>,
    pub request_body_kind: Option<BodyKind>,
    pub request_body_content_type: Option<String>,
    pub responses: Vec<SdkResponse>,
    pub response_status_codes: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkField {
    pub name: String,
    pub schema: String,
    pub json_schema: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkResponse {
    pub status_code: u16,
    pub body: Option<String>,
    pub body_json_schema: Option<String>,
    pub body_kind: Option<BodyKind>,
    pub body_content_type: Option<String>,
    pub headers: Vec<SdkField>,
}

impl SdkGenerationPlan {
    #[must_use]
    pub fn from_registry(target: SdkTarget, registry: &ContractRegistry) -> Self {
        Self {
            target,
            operations: registry
                .routes()
                .iter()
                .map(|route| SdkOperation {
                    name: route.operation_name.clone(),
                    method: route.method,
                    path: route.path.clone(),
                    request_params: route.request.params.iter().map(SdkField::from).collect(),
                    request_query: route.request.query.iter().map(SdkField::from).collect(),
                    request_headers: route.request.headers.iter().map(SdkField::from).collect(),
                    request_body: route
                        .request
                        .body
                        .as_ref()
                        .map(|body| body.schema.name.clone()),
                    request_body_json_schema: route
                        .request
                        .body
                        .as_ref()
                        .and_then(|body| body.schema.json_schema.clone()),
                    request_body_kind: route.request.body.as_ref().map(|body| body.kind),
                    request_body_content_type: route
                        .request
                        .body
                        .as_ref()
                        .and_then(|body| body.content_type.clone()),
                    responses: route
                        .responses
                        .iter()
                        .map(|response| SdkResponse {
                            status_code: response.status_code,
                            body: response.body.as_ref().map(|body| body.schema.name.clone()),
                            body_json_schema: response
                                .body
                                .as_ref()
                                .and_then(|body| body.schema.json_schema.clone()),
                            body_kind: response.body.as_ref().map(|body| body.kind),
                            body_content_type: response
                                .body
                                .as_ref()
                                .and_then(|body| body.content_type.clone()),
                            headers: response.headers.iter().map(SdkField::from).collect(),
                        })
                        .collect(),
                    response_status_codes: route
                        .responses
                        .iter()
                        .map(|response| response.status_code)
                        .collect(),
                })
                .collect(),
        }
    }
}

impl From<&forklaunch_contract::FieldContract> for SdkField {
    fn from(field: &forklaunch_contract::FieldContract) -> Self {
        Self {
            name: field.name.clone(),
            schema: field.schema.name.clone(),
            json_schema: field.schema.json_schema.clone(),
            required: field.required,
        }
    }
}

#[cfg(test)]
mod tests {
    use forklaunch_contract::{BodyContract, FieldContract, HttpMethod, RouteContract};

    use super::*;

    #[test]
    fn sdk_plan_retains_schema_graphs_and_response_media() {
        use forklaunch_contract::{BodyKind, SchemaRef};
        let schema = r##"{"type":"object","properties":{"children":{"type":"array","items":{"$ref":"#"}}}}"##;
        let field = FieldContract {
            schema: SchemaRef::named("Tree").with_json_schema(schema),
            ..FieldContract::optional("filter", "Tree")
        };
        let body = BodyContract {
            schema: SchemaRef::named("Tree").with_json_schema(schema),
            ..BodyContract::json("Tree")
        };
        let mut registry = ContractRegistry::new();
        registry.register(
            RouteContract::get("Read Tree", "Graph", "/tree")
                .with_query(field)
                .with_request_body(body.clone())
                .with_response(
                    200,
                    Some(BodyContract {
                        kind: BodyKind::ServerSentEvent,
                        content_type: Some("text/event-stream".into()),
                        ..body
                    }),
                ),
        );
        let plan = SdkGenerationPlan::from_registry(SdkTarget::Rust, &registry);
        let operation = &plan.operations[0];
        assert_eq!(operation.request_body_json_schema.as_deref(), Some(schema));
        assert_eq!(
            operation.request_query[0].json_schema.as_deref(),
            Some(schema)
        );
        let response = &operation.responses[0];
        assert_eq!(response.body_json_schema.as_deref(), Some(schema));
        assert_eq!(response.body_kind, Some(BodyKind::ServerSentEvent));
        assert_eq!(
            response.body_content_type.as_deref(),
            Some("text/event-stream")
        );
        let (variant, compiled) =
            crate::response::compile_response(HttpMethod::Get, response, "TreeEvent").unwrap();
        assert!(variant.streaming_response);
        assert!(!compiled.typescript_definitions.is_empty());
        assert!(compiled.rust_models.iter().any(|model| {
            model
                .fields
                .iter()
                .any(|field| field.field_type.starts_with("Option<Vec<"))
        }));
    }

    #[test]
    fn sdk_generation_plan_preserves_live_typed_operation_shape() {
        let mut registry = ContractRegistry::new();
        registry.register(
            RouteContract::new("Create User", "Create a user", HttpMethod::Post, "/users")
                .with_query(FieldContract::optional("dryRun", "Boolean"))
                .with_header(FieldContract::required("authorization", "String"))
                .with_request_body(BodyContract::json("CreateUserBody"))
                .with_response(201, Some(BodyContract::json("User"))),
        );

        let plan = SdkGenerationPlan::from_registry(SdkTarget::TypeScript, &registry);

        assert_eq!(plan.operations[0].name, "createUser");
        assert_eq!(
            plan.operations[0].request_body.as_deref(),
            Some("CreateUserBody")
        );
        assert_eq!(
            plan.operations[0].request_body_content_type.as_deref(),
            Some("application/json")
        );
        assert_eq!(plan.operations[0].request_query[0].name, "dryRun");
        assert!(!plan.operations[0].request_query[0].required);
        assert_eq!(plan.operations[0].request_headers[0].name, "authorization");
        assert!(plan.operations[0].request_headers[0].required);
        assert_eq!(
            plan.operations[0].responses[0].body.as_deref(),
            Some("User")
        );
        assert_eq!(plan.operations[0].response_status_codes, vec![201]);
    }
}
