use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{create_dir_all, read_to_string, write},
    io::Write,
    path::Path,
};

use anyhow::{Context, Result, bail};
use clap::{Arg, ArgAction, ArgMatches, Command, parser::ValueSource};
use convert_case::{Case, Casing};
use ramhorns::{Content, Template};
use serde_json::{Value as JsonValue, json};
use termcolor::{ColorChoice, StandardStream, WriteColor};
use toml::Value;

use crate::{
    CliCommand,
    core::{command::command, rendered_template::TEMPLATES_DIR},
    init::rust_project::{self, add_rust_workspace_member},
};

#[derive(Debug)]
pub(crate) struct GenerateCommand;

impl GenerateCommand {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

impl CliCommand for GenerateCommand {
    fn command(&self) -> Command {
        command("generate", "Generate SDKs from ForkLaunch artifacts")
            .arg(
                Arg::new("base_path")
                    .long("path")
                    .short('p')
                    .help("Path to application root"),
            )
            .arg(
                Arg::new("target")
                    .long("target")
                    .short('t')
                    .default_value("all")
                    .value_parser(["ts", "typescript", "rust", "all"])
                    .help("SDK target to generate"),
            )
            .arg(
                Arg::new("target_alias")
                    .index(1)
                    .value_parser(["ts", "typescript", "rust", "all"])
                    .help("SDK target shorthand: ts, rust, or all"),
            )
            .arg(
                Arg::new("service")
                    .long("service")
                    .short('s')
                    .help("Generate SDKs for one service"),
            )
            .arg(
                Arg::new("from")
                    .long("from")
                    .default_value("openapi")
                    .value_parser(["openapi", "contract"])
                    .help("Artifact source for SDK generation"),
            )
            .arg(
                Arg::new("check")
                    .long("check")
                    .action(ArgAction::SetTrue)
                    .help(
                        "Check whether generated SDK outputs are up to date without writing files",
                    ),
            )
    }

    fn handler(&self, matches: &ArgMatches) -> Result<()> {
        let mut stdout = StandardStream::stdout(ColorChoice::Always);
        let (app_root, _) = crate::core::validate::require_manifest(matches)?;

        if !rust_project::is_rust_app_root(&app_root)? {
            bail!("SDK generation is currently implemented for Rust and mixed Rust apps");
        }

        let target = resolve_sdk_generate_target(matches)?;
        let source = matches.get_one::<String>("from").unwrap().as_str();
        let selected_service = matches.get_one::<String>("service").map(String::as_str);
        let check = matches.get_flag("check");

        generate_or_check_sdk_outputs(
            &app_root,
            &target,
            source,
            selected_service,
            check,
            &mut stdout,
        )
    }
}

fn resolve_sdk_generate_target(matches: &ArgMatches) -> Result<String> {
    let flag_target = matches
        .get_one::<String>("target")
        .map(String::as_str)
        .unwrap_or("all");
    let resolved_flag_target = normalize_sdk_generate_target(flag_target)?;
    let explicit_flag_target = matches.value_source("target") == Some(ValueSource::CommandLine);

    let Some(target_alias) = matches.get_one::<String>("target_alias").map(String::as_str) else {
        return Ok(resolved_flag_target);
    };

    let resolved_alias_target = normalize_sdk_generate_target(target_alias)?;
    if explicit_flag_target && resolved_alias_target != resolved_flag_target {
        bail!(
            "SDK target alias `{}` conflicts with --target `{}`",
            target_alias,
            flag_target
        );
    }

    Ok(resolved_alias_target)
}

fn normalize_sdk_generate_target(target: &str) -> Result<String> {
    match target {
        "ts" | "typescript" => Ok("typescript".to_string()),
        "rust" => Ok("rust".to_string()),
        "all" => Ok("all".to_string()),
        _ => bail!("Unsupported SDK target `{target}`"),
    }
}

pub(crate) fn generate_or_check_sdk_outputs(
    app_root: &Path,
    target: &str,
    source: &str,
    selected_service: Option<&str>,
    check: bool,
    stdout: &mut StandardStream,
) -> Result<()> {
    let manifest = read_manifest(app_root)?;
    let services = if source == "contract" {
        rust_sdk_services_from_contracts(app_root, &manifest, selected_service)?
    } else {
        rust_sdk_services(&manifest, selected_service)
    };

    if services.is_empty() {
        bail!("No Rust services or workers found for SDK generation");
    }

    if check {
        check_sdk_outputs(app_root, target, &services)?;
        log_ok!(stdout, "SDK outputs are up to date");
        return Ok(());
    }

    if matches!(target, "typescript" | "all") {
        generate_typescript_sdk(app_root, &services)?;
        log_ok!(stdout, "Generated TypeScript SDK");
    }

    if matches!(target, "rust" | "all") {
        generate_rust_sdks(app_root, &services)?;
        log_ok!(stdout, "Generated Rust SDK crate(s)");
    }

    Ok(())
}

pub(crate) fn regenerate_typescript_sdk_if_present(
    app_root: &Path,
    stdout: &mut StandardStream,
) -> Result<()> {
    let sdk_root = app_root.join("sdks").join("typescript");
    if !sdk_root.join("package.json").exists() && !sdk_root.join("src").join("index.ts").exists() {
        return Ok(());
    }

    let manifest = read_manifest(app_root)?;
    let services = rust_sdk_services_from_contracts(app_root, &manifest, None)
        .unwrap_or_else(|_| rust_sdk_services(&manifest, None));
    generate_typescript_sdk(app_root, &services)?;
    log_ok!(stdout, "Regenerated TypeScript SDK");
    Ok(())
}

#[derive(Debug, Clone)]
struct SdkService {
    name: String,
    routers: Vec<SdkRouter>,
}

#[derive(Debug, Clone)]
struct SdkRouter {
    name: String,
    operations: Vec<SdkOperation>,
}

#[derive(Debug, Clone)]
struct SdkOperation {
    name: String,
    method: String,
    path: String,
    request_type: String,
    response_type: String,
    responses_type: String,
    request_params_type: String,
    request_query_type: String,
    request_body_type: String,
    request_headers_type: String,
    response_body_type: String,
    success_status: u16,
    rust_response_type: String,
    rust_models: Vec<RustModelTemplate>,
    rust_aliases: Vec<RustTypeAliasTemplate>,
}

#[derive(Content)]
struct TypeScriptSdkTemplate {
    operation_types: Vec<TypeScriptOperationTypeTemplate>,
    services: Vec<TypeScriptServiceTemplate>,
    aliases: Vec<TypeScriptAliasTemplate>,
}

#[derive(Content)]
struct TypeScriptOperationTypeTemplate {
    request_type: String,
    response_type: String,
    responses_type: String,
    request_params_type: String,
    request_query_type: String,
    request_body_type: String,
    request_headers_type: String,
    response_body_type: String,
    success_status: String,
}

#[derive(Content)]
struct TypeScriptServiceTemplate {
    service_key: String,
    routers: Vec<TypeScriptRouterTemplate>,
    last: bool,
}

#[derive(Content)]
struct TypeScriptRouterTemplate {
    router_key: String,
    operations: Vec<TypeScriptOperationTemplate>,
    last: bool,
}

#[derive(Content)]
struct TypeScriptOperationTemplate {
    operation_key: String,
    request_type: String,
    responses_type: String,
    optional_input: bool,
    service_name: String,
    router_name: String,
    operation_name: String,
    method: String,
    path: String,
    last: bool,
}

#[derive(Content)]
struct TypeScriptAliasTemplate {
    alias: String,
    service_key: String,
}

#[derive(Content)]
struct RustSdkTemplate {
    client_name: String,
    models: Vec<RustModelTemplate>,
    aliases: Vec<RustTypeAliasTemplate>,
    operations: Vec<RustOperationTemplate>,
}

#[derive(Content, Debug, Clone, PartialEq, Eq)]
struct RustModelTemplate {
    name: String,
    fields: Vec<RustModelFieldTemplate>,
}

#[derive(Content, Debug, Clone, PartialEq, Eq)]
struct RustModelFieldTemplate {
    field_name: String,
    field_type: String,
}

#[derive(Content, Debug, Clone, PartialEq, Eq)]
struct RustTypeAliasTemplate {
    name: String,
    target_type: String,
}

#[derive(Content)]
struct RustOperationTemplate {
    request_type: String,
    response_type: String,
    responses_type: String,
    success_variant: String,
    success_status: String,
    execute_method_name: String,
    method_name: String,
    http_method: String,
    path: String,
}

fn read_manifest(app_root: &Path) -> Result<Value> {
    let manifest_path = app_root.join(".forklaunch").join("manifest.toml");
    let content = read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    toml::from_str::<Value>(&content)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))
}

fn rust_sdk_services(manifest: &Value, selected_service: Option<&str>) -> Vec<SdkService> {
    manifest
        .get("projects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|project| {
            let project_type = project.get("type").and_then(Value::as_str)?;
            if !matches!(project_type, "Service" | "Worker") {
                return None;
            }
            let name = project.get("name").and_then(Value::as_str)?;
            if let Some(selected_service) = selected_service
                && selected_service != name
            {
                return None;
            }
            let metadata = project.get("metadata").and_then(Value::as_table)?;
            if metadata.get("language").and_then(Value::as_str) != Some("rust") {
                return None;
            }
            let router_names = project
                .get("routers")
                .and_then(Value::as_array)
                .map(|routers| {
                    routers
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let router_names = if router_names.is_empty() {
                vec!["health".to_string()]
            } else {
                router_names
            };
            let routers = router_names
                .iter()
                .map(|router| sdk_router(name, router))
                .collect();
            Some(SdkService {
                name: name.to_string(),
                routers,
            })
        })
        .collect()
}

fn sdk_router(service_name: &str, router_name: &str) -> SdkRouter {
    let operation_name = if router_name == "health" {
        "get".to_string()
    } else {
        "list".to_string()
    };
    let path = format!("/{}", router_name.replace('_', "-"));
    let response_schema = default_response_json_schema(router_name);

    SdkRouter {
        name: router_name.to_string(),
        operations: vec![build_sdk_operation(
            service_name,
            router_name,
            &operation_name,
            "GET",
            &path,
            RequestTypeShapes::default(),
            200,
            &response_schema,
        )],
    }
}

fn rust_sdk_services_from_contracts(
    app_root: &Path,
    manifest: &Value,
    selected_service: Option<&str>,
) -> Result<Vec<SdkService>> {
    let mut services = Vec::new();
    for project in manifest
        .get("projects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let project_type = project.get("type").and_then(Value::as_str);
        if !matches!(project_type, Some("Service") | Some("Worker")) {
            continue;
        }
        let Some(service_name) = project.get("name").and_then(Value::as_str) else {
            continue;
        };
        if let Some(selected_service) = selected_service
            && selected_service != service_name
        {
            continue;
        }
        let Some(metadata) = project.get("metadata").and_then(Value::as_table) else {
            continue;
        };
        if metadata.get("language").and_then(Value::as_str) != Some("rust") {
            continue;
        }

        let contract_path = metadata
            .get("contractArtifactPath")
            .and_then(Value::as_str)
            .map_or_else(
                || {
                    app_root
                        .join(".forklaunch")
                        .join("contracts")
                        .join(service_name)
                        .join("contract.json")
                },
                |path| app_root.join(path),
            );
        let contract_content = read_to_string(&contract_path)
            .with_context(|| format!("Failed to read {}", contract_path.display()))?;
        let contract = serde_json::from_str::<JsonValue>(&contract_content)
            .with_context(|| format!("Failed to parse {}", contract_path.display()))?;

        services.push(sdk_service_from_contract(service_name, &contract));
    }

    Ok(services)
}

fn sdk_service_from_contract(service_name: &str, contract: &JsonValue) -> SdkService {
    let mut routers = BTreeMap::<String, Vec<SdkOperation>>::new();
    for route in contract
        .get("routes")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
    {
        let method = route
            .get("method")
            .and_then(JsonValue::as_str)
            .unwrap_or("GET")
            .to_ascii_uppercase();
        let path = route
            .get("path")
            .or_else(|| route.get("openapiPath"))
            .and_then(JsonValue::as_str)
            .unwrap_or("/health")
            .to_string();
        let router_name = router_name_from_path(&path);
        let operation_name = route
            .get("operationName")
            .and_then(JsonValue::as_str)
            .filter(|name| !name.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| sdk_operation_name(&method, &path));
        let request_shapes = request_type_shapes_from_contract(route);
        let (success_status, response_schema) = success_response_schema(route)
            .unwrap_or_else(|| (200, default_response_json_schema(&router_name)));

        routers
            .entry(router_name.clone())
            .or_default()
            .push(build_sdk_operation(
                service_name,
                &router_name,
                &operation_name,
                &method,
                &path,
                request_shapes,
                success_status,
                &response_schema,
            ));
    }

    if routers.is_empty() {
        return SdkService {
            name: service_name.to_string(),
            routers: vec![sdk_router(service_name, "health")],
        };
    }

    SdkService {
        name: service_name.to_string(),
        routers: routers
            .into_iter()
            .map(|(name, operations)| SdkRouter { name, operations })
            .collect(),
    }
}

#[derive(Debug, Clone)]
struct RequestTypeShapes {
    params: String,
    query: String,
    body: String,
    headers: String,
}

impl Default for RequestTypeShapes {
    fn default() -> Self {
        Self {
            params: "never".to_string(),
            query: "never".to_string(),
            body: "never".to_string(),
            headers: "ForkLaunchHeaders".to_string(),
        }
    }
}

fn build_sdk_operation(
    service_name: &str,
    router_name: &str,
    operation_name: &str,
    method: &str,
    path: &str,
    request_shapes: RequestTypeShapes,
    success_status: u16,
    response_schema: &JsonValue,
) -> SdkOperation {
    let type_prefix = format!(
        "{}{}{}",
        service_name.to_case(Case::Pascal),
        router_name.to_case(Case::Pascal),
        operation_name.to_case(Case::Pascal)
    );
    let response_type = format!("{type_prefix}Response");
    let response_type_pascal = rust_type_name(&response_type);
    let (rust_models, rust_aliases, rust_response_type) =
        rust_types_from_schema(&response_type_pascal, response_schema);

    SdkOperation {
        name: operation_name.to_string(),
        method: method.to_ascii_uppercase(),
        path: path.to_string(),
        request_type: format!("{type_prefix}Request"),
        response_type,
        responses_type: format!("{type_prefix}Responses"),
        request_params_type: request_shapes.params,
        request_query_type: request_shapes.query,
        request_body_type: request_shapes.body,
        request_headers_type: request_shapes.headers,
        response_body_type: ts_type_from_schema(response_schema),
        success_status,
        rust_response_type,
        rust_models,
        rust_aliases,
    }
}

fn default_response_json_schema(router_name: &str) -> JsonValue {
    if router_name == "health" {
        json!({
            "type": "object",
            "required": ["service", "status", "recordCount", "routes"],
            "properties": {
                "service": { "type": "string" },
                "status": { "type": "string", "const": "ok" },
                "recordCount": { "type": "integer", "minimum": 0 },
                "routes": { "type": "array", "items": { "type": "string" } }
            }
        })
    } else {
        json!({
            "type": "object",
            "required": ["route", "summary", "records"],
            "properties": {
                "route": { "type": "string", "const": router_name },
                "summary": {
                    "type": "object",
                    "required": ["service", "total"],
                    "properties": {
                        "service": { "type": "string" },
                        "total": { "type": "integer", "minimum": 0 }
                    }
                },
                "records": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["id", "name"],
                        "properties": {
                            "id": { "type": "string" },
                            "name": { "type": "string" }
                        }
                    }
                }
            }
        })
    }
}

fn request_type_shapes_from_contract(route: &JsonValue) -> RequestTypeShapes {
    let Some(request) = route.get("request") else {
        return RequestTypeShapes::default();
    };

    let params = fields_ts_type(request.get("params"), false);
    let query = fields_ts_type(request.get("query"), false);
    let headers = fields_ts_type(request.get("headers"), true);
    let body_schema = request
        .get("body")
        .and_then(|body| body.get("schema"))
        .and_then(|schema| schema.get("jsonSchema"))
        .cloned();
    let body = body_schema
        .as_ref()
        .map(ts_type_from_schema)
        .unwrap_or_else(|| "never".to_string());

    RequestTypeShapes {
        params,
        query,
        body,
        headers,
    }
}

fn fields_ts_type(fields: Option<&JsonValue>, allow_arbitrary: bool) -> String {
    let fields = fields
        .and_then(JsonValue::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();

    if fields.is_empty() {
        return if allow_arbitrary {
            "ForkLaunchHeaders".to_string()
        } else {
            "never".to_string()
        };
    }

    let mut parts = Vec::new();
    for field in fields {
        let Some(name) = field.get("name").and_then(JsonValue::as_str) else {
            continue;
        };
        let optional = !field
            .get("required")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        let ty = field
            .get("schema")
            .map(ts_type_from_field_schema)
            .unwrap_or_else(|| "unknown".to_string());
        parts.push(format!(
            "{}{}: {}",
            ts_property_key(name),
            if optional { "?" } else { "" },
            ty
        ));
    }

    if parts.is_empty() {
        if allow_arbitrary {
            "ForkLaunchHeaders".to_string()
        } else {
            "never".to_string()
        }
    } else if allow_arbitrary {
        format!("{{ {} }} & ForkLaunchHeaders", parts.join("; "))
    } else {
        format!("{{ {} }}", parts.join("; "))
    }
}

fn success_response_schema(route: &JsonValue) -> Option<(u16, JsonValue)> {
    route
        .get("responses")
        .and_then(JsonValue::as_array)?
        .iter()
        .filter_map(|response| {
            let status = response.get("statusCode").and_then(JsonValue::as_u64)?;
            if !(200..300).contains(&status) {
                return None;
            }
            let schema = response
                .get("body")
                .and_then(|body| body.get("schema"))
                .and_then(|schema| schema.get("jsonSchema"))
                .cloned()
                .unwrap_or_else(|| json!({ "type": "null" }));
            Some((status as u16, schema))
        })
        .min_by_key(|(status, _)| *status)
}

fn router_name_from_path(path: &str) -> String {
    path.trim_matches('/')
        .split('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .unwrap_or("health")
        .replace('-', "_")
}

fn sdk_operation_name(method: &str, path: &str) -> String {
    if path.trim_matches('/') == "health" {
        return "get".to_string();
    }

    match method {
        "GET" => "list",
        "POST" => "create",
        "PUT" | "PATCH" => "update",
        "DELETE" => "delete",
        _ => "call",
    }
    .to_string()
}

fn ts_type_from_field_schema(schema: &JsonValue) -> String {
    schema
        .get("jsonSchema")
        .map(ts_type_from_schema)
        .or_else(|| {
            schema
                .get("name")
                .and_then(JsonValue::as_str)
                .map(ts_type_from_schema_name)
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn ts_type_from_schema_name(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "string" | "str" | "uuid" | "datetime" | "date" => "string".to_string(),
        "bool" | "boolean" => "boolean".to_string(),
        "usize" | "u64" | "u32" | "i64" | "i32" | "integer" | "int" | "number" | "float"
        | "f64" | "f32" => "number".to_string(),
        _ => "unknown".to_string(),
    }
}

fn ts_type_from_schema(schema: &JsonValue) -> String {
    if let Some(const_value) = schema.get("const") {
        return ts_literal_type(const_value);
    }

    if let Some(enum_values) = schema.get("enum").and_then(JsonValue::as_array)
        && !enum_values.is_empty()
    {
        return enum_values
            .iter()
            .map(ts_literal_type)
            .collect::<Vec<_>>()
            .join(" | ");
    }

    if let Some(one_of) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(JsonValue::as_array)
        && !one_of.is_empty()
    {
        return one_of
            .iter()
            .map(ts_type_from_schema)
            .collect::<Vec<_>>()
            .join(" | ");
    }

    match schema_type(schema) {
        Some("object") => ts_object_type(schema),
        Some("array") => {
            let item_type = schema
                .get("items")
                .map(ts_type_from_schema)
                .unwrap_or_else(|| "unknown".to_string());
            format!("Array<{item_type}>")
        }
        Some("integer" | "number") => "number".to_string(),
        Some("string") => "string".to_string(),
        Some("boolean") => "boolean".to_string(),
        Some("null") => "null".to_string(),
        _ if schema.get("properties").is_some() => ts_object_type(schema),
        _ => "unknown".to_string(),
    }
}

fn ts_object_type(schema: &JsonValue) -> String {
    let Some(properties) = schema.get("properties").and_then(JsonValue::as_object) else {
        return "Record<string, unknown>".to_string();
    };
    if properties.is_empty() {
        return "Record<string, unknown>".to_string();
    }

    let required = schema
        .get("required")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .collect::<BTreeSet<_>>();

    let mut parts = Vec::new();
    for (name, property_schema) in properties {
        let optional = if required.contains(name.as_str()) {
            ""
        } else {
            "?"
        };
        parts.push(format!(
            "{}{}: {}",
            ts_property_key(name),
            optional,
            ts_type_from_schema(property_schema)
        ));
    }

    format!("{{ {} }}", parts.join("; "))
}

fn schema_type(schema: &JsonValue) -> Option<&str> {
    schema.get("type").and_then(|schema_type| {
        schema_type.as_str().or_else(|| {
            schema_type.as_array().and_then(|types| {
                types
                    .iter()
                    .filter_map(JsonValue::as_str)
                    .find(|ty| *ty != "null")
            })
        })
    })
}

fn ts_literal_type(value: &JsonValue) -> String {
    if let Some(value) = value.as_str() {
        return format!("'{}'", escape_ts_string(value));
    }
    if value.is_number() || value.is_boolean() {
        return value.to_string();
    }
    if value.is_null() {
        return "null".to_string();
    }
    "unknown".to_string()
}

fn ts_property_key(name: &str) -> String {
    if is_ts_identifier(name) {
        name.to_string()
    } else {
        format!("'{}'", escape_ts_string(name))
    }
}

fn is_ts_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first == '$' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric())
}

fn rust_types_from_schema(
    root_name: &str,
    schema: &JsonValue,
) -> (Vec<RustModelTemplate>, Vec<RustTypeAliasTemplate>, String) {
    let mut models = BTreeMap::new();
    let response_type = build_rust_type(root_name, schema, &mut models);
    let mut aliases = Vec::new();

    if models.get(&response_type).is_none() {
        aliases.push(RustTypeAliasTemplate {
            name: rust_type_name(root_name),
            target_type: response_type.clone(),
        });
    }

    (
        models.into_values().collect(),
        aliases,
        rust_type_name(root_name),
    )
}

fn build_rust_type(
    type_name: &str,
    schema: &JsonValue,
    models: &mut BTreeMap<String, RustModelTemplate>,
) -> String {
    if schema.get("const").is_some() {
        return match schema_type(schema) {
            Some("integer" | "number") => "i64".to_string(),
            Some("boolean") => "bool".to_string(),
            Some("null") => "()".to_string(),
            _ => "String".to_string(),
        };
    }

    match schema_type(schema) {
        Some("object") => rust_model_from_schema(type_name, schema, models),
        Some("array") => {
            let item_type_name = format!("{}Item", rust_type_name(type_name));
            let item_type = schema
                .get("items")
                .map(|items| build_rust_type(&item_type_name, items, models))
                .unwrap_or_else(|| "String".to_string());
            format!("Vec<{item_type}>")
        }
        Some("integer") => "i64".to_string(),
        Some("number") => "f64".to_string(),
        Some("string") => "String".to_string(),
        Some("boolean") => "bool".to_string(),
        Some("null") => "()".to_string(),
        _ if schema.get("properties").is_some() => {
            rust_model_from_schema(type_name, schema, models)
        }
        _ => "String".to_string(),
    }
}

fn rust_model_from_schema(
    type_name: &str,
    schema: &JsonValue,
    models: &mut BTreeMap<String, RustModelTemplate>,
) -> String {
    let model_name = rust_type_name(type_name);
    if models.contains_key(&model_name) {
        return model_name;
    }

    let required = schema
        .get("required")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .collect::<BTreeSet<_>>();
    let mut fields = Vec::new();

    if let Some(properties) = schema.get("properties").and_then(JsonValue::as_object) {
        for (name, property_schema) in properties {
            let nested_type_name = format!("{}{}", model_name, name.to_case(Case::Pascal));
            let mut field_type = build_rust_type(&nested_type_name, property_schema, models);
            if !required.contains(name.as_str()) {
                field_type = format!("Option<{field_type}>");
            }
            fields.push(RustModelFieldTemplate {
                field_name: rust_field_ident(name),
                field_type,
            });
        }
    }

    models.insert(
        model_name.clone(),
        RustModelTemplate {
            name: model_name.clone(),
            fields,
        },
    );
    model_name
}

fn rust_type_name(input: &str) -> String {
    let mut name = input.to_case(Case::Pascal);
    if name.is_empty() {
        name = "GeneratedModel".to_string();
    }
    if name.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        name.insert_str(0, "Model");
    }
    name
}

fn rust_field_ident(input: &str) -> String {
    let mut name = input.to_case(Case::Snake);
    if name.is_empty() {
        name = "field".to_string();
    }
    if name.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        name.insert(0, '_');
    }
    if is_rust_keyword(&name) {
        name.push('_');
    }
    name
}

fn is_rust_keyword(input: &str) -> bool {
    matches!(
        input,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
    )
}

fn generate_typescript_sdk(app_root: &Path, services: &[SdkService]) -> Result<()> {
    let sdk_root = app_root.join("sdks").join("typescript");
    create_dir_all(sdk_root.join("src"))?;
    write_or_update_root_package_json(app_root)?;
    ensure_pnpm_workspace_includes_typescript_sdk(app_root)?;
    write(
        sdk_root.join("package.json"),
        serde_json::to_string_pretty(&json!({
            "name": "@forklaunch-generated/sdk",
            "version": "0.0.0",
            "private": true,
            "type": "module",
            "exports": {
                ".": "./src/index.ts"
            },
            "scripts": {
                "build": "tsc --noEmit"
            },
            "devDependencies": {
                "typescript": "^5.0.0"
            }
        }))?,
    )?;
    write(
        sdk_root.join("src").join("index.ts"),
        typescript_sdk(services)?,
    )?;
    Ok(())
}

fn write_or_update_root_package_json(app_root: &Path) -> Result<()> {
    let package_path = app_root.join("package.json");
    let mut package = if package_path.exists() {
        serde_json::from_str::<JsonValue>(
            &read_to_string(&package_path)
                .with_context(|| format!("Failed to read {}", package_path.display()))?,
        )
        .with_context(|| format!("Failed to parse {}", package_path.display()))?
    } else {
        json!({})
    };

    let package_object = package
        .as_object_mut()
        .context("Root package.json must be a JSON object")?;
    package_object
        .entry("private".to_string())
        .or_insert(JsonValue::Bool(true));
    package_object
        .entry("type".to_string())
        .or_insert(JsonValue::String("module".to_string()));
    package_object
        .entry("packageManager".to_string())
        .or_insert(JsonValue::String("pnpm@10.0.0".to_string()));

    let scripts = package_object
        .entry("scripts".to_string())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("Root package.json scripts must be a JSON object")?;
    scripts.insert(
        "sdk:generate".to_string(),
        JsonValue::String(
            "forklaunch sdk generate --target typescript --from contract".to_string(),
        ),
    );
    scripts.insert(
        "sdk:check".to_string(),
        JsonValue::String(
            "forklaunch sdk generate --target typescript --from contract --check".to_string(),
        ),
    );
    scripts.insert(
        "sdk:typecheck".to_string(),
        JsonValue::String("pnpm --filter @forklaunch-generated/sdk build".to_string()),
    );
    scripts.insert(
        "sdk:watch".to_string(),
        JsonValue::String("forklaunch sdk watch --target typescript --from contract".to_string()),
    );

    write(&package_path, serde_json::to_string_pretty(&package)?)
        .with_context(|| format!("Failed to update {}", package_path.display()))
}

fn ensure_pnpm_workspace_includes_typescript_sdk(app_root: &Path) -> Result<()> {
    let workspace_path = app_root.join("pnpm-workspace.yaml");
    let sdk_line = "  - \"sdks/typescript\"";
    if !workspace_path.exists() {
        return write(&workspace_path, format!("packages:\n{sdk_line}\n"))
            .with_context(|| format!("Failed to write {}", workspace_path.display()));
    }

    let mut contents = read_to_string(&workspace_path)
        .with_context(|| format!("Failed to read {}", workspace_path.display()))?;
    if contents
        .lines()
        .any(|line| line.trim() == "- \"sdks/typescript\"" || line.trim() == "- sdks/typescript")
    {
        return Ok(());
    }
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    if !contents.lines().any(|line| line.trim() == "packages:") {
        contents.push_str("packages:\n");
    }
    contents.push_str(sdk_line);
    contents.push('\n');
    write(&workspace_path, contents)
        .with_context(|| format!("Failed to update {}", workspace_path.display()))
}

fn check_sdk_outputs(app_root: &Path, target: &str, services: &[SdkService]) -> Result<()> {
    let mut stale_outputs = Vec::new();

    if matches!(target, "typescript" | "all") {
        compare_generated_file(
            &app_root
                .join("sdks")
                .join("typescript")
                .join("src")
                .join("index.ts"),
            &typescript_sdk(services)?,
            &mut stale_outputs,
        )?;
    }

    if matches!(target, "rust" | "all") {
        for service in services {
            let package_name = format!("{}-sdk", service.name);
            let crate_dir = app_root.join("sdks").join("rust").join(&package_name);
            compare_generated_file(
                &crate_dir.join("Cargo.toml"),
                &rust_sdk_cargo_toml(&package_name, &service.name),
                &mut stale_outputs,
            )?;
            compare_generated_file(
                &crate_dir.join("src").join("lib.rs"),
                &rust_sdk(service)?,
                &mut stale_outputs,
            )?;
        }
    }

    if stale_outputs.is_empty() {
        return Ok(());
    }

    bail!(
        "SDK outputs are stale or missing:\n  - {}\nRun `forklaunch sdk generate --target {target}` to regenerate them.",
        stale_outputs.join("\n  - ")
    )
}

fn compare_generated_file(
    path: &Path,
    expected: &str,
    stale_outputs: &mut Vec<String>,
) -> Result<()> {
    match read_to_string(path) {
        Ok(actual) if actual == expected => Ok(()),
        Ok(_) => {
            stale_outputs.push(path.display().to_string());
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            stale_outputs.push(path.display().to_string());
            Ok(())
        }
        Err(error) => Err(error).with_context(|| format!("Failed to read {}", path.display())),
    }
}

fn typescript_sdk(services: &[SdkService]) -> Result<String> {
    let mut operation_types = Vec::new();
    let mut service_blocks = Vec::new();
    let mut aliases = Vec::new();

    for (service_index, service) in services.iter().enumerate() {
        let service_key = service.name.to_case(Case::Camel);
        let alias = format!("{}Service", service_key);
        let mut routers = Vec::new();
        for (router_index, router) in service.routers.iter().enumerate() {
            let router_key = router.name.to_case(Case::Camel);
            let mut operations = Vec::new();
            for (operation_index, operation) in router.operations.iter().enumerate() {
                operation_types.push(TypeScriptOperationTypeTemplate {
                    request_type: operation.request_type.clone(),
                    response_type: operation.response_type.clone(),
                    responses_type: operation.responses_type.clone(),
                    request_params_type: operation.request_params_type.clone(),
                    request_query_type: operation.request_query_type.clone(),
                    request_body_type: operation.request_body_type.clone(),
                    request_headers_type: operation.request_headers_type.clone(),
                    response_body_type: operation.response_body_type.clone(),
                    success_status: operation.success_status.to_string(),
                });
                operations.push(TypeScriptOperationTemplate {
                    operation_key: operation.name.to_case(Case::Camel),
                    request_type: operation.request_type.clone(),
                    responses_type: operation.responses_type.clone(),
                    optional_input: operation.request_params_type == "never"
                        && operation.request_query_type == "never"
                        && operation.request_body_type == "never",
                    service_name: escape_ts_string(&service.name),
                    router_name: escape_ts_string(&router.name),
                    operation_name: escape_ts_string(&operation.name),
                    method: operation.method.to_string(),
                    path: escape_ts_string(&operation.path),
                    last: operation_index + 1 == router.operations.len(),
                });
            }
            routers.push(TypeScriptRouterTemplate {
                router_key,
                operations,
                last: router_index + 1 == service.routers.len(),
            });
        }
        service_blocks.push(TypeScriptServiceTemplate {
            service_key: service_key.clone(),
            routers,
            last: service_index + 1 == services.len(),
        });
        aliases.push(TypeScriptAliasTemplate { alias, service_key });
    }

    render_sdk_template(
        "sdk/typescript/index.ts",
        &TypeScriptSdkTemplate {
            operation_types,
            services: service_blocks,
            aliases,
        },
    )
}

fn generate_rust_sdks(app_root: &Path, services: &[SdkService]) -> Result<()> {
    for service in services {
        let package_name = format!("{}-sdk", service.name);
        let crate_dir = app_root.join("sdks").join("rust").join(&package_name);
        create_dir_all(crate_dir.join("src"))?;
        write(
            crate_dir.join("Cargo.toml"),
            rust_sdk_cargo_toml(&package_name, &service.name),
        )?;
        write(crate_dir.join("src").join("lib.rs"), rust_sdk(service)?)?;
        add_rust_workspace_member(
            app_root,
            &Path::new("sdks")
                .join("rust")
                .join(&package_name)
                .to_string_lossy(),
        )?;
    }
    Ok(())
}

fn rust_sdk_cargo_toml(package_name: &str, service_name: &str) -> String {
    format!(
        "[package]\nname = \"{}\"\nversion.workspace = true\nedition.workspace = true\nlicense.workspace = true\nauthors.workspace = true\ndescription = \"Generated Rust SDK for {}\"\n\n[dependencies]\nreqwest = {{ version = \"0.13.2\", features = [\"json\", \"query\"] }}\nserde = {{ version = \"1.0\", features = [\"derive\"] }}\n",
        package_name, service_name
    )
}

fn rust_sdk(service: &SdkService) -> Result<String> {
    let client_name = format!("{}Client", service.name.to_case(Case::Pascal));
    let mut operations = Vec::new();
    let mut models = BTreeMap::<String, RustModelTemplate>::new();
    let mut aliases = BTreeMap::<String, RustTypeAliasTemplate>::new();
    for router in &service.routers {
        for operation in &router.operations {
            for model in &operation.rust_models {
                models
                    .entry(model.name.clone())
                    .or_insert_with(|| model.clone());
            }
            for alias in &operation.rust_aliases {
                aliases
                    .entry(alias.name.clone())
                    .or_insert_with(|| alias.clone());
            }
            let method_name = format!(
                "{}_{}_request",
                router.name.to_case(Case::Snake),
                operation.name.to_case(Case::Snake)
            );
            let execute_method_name = format!(
                "{}_{}",
                router.name.to_case(Case::Snake),
                operation.name.to_case(Case::Snake)
            );
            operations.push(RustOperationTemplate {
                request_type: operation.request_type.to_case(Case::Pascal),
                response_type: operation.rust_response_type.clone(),
                responses_type: operation.responses_type.to_case(Case::Pascal),
                success_variant: format!("Status{}", operation.success_status),
                success_status: operation.success_status.to_string(),
                execute_method_name,
                method_name,
                http_method: operation.method.to_string(),
                path: escape_rust_string(&operation.path),
            });
        }
    }
    render_sdk_template(
        "sdk/rust/lib.rs",
        &RustSdkTemplate {
            client_name,
            models: models.into_values().collect(),
            aliases: aliases.into_values().collect(),
            operations,
        },
    )
}

fn escape_ts_string(input: &str) -> String {
    input.replace('\\', "\\\\").replace('\'', "\\'")
}

fn escape_rust_string(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

fn render_sdk_template<T: Content>(template_path: &str, data: &T) -> Result<String> {
    let template_file = TEMPLATES_DIR
        .get_file(template_path)
        .with_context(|| format!("Missing SDK template {template_path}"))?;
    let template = Template::new(
        template_file
            .contents_utf8()
            .with_context(|| format!("SDK template {template_path} is not UTF-8"))?,
    )?;
    Ok(template.render(data))
}
