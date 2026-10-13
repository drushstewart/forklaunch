#[cfg(test)]
use std::fs::write;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{create_dir_all, read_to_string},
    io::Write,
    path::Path,
};

use anyhow::{Context, Result, bail};
use clap::{Arg, ArgAction, ArgMatches, Command, parser::ValueSource};
use convert_case::{Case, Casing};
use ramhorns::Content;
use serde_json::{Value as JsonValue, json};
use termcolor::{ColorChoice, StandardStream, WriteColor};
use toml::Value;

use super::{
    media::{normalized_media_type, request_media_type},
    models::*,
    render::*,
};
use crate::{
    CliCommand,
    core::{command::command, rendered_template::TEMPLATES_DIR, template::render_embedded_template},
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
                    .value_parser(["openapi", "contract", "socket-contract", "asyncapi", "all"])
                    .help("Artifact source (all combines HTTP contracts and socket contracts)"),
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
    if source == "all" {
        // One watcher serializes both writers because they share workspace metadata.
        // Try both surfaces so a missing HTTP artifact does not block socket updates.
        let http = generate_or_check_sdk_outputs(app_root, target, "contract", selected_service, check, stdout);
        let sockets = super::websocket::generate_from(app_root, target, "socket-contract", selected_service, check, stdout, true);
        return match (http, sockets) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(http), Err(sockets)) => bail!("HTTP SDK: {http:#}; WebSocket SDK: {sockets:#}"),
            (Err(error), _) | (_, Err(error)) => Err(error),
        };
    }
    if matches!(source, "socket-contract" | "asyncapi") {
        return super::websocket::generate_from(
            app_root,
            target,
            source,
            selected_service,
            check,
            stdout,
            false,
        );
    }
    let manifest = read_manifest(app_root)?;
    let services = match source {
        "contract" => http_sdk_services_from_contracts(app_root, &manifest, selected_service)?,
        "openapi" => {
            http_sdk_services_from_artifacts(app_root, &manifest, selected_service, "openapi")?
        }
        // Internal lifecycle scaffolding can run before artifacts exist. This
        // source is deliberately unavailable through the public command.
        "manifest" => rust_sdk_services(&manifest, selected_service),
        _ => bail!("Unsupported SDK artifact source {source}"),
    };

    if services.is_empty() {
        bail!("No HTTP services or workers found for SDK generation");
    }

    let mut tracked_sources = super::http_sources::read(app_root)?;
    for project in typescript_sdk_projects(&manifest, selected_service) {
        if let Some(name) = project.get("name").and_then(Value::as_str)
            && services.iter().any(|service| service.name == name)
        {
            tracked_sources.insert(name.to_owned());
        }
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

    super::http_sources::write(app_root, &tracked_sources)?;
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
    let services_names = services
        .iter()
        .map(|service| service.name.clone())
        .collect::<BTreeSet<_>>();
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
    let imported = typescript_sdk_projects(&manifest, None)
        .into_iter()
        .filter_map(|project| project.get("name").and_then(Value::as_str))
        .filter(|name| services_names.contains(*name))
        .map(str::to_owned)
        .collect();
    super::http_sources::write(app_root, &imported)?;
    Ok(())
}

#[derive(Debug, Clone)]
struct SdkService {
    name: String,
    ts_models: Vec<RustTypeAliasTemplate>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseDecoder {
    Empty,
    Json,
    Text,
    Binary,
    Multipart,
    Sse,
}
impl ResponseDecoder {
    fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Json => "json",
            Self::Text => "text",
            Self::Binary => "binary",
            Self::Multipart => "multipart",
            Self::Sse => "sse",
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
                    Some("worker-postgres" | "worker-redis" | "worker-kafka" | "worker-bullmq")
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
                ts_models: Vec::new(),
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
    let mut services = rust_sdk_services(manifest, None)
        .into_iter()
        .map(|fallback| {
            match http_sdk_services_from_contracts(app_root, manifest, Some(&fallback.name)) {
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
        .collect::<Result<Vec<_>>>()?;
    let tracked = super::http_sources::read(app_root)?;
    for project in typescript_sdk_projects(manifest, None) {
        let name = project
            .get("name")
            .and_then(Value::as_str)
            .context("SDK service name is required")?;
        match http_sdk_services_from_artifacts(app_root, manifest, Some(name), "openapi") {
            Ok(imported) => services.extend(imported),
            Err(error)
                if !tracked.contains(name)
                    && error.chain().any(|cause| {
                        cause
                            .downcast_ref::<std::io::Error>()
                            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
                    }) => {}
            Err(error) => {
                return Err(error.context(format!(
                    "Restore the TypeScript OpenAPI export for {name} before regenerating its SDK"
                )));
            }
        }
    }
    Ok(services)
}

fn http_sdk_services_from_contracts(
    app_root: &Path,
    manifest: &Value,
    selected_service: Option<&str>,
) -> Result<Vec<SdkService>> {
    http_sdk_services_from_artifacts(app_root, manifest, selected_service, "contract")
}

fn typescript_sdk_projects<'a>(
    manifest: &'a Value,
    selected_service: Option<&str>,
) -> Vec<&'a Value> {
    super::http_sources::projects(manifest, selected_service)
        .into_iter()
        .filter(|project| super::http_sources::is_typescript(project))
        .collect()
}

fn http_sdk_services_from_artifacts(
    app_root: &Path,
    manifest: &Value,
    selected_service: Option<&str>,
    source: &str,
) -> Result<Vec<SdkService>> {
    let mut services = Vec::new();
    for project in super::http_sources::projects(manifest, selected_service) {
        let service_name = project
            .get("name")
            .and_then(Value::as_str)
            .context("SDK service name is required")?;
        let (source, artifact_path) = super::http_sources::artifact(app_root, project, source)?;
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
    let mut contract = contract.clone();
    super::versions::expand_contract(&mut contract)?;
    let mut ts_models = Vec::new();
    let mut schema_index = 0;
    fn prepare(
        schema: &mut JsonValue,
        service_name: &str,
        index: &mut usize,
        ts_models: &mut Vec<RustTypeAliasTemplate>,
    ) -> Result<()> {
        *index += 1;
        *schema = super::schema::normalize(
            schema,
            &format!("{}Schema{}", rust_type_name(service_name), index),
        )?;
        if let Some(definitions) = schema
            .get(super::schema::DEFINITIONS)
            .and_then(JsonValue::as_object)
        {
            for (name, definition) in definitions {
                ts_models.push(RustTypeAliasTemplate {
                    name: name.clone(),
                    target_type: ts_type_from_schema(definition),
                });
            }
        }
        Ok(())
    }
    if let Some(routes) = contract.get_mut("routes").and_then(JsonValue::as_array_mut) {
        for route in routes {
            if let Some(request) = route.get_mut("request") {
                for field in ["params", "query", "headers"] {
                    if let Some(fields) = request.get_mut(field).and_then(JsonValue::as_array_mut) {
                        for field in fields {
                            if let Some(schema) = field.pointer_mut("/schema/jsonSchema") {
                                prepare(schema, service_name, &mut schema_index, &mut ts_models)?;
                            }
                        }
                    }
                }
                if let Some(schema) = request.pointer_mut("/body/schema/jsonSchema") {
                    prepare(schema, service_name, &mut schema_index, &mut ts_models)?;
                }
            }
            if let Some(responses) = route.get_mut("responses").and_then(JsonValue::as_array_mut) {
                for response in responses {
                    if let Some(schema) = response.pointer_mut("/body/schema/jsonSchema") {
                        prepare(schema, service_name, &mut schema_index, &mut ts_models)?;
                    }
                }
            }
        }
    }
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
        ts_models,
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
        ResponseDecoder::Text | ResponseDecoder::Binary | ResponseDecoder::Multipart => {
            json!({"type":"string"})
        }
        ResponseDecoder::Json | ResponseDecoder::Sse => json!({}),
    };
    let response_schema = if matches!(
        decoder,
        ResponseDecoder::Empty
            | ResponseDecoder::Text
            | ResponseDecoder::Binary
            | ResponseDecoder::Multipart
    ) {
        &fallback_schema
    } else {
        response.schema.as_ref().unwrap_or(&fallback_schema)
    };
    let response_type = format!("{type_prefix}Response");
    let response_type_pascal = rust_type_name(&response_type);
    let (rust_models, mut rust_aliases, mut rust_response_type) =
        rust_types_from_schema(&response_type_pascal, response_schema);
    if matches!(
        decoder,
        ResponseDecoder::Binary | ResponseDecoder::Multipart
    ) {
        for alias in &mut rust_aliases {
            if alias.name == rust_response_type {
                alias.target_type = if decoder == ResponseDecoder::Multipart {
                    "MultipartResponse"
                } else {
                    "Vec<u8>"
                }
                .into();
            }
        }
    }

    if decoder == ResponseDecoder::Sse {
        rust_response_type = format!("Box<SdkEventStream<{rust_response_type}>>");
    } else if decoder == ResponseDecoder::Json && !rust_models.is_empty() {
        // Object and union payloads can grow arbitrarily large. Keep status
        // enums small without changing the wire format.
        rust_response_type = format!("Box<{rust_response_type}>");
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
                ResponseDecoder::Multipart => "FormData".into(),
                ResponseDecoder::Json => ts_type_from_schema(response_schema),
                ResponseDecoder::Sse => {
                    format!("AsyncIterable<{}>", ts_type_from_schema(response_schema))
                }
            },
            status_code,
            status_variant: format!("Status{status_code}"),
            empty_response,
            json_response: decoder == ResponseDecoder::Json,
            text_response: decoder == ResponseDecoder::Text,
            binary_response: decoder == ResponseDecoder::Binary,
            multipart_response: decoder == ResponseDecoder::Multipart,
            streaming_response: decoder == ResponseDecoder::Sse,
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
        Some("urlEncoded") => "urlEncoded",
        Some("multipart") => "multipart",
        Some("binary" | "file") => "binary",
        Some(kind) => bail!("Unsupported SDK request body kind {kind}"),
    };
    let content_type = request_media_type(
        contract
            .and_then(|body| body.get("contentType"))
            .and_then(JsonValue::as_str)
            .unwrap_or(match kind {
                "text" => "text/plain",
                "binary" => "application/octet-stream",
                "urlEncoded" => "application/x-www-form-urlencoded",
                "multipart" => "multipart/form-data",
                _ => "application/json",
            }),
    )?;
    let normalized_content_type = normalized_media_type(&content_type)?;
    if normalized_content_type == "text/event-stream" {
        bail!("SSE request bodies are unsupported");
    }
    if normalized_content_type.starts_with("multipart/") != (kind == "multipart")
        || kind == "multipart" && normalized_content_type != "multipart/form-data"
    {
        bail!("Multipart SDK requests require multipart/form-data and the multipart body kind");
    }
    let body = if contract.is_none() {
        "never".into()
    } else {
        match kind {
            "text" => "string".into(),
            "binary" => "Uint8Array | ArrayBuffer | Blob".into(),
            "multipart" => format!(
                "FormData | {}",
                contract
                    .and_then(|body| body.get("schema"))
                    .and_then(|schema| schema.get("jsonSchema"))
                    .map(ts_type_from_schema)
                    .unwrap_or_else(|| "Record<string, unknown>".into())
            ),
            "urlEncoded" => format!(
                "URLSearchParams | {}",
                contract
                    .and_then(|body| body.get("schema"))
                    .and_then(|schema| schema.get("jsonSchema"))
                    .map(ts_type_from_schema)
                    .unwrap_or_else(|| { "Record<string, unknown>".into() })
            ),
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
            Some("serverSentEvent") => ResponseDecoder::Sse,
            Some("multipart") => ResponseDecoder::Multipart,
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
                    ResponseDecoder::Multipart => "multipart/form-data",
                    ResponseDecoder::Sse => "text/event-stream",
                }),
        )?;
        if (content_type == "text/event-stream") != (decoder == ResponseDecoder::Sse) {
            bail!("SSE SDK responses require the serverSentEvent body kind and text/event-stream");
        }
        if (decoder == ResponseDecoder::Multipart && content_type != "multipart/form-data")
            || (content_type.starts_with("multipart/") && decoder != ResponseDecoder::Multipart)
        {
            bail!("Multipart SDK responses require multipart body kind and multipart/form-data");
        }
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

fn generate_typescript_sdk(app_root: &Path, services: &[SdkService]) -> Result<()> {
    let sdk_root = app_root.join("sdks").join("typescript");
    create_dir_all(sdk_root.join("src"))?;
    let compiler_config = sdk_root.join("tsconfig.json");
    if !compiler_config.exists() {
        super::write_if_changed(&compiler_config, rust_project::TYPESCRIPT_SDK_TSCONFIG)?;
    }
    super::workspace::include_typescript_sdk(app_root)?;
    super::write_if_changed(
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
    super::write_if_changed(
        sdk_root.join("src").join("index.ts"),
        typescript_sdk(services)?,
    )?;
    Ok(())
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
                &rust_sdk_cargo_toml(
                    &package_name,
                    &service.name,
                    service_has_multipart(service),
                    service_has_multipart_response(service),
                ),
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
    for model in services.iter().flat_map(|service| &service.ts_models) {
        if !type_names.insert(model.name.clone()) {
            bail!("Duplicate generated TypeScript SDK type {}", model.name);
        }
    }
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
                    request_content_type: escape_ts_string(&operation.request_content_type),
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
            schema_models: services
                .iter()
                .flat_map(|service| service.ts_models.clone())
                .collect(),
            sse_source: TEMPLATES_DIR
                .get_file("sdk/typescript/sse.ts")
                .context("Missing SSE SDK helper")?
                .contents_utf8()
                .context("Invalid SSE SDK helper")?
                .to_owned(),
            transport_source: TEMPLATES_DIR
                .get_file("sdk/typescript/transport.ts")
                .context("Missing SDK transport helper")?
                .contents_utf8()
                .context("Invalid SDK transport helper")?
                .to_owned(),
            form_stringify_source: TEMPLATES_DIR
                .get_file("sdk/typescript/safeStringify.ts")
                .context("Missing SDK form stringifier")?
                .contents_utf8()
                .context("Invalid SDK form stringifier")?
                .to_owned(),
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
        super::write_if_changed(
            crate_dir.join("Cargo.toml"),
            rust_sdk_cargo_toml(
                &package_name,
                &service.name,
                service_has_multipart(service),
                service_has_multipart_response(service),
            ),
        )?;
        super::write_if_changed(crate_dir.join("src").join("lib.rs"), rust_sdk(service)?)?;
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

fn service_has_multipart(service: &SdkService) -> bool {
    service.routers.iter().any(|router| {
        router
            .operations
            .iter()
            .any(|operation| operation.request_kind == "multipart")
    })
}

fn service_has_multipart_response(service: &SdkService) -> bool {
    service.routers.iter().any(|router| {
        router.operations.iter().any(|op| {
            op.responses
                .iter()
                .any(|response| response.multipart_response)
        })
    })
}

fn rust_sdk_cargo_toml(
    package_name: &str,
    service_name: &str,
    multipart: bool,
    multipart_response: bool,
) -> String {
    let response_dependencies = if multipart_response {
        "multer = \"3.1\"\nfutures-util = \"0.3\"\n"
    } else {
        ""
    };
    let multipart_feature = if multipart { ", \"multipart\"" } else { "" };
    format!(
        "[package]\nname = \"{}\"\nversion.workspace = true\nedition.workspace = true\nlicense.workspace = true\nauthors.workspace = true\ndescription = \"Generated Rust SDK for {}\"\n\n[dependencies]\nreqwest = {{ version = \"0.13.2\", features = [\"json\", \"query\"{}] }}\nserde = {{ version = \"1.0\", features = [\"derive\"] }}\nserde_json = \"1.0\"\n{}",
        package_name, service_name, multipart_feature, response_dependencies
    )
}

fn rust_sdk(service: &SdkService) -> Result<String> {
    let has_multipart = service_has_multipart(service);
    let has_sse = service.routers.iter().any(|router| {
        router.operations.iter().any(|operation| {
            operation
                .responses
                .iter()
                .any(|response| response.streaming_response)
        })
    });
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
                multipart_request: operation.request_kind == "multipart",
                streaming_response: operation
                    .responses
                    .iter()
                    .any(|response| response.streaming_response),
                multipart_field: if has_multipart {
                    format!(
                        "multipart: {},",
                        if operation.request_kind == "multipart" {
                            "input.body"
                        } else {
                            "None"
                        }
                    )
                } else {
                    String::new()
                },
                request_content_type: escape_rust_string(&operation.request_content_type),
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
            model_source: render_rust_models(
                &models.values().cloned().collect::<Vec<_>>(),
                &aliases.values().cloned().collect::<Vec<_>>(),
            )?,
            has_operations: !operations.is_empty(),
            has_multipart,
            has_multipart_response: service_has_multipart_response(service),
            has_sse,
            sse_source: if has_sse {
                rust_sse_source()?
            } else {
                String::new()
            },
            has_serde: !models.is_empty() || !operations.is_empty(),
            client_name,
            operations,
        },
    )
}

fn escape_rust_string(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

fn rust_sse_source() -> Result<String> {
    let decoder = TEMPLATES_DIR
        .get_file("sdk/rust/sse_decoder.rs")
        .context("Missing SSE decoder")?
        .contents_utf8()
        .context("Invalid SSE decoder")?;
    let decoder = decoder.split("#[cfg(test)]").next().unwrap_or(decoder);
    let client = TEMPLATES_DIR
        .get_file("sdk/rust/sse_client.rs")
        .context("Missing SSE client")?
        .contents_utf8()
        .context("Invalid SSE client")?;
    Ok(format!("pub mod sse {{\n{decoder}\n}}\n{client}"))
}

fn render_sdk_template<T: Content>(template_path: &str, data: &T) -> Result<String> {
    render_embedded_template(template_path, data)
}

#[cfg(test)]
mod compiler_config_tests {
    use super::*;
    #[test]
    fn repeated_generation_preserves_output_and_workspace_timestamps() {
        let root = tempfile::tempdir().unwrap();
        generate_typescript_sdk(root.path(), &[]).unwrap();
        let paths = [
            "package.json",
            "pnpm-workspace.yaml",
            "sdks/typescript/package.json",
            "sdks/typescript/tsconfig.json",
            "sdks/typescript/src/index.ts",
        ];
        let timestamp = std::time::UNIX_EPOCH + std::time::Duration::from_secs(123456789);
        for path in paths {
            std::fs::File::options()
                .write(true)
                .open(root.path().join(path))
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(timestamp))
                .unwrap();
        }
        generate_typescript_sdk(root.path(), &[]).unwrap();
        for path in paths {
            assert_eq!(
                std::fs::metadata(root.path().join(path))
                    .unwrap()
                    .modified()
                    .unwrap(),
                timestamp,
                "rewrote {path}"
            );
        }
    }

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
                model_source: render_rust_models(
                    &models.values().cloned().collect::<Vec<_>>(),
                    &[],
                )
                .unwrap(),
                has_operations: false,
                has_multipart: false,
                has_multipart_response: false,
                has_sse: false,
                sse_source: String::new(),
                has_serde: !models.is_empty(),
                client_name: "Client".into(),
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
    fn durable_worker_lifecycle_fallback_never_advertises_unmounted_routes() {
        let root = tempfile::tempdir().unwrap();
        for variant in [
            "worker-postgres",
            "worker-redis",
            "worker-kafka",
            "worker-bullmq",
        ] {
            let manifest: Value = toml::from_str(&format!(
                r#"
[[projects]]
name = "jobs"
type = "Worker"
variant = "{variant}"
routers = ["health", "process"]
[projects.metadata]
language = "rust"
"#
            ))
            .unwrap();
            let services = rust_sdk_services_for_lifecycle(root.path(), &manifest).unwrap();
            assert_eq!(services.len(), 1);
            assert!(services[0].routers.is_empty());
            assert!(!typescript_sdk(&services).unwrap().contains("/health"));
        }
    }
    #[test]
    fn empty_application_contract_does_not_invent_health_routes() {
        let service = sdk_service_from_contract("jobs", &json!({"routes": []})).unwrap();
        assert!(service.routers.is_empty());
        let source = rust_sdk(&service).unwrap();
        assert!(!source.contains("pub async fn"));
        assert!(!source.contains("use serde::{"));
        assert!(!source.contains("fn path_with_params"));
        assert!(source.contains("_http: reqwest::Client"));
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
        assert!(source.contains("Status201(Box<ExampleItemGetItemStatus201Response>)"));
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
    fn sse_responses_generate_owned_typed_streams_and_configurable_limits() {
        let service = sdk_service_from_contract("events", &json!({"routes":[{"path":"/events","operationName":"listen","method":"GET","responses":[{"statusCode":200,"body":{"kind":"serverSentEvent","schema":{"jsonSchema":{"type":"object","required":["data"],"properties":{"data":{"type":"string"},"id":{"type":"string"}}}}}}]}]})).unwrap();
        assert!(service.routers[0].operations[0].responses[0].streaming_response);
        let source = rust_sdk(&service).unwrap();
        assert!(source.contains("pub struct SdkEventStream<T>"));
        assert!(
            source.contains("SdkEventStream::from_response(response, self.max_sse_event_bytes)")
        );
        assert!(source.contains("pub fn with_sse_event_limit"));
        assert!(source.contains("pub mod sse"));
        let source = typescript_sdk(&[service]).unwrap();
        assert!(source.contains("AsyncIterable<"));
        assert!(source.contains("200: 'sse'"));
        assert!(source.contains("iterator.return = async"));
    }

    #[test]
    fn inconsistent_streaming_and_multipart_response_media_fail_explicitly() {
        for body in [
            json!({"kind":"text","contentType":"text/event-stream"}),
            json!({"kind":"multipart","contentType":"multipart/mixed"}),
            json!({"kind":"text","contentType":"multipart/form-data"}),
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
        let paths = [
            root.path().join("Cargo.toml"),
            sdk.join("Cargo.toml"),
            sdk.join("src/lib.rs"),
        ];
        let timestamp = std::time::UNIX_EPOCH + std::time::Duration::from_secs(123456789);
        for path in &paths {
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(timestamp))
                .unwrap();
        }
        regenerate_sdk_outputs_if_present(
            root.path(),
            &mut StandardStream::stdout(ColorChoice::Never),
        )
        .unwrap();
        for path in &paths {
            assert_eq!(
                std::fs::metadata(path).unwrap().modified().unwrap(),
                timestamp,
                "rewrote {}",
                path.display()
            );
        }
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
            (
                "binary",
                "application/octet-stream",
                "Uint8Array | ArrayBuffer | Blob",
            ),
            ("file", "image/png", "Uint8Array | ArrayBuffer | Blob"),
            ("json", "application/problem+json", "unknown"),
        ] {
            let shapes = request_type_shapes_from_contract(
                &json!({"request":{"body":{"kind":kind,"contentType":media}}}),
            )
            .unwrap();
            assert_eq!(shapes.body, expected);
            assert_eq!(shapes.content_type, media);
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
    fn multipart_requests_use_native_forms_and_encoder_owned_boundaries() {
        let service = sdk_service_from_contract("uploads", &json!({"routes":[{"path":"/upload","operationName":"upload","method":"POST","request":{"body":{"kind":"multipart"}},"responses":[{"statusCode":204}]}]})).unwrap();
        assert!(service_has_multipart(&service));
        let source = rust_sdk(&service).unwrap();
        assert!(source.contains("pub body: Option<MultipartForm>"));
        assert!(source.contains("builder.multipart(form)"));
        assert!(source.contains("multipart: input.body,"));
        assert!(source.contains("name.eq_ignore_ascii_case(\"content-type\")"));
        let cargo: toml::Value =
            toml::from_str(&rust_sdk_cargo_toml("uploads-sdk", "uploads", true, false)).unwrap();
        assert!(
            cargo["dependencies"]["reqwest"]["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|feature| feature.as_str() == Some("multipart"))
        );
        let source = typescript_sdk(&[service]).unwrap();
        assert!(
            source
                .contains("ForkLaunchSdkRequest<never, never, FormData | Record<string, unknown>,")
        );
        assert!(source.contains("requestKind: 'multipart'"));
        assert!(source.contains("requestHeaders.delete('content-type')"));
    }

    #[test]
    fn unsupported_request_transports_fail_before_rendering() {
        for body in [
            json!({"kind":"multipart","contentType":"multipart/mixed"}),
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
                model_source: render_rust_models(
                    &models.values().cloned().collect::<Vec<_>>(),
                    &[],
                )
                .unwrap(),
                has_operations: false,
                has_multipart: false,
                has_multipart_response: false,
                has_sse: false,
                sse_source: String::new(),
                has_serde: !models.is_empty(),
                client_name: "Client".into(),
                operations: vec![],
            },
        )
        .unwrap();
        assert!(source.contains("#[serde(untagged)]"));
        assert!(source.contains("impl Default for Choice"));
        assert!(source.contains("#[serde(deny_unknown_fields)]"));
    }
}

#[cfg(test)]
mod recursive_model_tests {
    use super::*;

    #[test]
    fn required_recursive_defaults_are_omitted_but_guarded_defaults_remain() {
        let schema = super::super::schema::normalize(
            &json!({"type":"object","required":["next"],"properties":{"next":{"$ref":"#"}}}),
            "Cycle",
        )
        .unwrap();
        let (models, _, _) = rust_types_from_schema("Root", &schema);
        assert!(models.iter().all(|model| !model.has_default));
        let schema = super::super::schema::normalize(
            &json!({"type":"object","properties":{"next":{"$ref":"#"}}}),
            "Optional",
        )
        .unwrap();
        let (models, _, _) = rust_types_from_schema("Root", &schema);
        assert!(models.iter().all(|model| model.has_default));
    }

    #[test]
    fn acyclic_refs_preserve_existing_inline_types_and_recursive_maps_have_structural_guards() {
        let schema = super::super::schema::normalize(&json!({"type":"object","required":["label"],"properties":{"label":{"$ref":"#/$defs/Text"}},"$defs":{"Text":{"type":"string"}}}), "Plain").unwrap();
        let (models, _, _) = rust_types_from_schema("Root", &schema);
        assert_eq!(models.len(), 1);
        assert_eq!(
            models[0]
                .fields
                .iter()
                .find(|field| field.field_name == "label")
                .unwrap()
                .field_type,
            "String"
        );
        let schema = super::super::schema::normalize(
            &json!({"type":"object","additionalProperties":{"$ref":"#"}}),
            "Map",
        )
        .unwrap();
        assert_eq!(ts_type_from_schema(&schema), "{ [key: string]: MapRef1 }");
    }
}

#[cfg(test)]
mod mixed_tests;
