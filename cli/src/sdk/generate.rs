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

    let Some(target_alias) = matches
        .get_one::<String>("target_alias")
        .map(String::as_str)
    else {
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
    let services = match source {
        "contract" => rust_sdk_services_from_contracts(app_root, &manifest, selected_service)?,
        "openapi" => {
            rust_sdk_services_from_artifacts(app_root, &manifest, selected_service, "openapi")?
        }
        // Internal lifecycle scaffolding can run before artifacts exist. This
        // source is deliberately unavailable through the public command.
        "manifest" => rust_sdk_services(&manifest, selected_service),
        _ => bail!("Unsupported SDK artifact source {source}"),
    };

    if services.is_empty() {
        bail!("No Rust services or workers found for SDK generation");
    }

    // Validate every requested output before mutating workspace files.
    if matches!(target, "typescript" | "all") {
        typescript_sdk(&services)?;
    }
    if matches!(target, "rust" | "all") {
        for service in &services {
            rust_sdk(service)?;
        }
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

pub(crate) fn regenerate_sdk_outputs_if_present(
    app_root: &Path,
    stdout: &mut StandardStream,
) -> Result<()> {
    let sdk_root = app_root.join("sdks").join("typescript");
    let typescript_present =
        sdk_root.join("package.json").exists() || sdk_root.join("src/index.ts").exists();
    let rust_root = app_root.join("sdks/rust");
    if !typescript_present && !rust_root.is_dir() {
        return Ok(());
    }
    let manifest = read_manifest(app_root)?;
    let services = rust_sdk_services_for_lifecycle(app_root, &manifest)?;
    if typescript_present {
        generate_typescript_sdk(app_root, &services)?;
        log_ok!(stdout, "Regenerated TypeScript SDK");
    }
    let existing_rust_services = services
        .into_iter()
        .filter(|service| {
            rust_root
                .join(format!("{}-sdk/Cargo.toml", service.name))
                .is_file()
        })
        .collect::<Vec<_>>();
    if !existing_rust_services.is_empty() {
        generate_rust_sdks(app_root, &existing_rust_services)?;
        log_ok!(stdout, "Regenerated Rust SDK crate(s)");
    }
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
    responses_type: String,
    request_params_type: String,
    request_query_type: String,
    request_body_type: String,
    request_headers_type: String,
    request_kind: String,
    request_content_type: String,
    responses: Vec<SdkResponseVariant>,
    rust_models: Vec<RustModelTemplate>,
    rust_aliases: Vec<RustTypeAliasTemplate>,
}

#[derive(Content, Debug, Clone)]
struct SdkResponseVariant {
    response_type: String,
    response_body_type: String,
    rust_response_type: String,
    status_code: u16,
    status_variant: String,
    empty_response: bool,
    json_response: bool,
    text_response: bool,
    binary_response: bool,
    response_kind: String,
    content_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseDecoder {
    Empty,
    Json,
    Text,
    Binary,
}
impl ResponseDecoder {
    fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Json => "json",
            Self::Text => "text",
            Self::Binary => "binary",
        }
    }
}
#[derive(Debug, Clone)]
struct SdkResponseContract {
    status: u16,
    schema: Option<JsonValue>,
    decoder: ResponseDecoder,
    content_type: String,
}
impl SdkResponseContract {
    fn empty(status: u16) -> Self {
        Self {
            status,
            schema: None,
            decoder: ResponseDecoder::Empty,
            content_type: "application/json".into(),
        }
    }
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
    responses_type: String,
    request_params_type: String,
    request_query_type: String,
    request_body_type: String,
    request_headers_type: String,
    responses: Vec<SdkResponseVariant>,
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
    request_kind: String,
    request_content_type: String,
    responses: Vec<SdkResponseVariant>,
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
    union: bool,
    deny_unknown_fields: bool,
    variants: Vec<RustUnionVariantTemplate>,
}

#[derive(Content, Debug, Clone, PartialEq, Eq)]
struct RustUnionVariantTemplate {
    name: String,
    value_type: String,
    first: bool,
}

#[derive(Content, Debug, Clone, PartialEq, Eq)]
struct RustModelFieldTemplate {
    flatten: bool,
    wire_name_literal: String,
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
    responses_type: String,
    responses: Vec<SdkResponseVariant>,
    execute_method_name: String,
    method_name: String,
    http_method: String,
    path: String,
    binary_request: bool,
    request_content_type: String,
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
            let durable_worker = project_type == "Worker"
                && matches!(
                    project.get("variant").and_then(Value::as_str),
                    Some("worker-postgres" | "worker-redis")
                );
            let router_names = if durable_worker {
                Vec::new()
            } else if router_names.is_empty() {
                vec!["health".to_string()]
            } else {
                router_names
            };
            let routers = router_names
                .iter()
                .map(|router| sdk_router(name, router, project_type == "Service"))
                .collect();
            Some(SdkService {
                name: name.to_string(),
                routers,
            })
        })
        .collect()
}

fn sdk_router(service_name: &str, router_name: &str, service: bool) -> SdkRouter {
    let operation_name = if service {
        if router_name == "health" {
            "getHealth".into()
        } else {
            format!("list{}", router_name.to_case(Case::Pascal))
        }
    } else if router_name == "health" {
        "get".to_string()
    } else {
        "list".to_string()
    };
    let path = format!("/{router_name}");
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
            &SdkResponseContract {
                status: 200,
                schema: Some(response_schema),
                decoder: ResponseDecoder::Json,
                content_type: "application/json".into(),
            },
        )],
    }
}

// Lifecycle operations can invalidate one project's artifact. Preserve every
// other project's actual contract, and never conceal malformed artifacts.
fn rust_sdk_services_for_lifecycle(app_root: &Path, manifest: &Value) -> Result<Vec<SdkService>> {
    rust_sdk_services(manifest, None)
        .into_iter()
        .map(|fallback| {
            match rust_sdk_services_from_contracts(app_root, manifest, Some(&fallback.name)) {
                Ok(mut services) => services.pop().context("Missing Rust SDK service"),
                Err(error)
                    if error.chain().any(|cause| {
                        cause
                            .downcast_ref::<std::io::Error>()
                            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
                    }) =>
                {
                    Ok(fallback)
                }
                Err(error) => Err(error),
            }
        })
        .collect()
}

fn rust_sdk_services_from_contracts(
    app_root: &Path,
    manifest: &Value,
    selected_service: Option<&str>,
) -> Result<Vec<SdkService>> {
    rust_sdk_services_from_artifacts(app_root, manifest, selected_service, "contract")
}

fn rust_sdk_services_from_artifacts(
    app_root: &Path,
    manifest: &Value,
    selected_service: Option<&str>,
    source: &str,
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

        let (metadata_key, directory, filename) = if source == "openapi" {
            ("openapiPath", "openapi", "openapi.json")
        } else {
            ("contractArtifactPath", "contracts", "contract.json")
        };
        let artifact_path = metadata
            .get(metadata_key)
            .and_then(Value::as_str)
            .map_or_else(
                || {
                    app_root
                        .join(".forklaunch")
                        .join(directory)
                        .join(service_name)
                        .join(filename)
                },
                |path| app_root.join(path),
            );
        let artifact_path = artifact_path
            .canonicalize()
            .with_context(|| format!("Failed to locate {}", artifact_path.display()))?;
        if !artifact_path.starts_with(app_root.canonicalize()?) {
            bail!("SDK artifact path must stay inside the application");
        }
        if std::fs::metadata(&artifact_path)?.len() > 16 * 1024 * 1024 {
            bail!("SDK artifact exceeds 16 MiB");
        }
        let content = read_to_string(&artifact_path)
            .with_context(|| format!("Failed to read {}", artifact_path.display()))?;
        let artifact = serde_json::from_str::<JsonValue>(&content)
            .with_context(|| format!("Failed to parse {}", artifact_path.display()))?;
        let contract = if source == "openapi" {
            super::openapi::contract_from_openapi(&artifact)
                .with_context(|| format!("Failed to import {}", artifact_path.display()))?
        } else {
            artifact
        };

        services.push(sdk_service_from_contract(service_name, &contract)?);
    }

    Ok(services)
}

fn sdk_service_from_contract(service_name: &str, contract: &JsonValue) -> Result<SdkService> {
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
        let request_shapes = request_type_shapes_from_contract(route)?;
        let responses = response_contracts(route)?;
        let first_response = responses
            .first()
            .cloned()
            .unwrap_or_else(|| SdkResponseContract::empty(200));
        let mut operation = build_sdk_operation(
            service_name,
            &router_name,
            &operation_name,
            &method,
            &path,
            request_shapes.clone(),
            &first_response,
        );
        if responses.is_empty() {
            operation.responses.clear();
            operation.rust_models.clear();
            operation.rust_aliases.clear();
        }
        for response in responses.iter().skip(1) {
            let mut alternative = build_sdk_operation(
                service_name,
                &router_name,
                &format!("{operation_name}Status{}", response.status),
                &method,
                &path,
                request_shapes.clone(),
                response,
            );
            operation.responses.append(&mut alternative.responses);
            operation.rust_models.append(&mut alternative.rust_models);
            operation.rust_aliases.append(&mut alternative.rust_aliases);
        }
        routers.entry(router_name).or_default().push(operation);
    }

    Ok(SdkService {
        name: service_name.to_string(),
        routers: routers
            .into_iter()
            .map(|(name, operations)| SdkRouter { name, operations })
            .collect(),
    })
}

#[derive(Debug, Clone)]
struct RequestTypeShapes {
    kind: String,
    content_type: String,
    params: String,
    query: String,
    body: String,
    headers: String,
}

impl Default for RequestTypeShapes {
    fn default() -> Self {
        Self {
            kind: "json".into(),
            content_type: "application/json".into(),
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
    response: &SdkResponseContract,
) -> SdkOperation {
    let type_prefix = format!(
        "{}{}{}",
        service_name.to_case(Case::Pascal),
        router_name.to_case(Case::Pascal),
        operation_name.to_case(Case::Pascal)
    );
    let status_code = response.status;
    let empty_response = method.eq_ignore_ascii_case("HEAD")
        || matches!(status_code, 204 | 205)
        || response.decoder == ResponseDecoder::Empty;
    let decoder = if empty_response {
        ResponseDecoder::Empty
    } else {
        response.decoder
    };
    let fallback_schema = match decoder {
        ResponseDecoder::Empty => json!({"type":"null"}),
        ResponseDecoder::Text | ResponseDecoder::Binary => json!({"type":"string"}),
        ResponseDecoder::Json => json!({}),
    };
    let response_schema = if matches!(
        decoder,
        ResponseDecoder::Empty | ResponseDecoder::Text | ResponseDecoder::Binary
    ) {
        &fallback_schema
    } else {
        response.schema.as_ref().unwrap_or(&fallback_schema)
    };
    let response_type = format!("{type_prefix}Response");
    let response_type_pascal = rust_type_name(&response_type);
    let (rust_models, mut rust_aliases, rust_response_type) =
        rust_types_from_schema(&response_type_pascal, response_schema);
    if decoder == ResponseDecoder::Binary {
        for alias in &mut rust_aliases {
            if alias.name == rust_response_type {
                alias.target_type = "Vec<u8>".into();
            }
        }
    }

    SdkOperation {
        name: operation_name.to_string(),
        method: method.to_ascii_uppercase(),
        path: path.to_string(),
        request_type: format!("{type_prefix}Request"),
        responses_type: format!("{type_prefix}Responses"),
        request_params_type: request_shapes.params,
        request_query_type: request_shapes.query,
        request_body_type: request_shapes.body,
        request_headers_type: request_shapes.headers,
        request_kind: request_shapes.kind,
        request_content_type: request_shapes.content_type,
        responses: vec![SdkResponseVariant {
            response_type,
            response_body_type: match decoder {
                ResponseDecoder::Empty => "undefined".into(),
                ResponseDecoder::Text => "string".into(),
                ResponseDecoder::Binary => "Uint8Array".into(),
                ResponseDecoder::Json => ts_type_from_schema(response_schema),
            },
            status_code,
            status_variant: format!("Status{status_code}"),
            empty_response,
            json_response: decoder == ResponseDecoder::Json,
            text_response: decoder == ResponseDecoder::Text,
            binary_response: decoder == ResponseDecoder::Binary,
            response_kind: decoder.as_str().into(),
            content_type: response.content_type.clone(),
            rust_response_type,
        }],
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

fn request_type_shapes_from_contract(route: &JsonValue) -> Result<RequestTypeShapes> {
    let Some(request) = route.get("request") else {
        return Ok(RequestTypeShapes::default());
    };

    let params = fields_ts_type(request.get("params"), false);
    let query = fields_ts_type(request.get("query"), false);
    let headers = fields_ts_type(request.get("headers"), true);
    let contract = request.get("body").filter(|body| !body.is_null());
    let kind = match contract
        .and_then(|body| body.get("kind"))
        .and_then(JsonValue::as_str)
    {
        None | Some("json") => "json",
        Some("text") => "text",
        Some("binary" | "file") => "binary",
        Some(kind) => bail!("Unsupported SDK request body kind {kind}"),
    };
    let content_type = normalized_media_type(
        contract
            .and_then(|body| body.get("contentType"))
            .and_then(JsonValue::as_str)
            .unwrap_or(match kind {
                "text" => "text/plain",
                "binary" => "application/octet-stream",
                _ => "application/json",
            }),
    )?;
    let body = if contract.is_none() {
        "never".into()
    } else {
        match kind {
            "text" => "string".into(),
            "binary" => "Uint8Array".into(),
            _ => contract
                .and_then(|body| body.get("schema"))
                .and_then(|schema| schema.get("jsonSchema"))
                .map(ts_type_from_schema)
                .unwrap_or_else(|| "unknown".into()),
        }
    };
    Ok(RequestTypeShapes {
        params,
        query,
        body,
        headers,
        kind: kind.into(),
        content_type,
    })
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

fn normalized_media_type(value: &str) -> Result<String> {
    let value = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if !value
        .split_once('/')
        .is_some_and(|(left, right)| !left.is_empty() && !right.is_empty() && !right.contains('/'))
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/!#$&^_.+-".contains(&byte))
    {
        bail!("Invalid SDK body media type");
    }
    if value == "text/event-stream" || value.starts_with("multipart/") {
        bail!("Streaming/multipart SDK body transport remains unsupported");
    }
    Ok(value)
}

fn response_contracts(route: &JsonValue) -> Result<Vec<SdkResponseContract>> {
    let mut contracts = BTreeMap::new();
    for response in route
        .get("responses")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
    {
        let status = response
            .get("statusCode")
            .and_then(JsonValue::as_u64)
            .filter(|status| (100..=599).contains(status))
            .context("Invalid SDK response status code")? as u16;
        if status < 200 {
            continue;
        }
        let body = response.get("body").filter(|body| !body.is_null());
        let decoder = match body
            .and_then(|body| body.get("kind"))
            .and_then(JsonValue::as_str)
        {
            _ if body.is_none() => ResponseDecoder::Empty,
            None | Some("json") => ResponseDecoder::Json,
            Some("text") => ResponseDecoder::Text,
            Some("binary" | "file") => ResponseDecoder::Binary,
            Some(kind) => bail!("Unsupported SDK response body kind {kind}"),
        };
        let schema = body
            .and_then(|body| body.get("schema"))
            .and_then(|schema| schema.get("jsonSchema"))
            .cloned();
        let content_type = normalized_media_type(
            body.and_then(|body| body.get("contentType"))
                .and_then(JsonValue::as_str)
                .unwrap_or(match decoder {
                    ResponseDecoder::Empty | ResponseDecoder::Json => "application/json",
                    ResponseDecoder::Text => "text/plain",
                    ResponseDecoder::Binary => "application/octet-stream",
                }),
        )?;
        if contracts
            .insert(
                status,
                SdkResponseContract {
                    status,
                    schema,
                    decoder,
                    content_type,
                },
            )
            .is_some()
        {
            bail!("Duplicate SDK response status {status}");
        }
    }
    Ok(contracts.into_values().collect())
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
    if schema.get("nullable") == Some(&JsonValue::Bool(true)) {
        let mut value = schema.clone();
        value.as_object_mut().unwrap().remove("nullable");
        return format!("{} | null", ts_type_from_schema(&value));
    }
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

    if let Some(types) = schema.get("type").and_then(JsonValue::as_array) {
        return types
            .iter()
            .filter_map(JsonValue::as_str)
            .map(|kind| {
                let mut variant = schema.clone();
                variant["type"] = JsonValue::String(kind.to_owned());
                ts_type_from_schema(&variant)
            })
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
    let properties = schema.get("properties").and_then(JsonValue::as_object);
    if properties.is_none_or(serde_json::Map::is_empty) {
        let value_type = match schema.get("additionalProperties") {
            Some(JsonValue::Bool(false)) => "never".into(),
            Some(value) if value.is_object() => ts_type_from_schema(value),
            _ => "unknown".into(),
        };
        return format!("Record<string, {value_type}>");
    }
    let properties = properties.unwrap();

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

    let object_type = format!("{{ {} }}", parts.join("; "));
    if schema.get("additionalProperties") == Some(&JsonValue::Bool(false)) {
        object_type
    } else {
        format!("{object_type} & Record<string, unknown>")
    }
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

// Strip only a representable nullable alternative; other unions retain JSON values.
fn non_null_schema(schema: &JsonValue) -> Option<JsonValue> {
    let mut variant = schema.clone();
    let mut nullable = variant.get("nullable") == Some(&JsonValue::Bool(true));
    variant.as_object_mut()?.remove("nullable");
    if let Some(types) = variant.get("type").and_then(JsonValue::as_array) {
        let non_null = types
            .iter()
            .filter(|value| value.as_str() != Some("null"))
            .cloned()
            .collect::<Vec<_>>();
        if non_null.len() != types.len() && !non_null.is_empty() {
            nullable = true;
            variant["type"] = if non_null.len() == 1 {
                non_null[0].clone()
            } else {
                JsonValue::Array(non_null)
            };
        }
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(alternatives) = variant.get(key).and_then(JsonValue::as_array) {
            if alternatives.len() == 2
                && alternatives
                    .iter()
                    .any(|value| value.get("type").and_then(JsonValue::as_str) == Some("null"))
            {
                return alternatives
                    .iter()
                    .find(|value| value.get("type").and_then(JsonValue::as_str) != Some("null"))
                    .cloned();
            }
        }
    }
    nullable.then_some(variant)
}

fn build_rust_type(
    type_name: &str,
    schema: &JsonValue,
    models: &mut BTreeMap<String, RustModelTemplate>,
) -> String {
    if let Some(variant) = non_null_schema(schema) {
        return format!(
            "Option<{}>",
            build_rust_type(&format!("{type_name}Value"), &variant, models)
        );
    }
    if let Some(alternatives) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(JsonValue::as_array)
    {
        return build_rust_union(type_name, alternatives, models);
    }
    if let Some(types) = schema.get("type").and_then(JsonValue::as_array) {
        if types.len() > 1 {
            let alternatives = types
                .iter()
                .map(|kind| {
                    let mut variant = schema.clone();
                    variant["type"] = kind.clone();
                    variant
                })
                .collect::<Vec<_>>();
            return build_rust_union(type_name, &alternatives, models);
        }
    }
    if let Some(value) = schema.get("const") {
        return match value {
            JsonValue::Null => "()",
            JsonValue::Bool(_) => "bool",
            JsonValue::Number(number) if number.is_i64() => "i64",
            JsonValue::Number(_) => "f64",
            JsonValue::String(_) => "String",
            _ => "serde_json::Value",
        }
        .into();
    }

    match schema_type(schema) {
        Some("object")
            if schema
                .get("properties")
                .and_then(JsonValue::as_object)
                .is_none_or(serde_json::Map::is_empty) =>
        {
            let value_type = schema
                .get("additionalProperties")
                .filter(|value| value.is_object())
                .map(|value| build_rust_type(&format!("{type_name}Value"), value, models))
                .unwrap_or_else(|| "serde_json::Value".into());
            format!("BTreeMap<String, {value_type}>")
        }
        Some("object") => rust_model_from_schema(type_name, schema, models),
        Some("array") => {
            let item_type_name = format!("{}Item", rust_type_name(type_name));
            let item_type = schema
                .get("items")
                .map(|items| build_rust_type(&item_type_name, items, models))
                .unwrap_or_else(|| "serde_json::Value".to_string());
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
        _ => "serde_json::Value".to_string(),
    }
}

fn build_rust_union(
    type_name: &str,
    alternatives: &[JsonValue],
    models: &mut BTreeMap<String, RustModelTemplate>,
) -> String {
    if alternatives.is_empty() {
        return "serde_json::Value".into();
    }
    let base = rust_type_name(type_name);
    let mut name = base.clone();
    let mut suffix = 2;
    while models.contains_key(&name) {
        name = format!("{base}{suffix}");
        suffix += 1;
    }
    models.insert(
        name.clone(),
        RustModelTemplate {
            name: name.clone(),
            fields: vec![],
            union: true,
            deny_unknown_fields: false,
            variants: vec![],
        },
    );
    let variants = alternatives
        .iter()
        .enumerate()
        .map(|(index, schema)| RustUnionVariantTemplate {
            name: format!("Variant{}", index + 1),
            value_type: build_rust_type(&format!("{name}Variant{}", index + 1), schema, models),
            first: index == 0,
        })
        .collect();
    models.get_mut(&name).unwrap().variants = variants;
    name
}

fn rust_model_from_schema(
    type_name: &str,
    schema: &JsonValue,
    models: &mut BTreeMap<String, RustModelTemplate>,
) -> String {
    let base_name = rust_type_name(type_name);
    let mut model_name = base_name.clone();
    let mut suffix = 2;
    while models.contains_key(&model_name) {
        model_name = format!("{base_name}{suffix}");
        suffix += 1;
    }
    // Reserve the name before nested fields allocate their own models.
    models.insert(
        model_name.clone(),
        RustModelTemplate {
            name: model_name.clone(),
            fields: vec![],
            union: false,
            deny_unknown_fields: false,
            variants: vec![],
        },
    );

    let required = schema
        .get("required")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .collect::<BTreeSet<_>>();
    let mut fields = Vec::new();
    let mut field_names = BTreeSet::new();

    if let Some(properties) = schema.get("properties").and_then(JsonValue::as_object) {
        for (name, property_schema) in properties {
            let base_field_name = rust_field_ident(name);
            let mut field_name = base_field_name.clone();
            let mut suffix = 2;
            while !field_names.insert(field_name.clone()) {
                field_name = format!("{}_{suffix}", base_field_name.trim_end_matches('_'));
                suffix += 1;
            }
            let nested_type_name = format!("{}{}", model_name, field_name.to_case(Case::Pascal));
            let mut field_type = build_rust_type(&nested_type_name, property_schema, models);
            if !required.contains(name.as_str()) {
                field_type = format!("Option<{field_type}>");
            }
            fields.push(RustModelFieldTemplate {
                flatten: false,
                wire_name_literal: format!("{name:?}"),
                field_name,
                field_type,
            });
        }
    }

    if schema.get("additionalProperties") != Some(&JsonValue::Bool(false)) {
        let mut field_name = "additional_properties".to_string();
        let mut suffix = 2;
        while !field_names.insert(field_name.clone()) {
            field_name = format!("additional_properties_{suffix}");
            suffix += 1;
        }
        let value_type = schema
            .get("additionalProperties")
            .filter(|value| value.is_object())
            .map(|value| build_rust_type(&format!("{model_name}AdditionalValue"), value, models))
            .unwrap_or_else(|| "serde_json::Value".into());
        fields.push(RustModelFieldTemplate {
            flatten: true,
            wire_name_literal: "\"\"".into(),
            field_name,
            field_type: format!("BTreeMap<String, {value_type}>"),
        });
    }

    models.insert(
        model_name.clone(),
        RustModelTemplate {
            name: model_name.clone(),
            fields,
            union: false,
            deny_unknown_fields: schema.get("additionalProperties")
                == Some(&JsonValue::Bool(false)),
            variants: vec![],
        },
    );
    model_name
}

fn rust_type_name(input: &str) -> String {
    let mut name = input
        .to_case(Case::Pascal)
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    if name.is_empty() {
        name = "GeneratedModel".to_string();
    }
    if name.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        name.insert_str(0, "Model");
    }
    name
}

fn rust_field_ident(input: &str) -> String {
    let mut name = input
        .to_case(Case::Snake)
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if name.is_empty() || name.chars().all(|ch| ch == '_') {
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
        "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "gen"
            | "macro"
            | "override"
            | "priv"
            | "try"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "union"
            | "as"
            | "break"
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
    let compiler_config = sdk_root.join("tsconfig.json");
    if !compiler_config.exists() {
        write(&compiler_config, rust_project::TYPESCRIPT_SDK_TSCONFIG)?;
    }
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
    let mut type_names = BTreeSet::new();
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
                for name in [&operation.request_type, &operation.responses_type]
                    .into_iter()
                    .chain(
                        operation
                            .responses
                            .iter()
                            .map(|response| &response.response_type),
                    )
                {
                    if !type_names.insert(name.clone()) {
                        bail!("Duplicate generated TypeScript SDK type {name}");
                    }
                }
                operation_types.push(TypeScriptOperationTypeTemplate {
                    request_type: operation.request_type.clone(),
                    responses_type: operation.responses_type.clone(),
                    request_params_type: operation.request_params_type.clone(),
                    request_query_type: operation.request_query_type.clone(),
                    request_body_type: operation.request_body_type.clone(),
                    request_headers_type: operation.request_headers_type.clone(),
                    responses: operation.responses.clone(),
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
                    request_kind: operation.request_kind.clone(),
                    request_content_type: operation.request_content_type.clone(),
                    responses: operation.responses.clone(),
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
        "[package]\nname = \"{}\"\nversion.workspace = true\nedition.workspace = true\nlicense.workspace = true\nauthors.workspace = true\ndescription = \"Generated Rust SDK for {}\"\n\n[dependencies]\nreqwest = {{ version = \"0.13.2\", features = [\"json\", \"query\"] }}\nserde = {{ version = \"1.0\", features = [\"derive\"] }}\nserde_json = \"1.0\"\n",
        package_name, service_name
    )
}

fn rust_sdk(service: &SdkService) -> Result<String> {
    let client_name = format!("{}Client", service.name.to_case(Case::Pascal));
    let mut operations = Vec::new();
    let mut models = BTreeMap::<String, RustModelTemplate>::new();
    let mut aliases = BTreeMap::<String, RustTypeAliasTemplate>::new();
    let mut operation_type_names = BTreeSet::new();
    let mut method_names = BTreeSet::new();
    for router in &service.routers {
        for operation in &router.operations {
            for name in [&operation.request_type, &operation.responses_type] {
                let name = name.to_case(Case::Pascal);
                if !operation_type_names.insert(name.clone()) {
                    bail!("Duplicate generated Rust SDK type {name}");
                }
            }
            for model in &operation.rust_models {
                if models
                    .get(&model.name)
                    .is_some_and(|existing| existing != model)
                {
                    bail!("Conflicting generated Rust SDK model {}", model.name);
                }
                models
                    .entry(model.name.clone())
                    .or_insert_with(|| model.clone());
            }
            for alias in &operation.rust_aliases {
                if aliases
                    .get(&alias.name)
                    .is_some_and(|existing| existing != alias)
                {
                    bail!("Conflicting generated Rust SDK alias {}", alias.name);
                }
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
            if !method_names.insert(method_name.clone())
                || !method_names.insert(execute_method_name.clone())
            {
                bail!("Duplicate generated Rust SDK method {execute_method_name}");
            }
            operations.push(RustOperationTemplate {
                request_type: operation.request_type.to_case(Case::Pascal),
                responses_type: operation.responses_type.to_case(Case::Pascal),
                responses: operation.responses.clone(),
                execute_method_name,
                method_name,
                http_method: operation.method.to_string(),
                path: escape_rust_string(&operation.path),
                binary_request: operation.request_kind == "binary",
                request_content_type: operation.request_content_type.clone(),
            });
        }
    }
    for name in models.keys().chain(aliases.keys()) {
        if operation_type_names.contains(name)
            || models.contains_key(name) && aliases.contains_key(name)
        {
            bail!("Conflicting generated Rust SDK type {name}");
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

#[cfg(test)]
mod compiler_config_tests {
    use super::*;
    #[test]
    fn sdk_generation_adds_missing_compiler_configuration_and_preserves_user_settings() {
        let root = tempfile::tempdir().unwrap();
        generate_typescript_sdk(root.path(), &[]).unwrap();
        let path = root.path().join("sdks/typescript/tsconfig.json");
        assert!(path.is_file());
        let custom = r#"{"compilerOptions":{"strict":true,"noUncheckedIndexedAccess":true},"include":["src/**/*.ts","custom/**/*.ts"]}"#;
        write(&path, custom).unwrap();
        generate_typescript_sdk(root.path(), &[]).unwrap();
        assert_eq!(read_to_string(path).unwrap(), custom);
    }
}

#[cfg(test)]
mod wire_name_tests {
    use super::*;

    #[test]
    fn rust_models_preserve_exact_json_property_names() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "recordCount": {"type": "integer"},
                "type": {"type": "string"},
                "user-name": {"type": "string"},
                "quote\"\\\n\u{0000}": {"type": "string"}
            }
        });
        let mut models = BTreeMap::new();
        rust_model_from_schema("Response", &schema, &mut models);
        let fields = &models["Response"].fields;
        for (wire, ident) in [
            ("recordCount", "record_count"),
            ("type", "type_"),
            ("user-name", "user_name"),
        ] {
            let field = fields
                .iter()
                .find(|field| field.field_name == ident)
                .unwrap();
            assert_eq!(field.wire_name_literal, format!("{wire:?}"));
        }
        let source = render_sdk_template(
            "sdk/rust/lib.rs",
            &RustSdkTemplate {
                client_name: "Client".into(),
                models: models.into_values().collect(),
                aliases: vec![],
                operations: vec![],
            },
        )
        .unwrap();
        assert!(source.contains("#[serde(rename = \"recordCount\")]"));
        assert!(source.contains(r#"#[serde(rename = "quote\"\\\n\0")]"#));
    }
}

#[cfg(test)]
mod empty_contract_tests {
    use super::*;
    #[test]
    fn empty_application_contract_does_not_invent_health_routes() {
        let service = sdk_service_from_contract("jobs", &json!({"routes": []})).unwrap();
        assert!(service.routers.is_empty());
        let source = rust_sdk(&service).unwrap();
        assert!(!source.contains("pub async fn"));
        let source = typescript_sdk(&[service]).unwrap();
        assert!(!source.contains("/health"));
    }
}

#[cfg(test)]
mod empty_response_tests {
    use super::*;
    #[test]
    fn absent_bodies_and_json_null_have_distinct_types() {
        for (method, status, body, empty) in [
            ("GET", 200, JsonValue::Null, true),
            ("DELETE", 204, JsonValue::Null, true),
            (
                "POST",
                205,
                json!({"schema":{"jsonSchema":{"type":"string"}}}),
                true,
            ),
            (
                "HEAD",
                200,
                json!({"schema":{"jsonSchema":{"type":"string"}}}),
                true,
            ),
            (
                "GET",
                200,
                json!({"schema":{"jsonSchema":{"type":"null"}}}),
                false,
            ),
        ] {
            let service = sdk_service_from_contract(
                "example",
                &json!({"routes":[{
                    "method":method, "path":"/item", "operationName":"call", "responses":[{"statusCode":status, "body":body}]
                }]}),
            );
            let operation = &service.unwrap().routers[0].operations[0];
            let response = &operation.responses[0];
            assert_eq!(response.empty_response, empty);
            assert_eq!(
                response.response_body_type,
                if empty { "undefined" } else { "null" }
            );
            assert!(operation.rust_aliases.iter().any(|alias| alias.name
                == response.rust_response_type
                && alias.target_type == "()"));
        }
    }
}

#[cfg(test)]
mod multiple_response_tests {
    use super::*;
    #[test]
    fn generates_distinct_success_models_and_all_status_branches() {
        let contract = json!({"routes":[{"method":"GET","path":"/item","operationName":"getItem","responses":[
            {"statusCode":201,"body":{"schema":{"jsonSchema":{"type":"object","properties":{"createdAt":{"type":"string"}}}}}},
            {"statusCode":200,"body":{"schema":{"jsonSchema":{"type":"object","properties":{"displayName":{"type":"string"}}}}}},
            {"statusCode":204}
        ]}]});
        let service = sdk_service_from_contract("example", &contract).unwrap();
        let operation = &service.routers[0].operations[0];
        assert_eq!(
            operation
                .responses
                .iter()
                .map(|response| response.status_code)
                .collect::<Vec<_>>(),
            vec![200, 201, 204]
        );
        assert_ne!(
            operation.responses[0].rust_response_type,
            operation.responses[1].rust_response_type
        );
        let source = rust_sdk(&service).unwrap();
        assert!(source.contains("Status201(ExampleItemGetItemStatus201Response)"));
        assert!(source.contains("pub created_at: Option<String>"));
        let source = typescript_sdk(&[service]).unwrap();
        assert!(source.contains("201: ExampleItemGetItemStatus201Response"));
        assert!(source.contains("204: ExampleItemGetItemStatus204Response"));
    }
    #[test]
    fn colliding_response_model_names_fail_before_generation() {
        let contract = json!({"routes":[
            {"path":"/item","operationName":"getItem","responses":[
                {"statusCode":200},
                {"statusCode":201,"body":{"schema":{"jsonSchema":{"type":"object","properties":{"createdAt":{"type":"string"}}}}}}
            ]},
            {"path":"/item/new","operationName":"getItemStatus201","responses":[
                {"statusCode":200,"body":{"schema":{"jsonSchema":{"type":"object","properties":{"different":{"type":"string"}}}}}}
            ]}
        ]});
        let service = sdk_service_from_contract("example", &contract).unwrap();
        assert!(rust_sdk(&service).is_err());
        assert!(typescript_sdk(&[service]).is_err());
    }
    #[test]
    fn duplicate_statuses_fail_and_error_only_contracts_do_not_invent_success() {
        let duplicate = json!({"routes":[{"path":"/item","responses":[{"statusCode":200},{"statusCode":200}]}]});
        assert!(sdk_service_from_contract("example", &duplicate).is_err());
        let service = sdk_service_from_contract(
            "example",
            &json!({"routes":[{"path":"/item","responses":[{"statusCode":403}]}]}),
        )
        .unwrap();
        assert_eq!(
            service.routers[0].operations[0].responses[0].status_code,
            403
        );
        assert!(!rust_sdk(&service).unwrap().contains("Status200"));
    }
}

#[cfg(test)]
mod response_media_tests {
    use super::*;
    #[test]
    fn declared_errors_and_downloads_have_contract_specific_decoders() {
        let service = sdk_service_from_contract("example", &json!({"routes":[{"path":"/item","operationName":"getItem","responses":[
            {"statusCode":200,"body":{"kind":"binary","contentType":"application/octet-stream"}},
            {"statusCode":400,"body":{"kind":"json","contentType":"application/problem+json","schema":{"jsonSchema":{"type":"object","required":["errorCode"],"properties":{"errorCode":{"type":"string"}}}}}},
            {"statusCode":409,"body":{"kind":"text","contentType":"text/plain; charset=utf-8"}}
        ]}]})).unwrap();
        let operation = &service.routers[0].operations[0];
        assert!(operation.responses[0].binary_response);
        assert_eq!(operation.responses[0].response_body_type, "Uint8Array");
        assert!(operation.responses[1].json_response);
        assert_eq!(
            operation.responses[1].content_type,
            "application/problem+json"
        );
        assert!(operation.responses[2].text_response);
        let source = rust_sdk(&service).unwrap();
        assert!(source.contains("Vec<u8>"));
        assert!(source.contains("pub error_code: String"));
        assert!(source.contains("response.bytes().await?"));
        let source = typescript_sdk(&[service]).unwrap();
        assert!(source.contains("400: 'application/problem+json'"));
        assert!(source.contains("Omit<ForkLaunchErrorResponses"));
    }
    #[test]
    fn streaming_and_multipart_responses_fail_explicitly() {
        for body in [
            json!({"kind":"text","contentType":"text/event-stream"}),
            json!({"kind":"multipart"}),
        ] {
            assert!(
                response_contracts(&json!({"responses":[{"statusCode":200,"body":body}]})).is_err()
            );
        }
    }
    #[test]
    fn unspecified_json_schema_retains_json_value_type() {
        let mut models = BTreeMap::new();
        assert_eq!(
            build_rust_type("Unknown", &json!({}), &mut models),
            "serde_json::Value"
        );
    }
}

#[cfg(test)]
mod identifier_collision_tests {
    use super::*;
    #[test]
    fn fields_and_nested_models_keep_distinct_wire_values_after_normalization() {
        let schema = json!({"type":"object","properties":{
            "fooBar":{"type":"object","properties":{"first":{"type":"string"}}},
            "foo_bar":{"type":"object","properties":{"second":{"type":"integer"}}},
            "foo_bar2":{"type":"object","properties":{"third":{"type":"boolean"}}},
            "foo_bar_2":{"type":"object","properties":{"fourth":{"type":"string"}}},
            "type":{"type":"string"},"type_":{"type":"integer"},"gen":{"type":"string"},"_":{"type":"string"},"quote\"\\":{"type":"string"}
        }});
        let mut models = BTreeMap::new();
        let root = rust_model_from_schema("Response", &schema, &mut models);
        let fields = models[&root]
            .fields
            .iter()
            .filter(|field| !field.flatten)
            .collect::<Vec<_>>();
        assert_eq!(fields.len(), 9);
        assert_eq!(
            fields
                .iter()
                .map(|f| &f.field_name)
                .collect::<BTreeSet<_>>()
                .len(),
            9
        );
        for field in &fields {
            assert!(
                field
                    .field_name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
            );
            assert!(!is_rust_keyword(&field.field_name));
        }
        let nested = fields
            .iter()
            .filter(|field| field.field_type.contains("ResponseFoo"))
            .collect::<Vec<_>>();
        assert_eq!(nested.len(), 4);
        assert_eq!(
            nested
                .iter()
                .map(|f| &f.field_type)
                .collect::<BTreeSet<_>>()
                .len(),
            4
        );
    }
}

#[cfg(test)]
mod nullable_dictionary_tests {
    use super::*;
    #[test]
    fn nullable_dialects_and_dictionary_values_preserve_wire_types() {
        for schema in [
            json!({"type":["string","null"]}),
            json!({"type":"string","nullable":true}),
            json!({"anyOf":[{"type":"string"},{"type":"null"}]}),
            json!({"oneOf":[{"type":"null"},{"type":"string"}]}),
        ] {
            assert_eq!(
                build_rust_type("Value", &schema, &mut BTreeMap::new()),
                "Option<String>"
            );
            assert!(ts_type_from_schema(&schema).contains("null"));
        }
        let dictionary =
            json!({"type":"object","additionalProperties":{"type":["integer","null"]}});
        assert_eq!(
            build_rust_type("Map", &dictionary, &mut BTreeMap::new()),
            "BTreeMap<String, Option<i64>>"
        );
        assert_eq!(
            ts_type_from_schema(&dictionary),
            "Record<string, number | null>"
        );
        assert_eq!(
            build_rust_type("List", &json!({"type":"array"}), &mut BTreeMap::new()),
            "Vec<serde_json::Value>"
        );
        assert_eq!(
            build_rust_type(
                "Union",
                &json!({"type":["string","integer"]}),
                &mut BTreeMap::new()
            ),
            "Union"
        );
        assert_eq!(
            build_rust_type("Constant", &json!({"const":1.5}), &mut BTreeMap::new()),
            "f64"
        );
    }
    #[test]
    fn additional_values_use_a_distinct_flattened_field() {
        let schema = json!({"type":"object","properties":{"additional_properties":{"type":"string"}},"additionalProperties":{"type":"integer"}});
        let mut models = BTreeMap::new();
        rust_model_from_schema("Example", &schema, &mut models);
        let field = models["Example"]
            .fields
            .iter()
            .find(|field| field.flatten)
            .unwrap();
        assert_eq!(field.field_name, "additional_properties_2");
        assert_eq!(field.field_type, "BTreeMap<String, i64>");
        let closed = json!({"type":"object","properties":{"id":{"type":"string"}},"additionalProperties":false});
        rust_model_from_schema("Closed", &closed, &mut models);
        assert!(!models["Closed"].fields.iter().any(|field| field.flatten));
    }
}

#[cfg(test)]
mod lifecycle_contract_tests {
    use super::*;
    fn manifest() -> Value {
        toml::from_str(
            r#"
[[projects]]
name = "existing"
type = "Service"
routers = ["health", "custom"]
[projects.metadata]
language = "rust"
[[projects]]
name = "changed"
type = "Service"
routers = ["health", "internal-users", "audit_log"]
[projects.metadata]
language = "rust"
"#,
        )
        .unwrap()
    }
    #[test]
    fn missing_contract_falls_back_only_for_its_own_service() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".forklaunch/contracts/existing");
        std::fs::create_dir_all(&path).unwrap();
        write(path.join("contract.json"), json!({"routes":[{"path":"/custom","operationName":"fetchActual","responses":[{"statusCode":202,"body":{"schema":{"jsonSchema":{"type":"object","properties":{"realField":{"type":"boolean"}}}}}}]}]}).to_string()).unwrap();
        let services = rust_sdk_services_for_lifecycle(root.path(), &manifest()).unwrap();
        let source = typescript_sdk(&services).unwrap();
        assert!(source.contains("fetchActual:"));
        assert!(source.contains("realField?: boolean"));
        assert!(source.contains("listInternalUsers:"));
        assert!(source.contains("getHealth:"));
        assert!(source.contains("path: '/audit_log'"));
        assert!(!source.contains("listCustom:"));
    }
    #[test]
    fn rust_only_lifecycle_refresh_updates_only_existing_sdk_crates() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(".forklaunch")).unwrap();
        write(
            root.path().join(".forklaunch/manifest.toml"),
            toml::to_string(&manifest()).unwrap(),
        )
        .unwrap();
        write(
            root.path().join("Cargo.toml"),
            "[workspace]\nmembers = []\n",
        )
        .unwrap();
        let sdk = root.path().join("sdks/rust/changed-sdk");
        std::fs::create_dir_all(sdk.join("src")).unwrap();
        write(sdk.join("Cargo.toml"), "old SDK").unwrap();
        write(sdk.join("src/lib.rs"), "stale method").unwrap();
        regenerate_sdk_outputs_if_present(
            root.path(),
            &mut StandardStream::stdout(ColorChoice::Never),
        )
        .unwrap();
        let source = read_to_string(sdk.join("src/lib.rs")).unwrap();
        assert!(source.contains("internal_users_list_internal_users"));
        assert!(!source.contains("stale method"));
        assert!(!root.path().join("sdks/typescript").exists());
        assert!(!root.path().join("sdks/rust/existing-sdk").exists());
    }
    #[test]
    fn malformed_existing_artifact_is_not_silently_replaced() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".forklaunch/contracts/existing");
        std::fs::create_dir_all(&path).unwrap();
        write(path.join("contract.json"), "invalid JSON").unwrap();
        assert!(rust_sdk_services_for_lifecycle(root.path(), &manifest()).is_err());
    }
}

#[cfg(test)]
mod request_media_tests {
    use super::*;
    #[test]
    fn text_binary_and_unspecified_json_requests_have_correct_types() {
        for (kind, media, expected) in [
            ("text", "text/plain; charset=utf-8", "string"),
            ("binary", "application/octet-stream", "Uint8Array"),
            ("file", "image/png", "Uint8Array"),
            ("json", "application/problem+json", "unknown"),
        ] {
            let shapes = request_type_shapes_from_contract(
                &json!({"request":{"body":{"kind":kind,"contentType":media}}}),
            )
            .unwrap();
            assert_eq!(shapes.body, expected);
            assert_eq!(shapes.content_type, media.split(';').next().unwrap());
        }
        let service = sdk_service_from_contract("example", &json!({"routes":[{"path":"/bytes","operationName":"upload","method":"POST","request":{"body":{"kind":"binary"}},"responses":[{"statusCode":204}]}]})).unwrap();
        let source = rust_sdk(&service).unwrap();
        assert!(source.contains("pub body: Option<Vec<u8>>"));
        assert!(source.contains("content_type: \"application/octet-stream\""));
        assert!(
            typescript_sdk(&[service])
                .unwrap()
                .contains("requestKind: 'binary'")
        );
    }
    #[test]
    fn unsupported_request_transports_fail_before_rendering() {
        for body in [
            json!({"kind":"multipart"}),
            json!({"kind":"urlEncoded"}),
            json!({"kind":"text","contentType":"text/event-stream"}),
            json!({"kind":"binary","contentType":"multipart/form-data"}),
            json!({"kind":"json","contentType":"application/json/injected"}),
        ] {
            assert!(request_type_shapes_from_contract(&json!({"request":{"body":body}})).is_err());
        }
    }
}

#[cfg(test)]
mod union_model_tests {
    use super::*;
    #[test]
    fn schema_unions_generate_untagged_models_with_distinct_branches() {
        let mut models = BTreeMap::new();
        let schema = json!({"oneOf":[{"type":"object","required":["name"],"properties":{"name":{"type":"string"}},"additionalProperties":false},{"type":"object","required":["count"],"properties":{"count":{"type":"integer"}},"additionalProperties":false},{"type":"boolean"}]});
        assert_eq!(build_rust_type("Choice", &schema, &mut models), "Choice");
        let union = &models["Choice"];
        assert!(union.union);
        assert_eq!(
            union
                .variants
                .iter()
                .map(|v| v.value_type.as_str())
                .collect::<Vec<_>>(),
            ["ChoiceVariant1", "ChoiceVariant2", "bool"]
        );
        assert!(models["ChoiceVariant1"].deny_unknown_fields);
        let source = render_sdk_template(
            "sdk/rust/lib.rs",
            &RustSdkTemplate {
                client_name: "Client".into(),
                models: models.into_values().collect(),
                aliases: vec![],
                operations: vec![],
            },
        )
        .unwrap();
        assert!(source.contains("#[serde(untagged)]"));
        assert!(source.contains("impl Default for Choice"));
        assert!(source.contains("#[serde(deny_unknown_fields)]"));
    }
}
