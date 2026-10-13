//! Checked, transport-capable Rust SDK generation using the shared CLI compiler.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use forklaunch_sdk_codegen::{
    BodyKind, RustModelTemplate, RustTypeAliasTemplate, SdkGenerationPlan,
    render::{RustOperationTemplate, RustSdkTemplate},
    render_rust_models,
    response::{
        compile_response, field_identifier, normalized_media_type, type_identifier,
        validate_identifier,
    },
};

use crate::RustSdkOptions;

/// Generate a reqwest client with typed status responses and owned SSE streams.
///
/// The generated crate requires reqwest (`json`, `query`, and `multipart` when
/// used), serde (`derive`) and `serde_json`. JSON request bodies remain encoded
/// strings; binary bodies and multipart forms use their owned transport types.
/// Multipart responses additionally require multer 3.1 and futures-util 0.3.
///
/// # Errors
/// Rejects invalid schemas/media, unsupported request streams, duplicate names,
/// conflicting types and invalid status codes before returning generated source.
pub fn generate_rust_http_sdk_source(
    plan: &SdkGenerationPlan,
    options: &RustSdkOptions,
) -> Result<String> {
    validate_identifier(&options.client_struct)?;
    let has_multipart = plan
        .operations
        .iter()
        .any(|op| op.request_body_kind == Some(BodyKind::Multipart));
    let mut models = BTreeMap::<String, RustModelTemplate>::new();
    let mut aliases = BTreeMap::<String, RustTypeAliasTemplate>::new();
    let mut names = reserved_type_names();
    if !names.insert(options.client_struct.clone()) {
        bail!(
            "Conflicting generated Rust SDK client type {}",
            options.client_struct
        );
    }
    let mut methods = BTreeSet::from([
        "new".to_string(),
        "with_http_client".into(),
        "with_sse_event_limit".into(),
        "with_multipart_response_limit".into(),
        "with_default_header".into(),
        "with_bearer_token".into(),
        "with_jwt".into(),
        "with_basic_auth".into(),
        "with_hmac_authorization".into(),
        "with_auth_hook".into(),
    ]);
    let mut operations = Vec::new();
    for op in &plan.operations {
        if !op.path.starts_with('/') {
            bail!("SDK operation path must start with /");
        }
        let prefix = type_identifier(&op.name);
        let request_type = format!("{prefix}Request");
        let responses_type = format!("{prefix}Responses");
        for name in [&request_type, &responses_type] {
            if !names.insert(name.clone()) {
                bail!("Duplicate generated Rust SDK type {name}");
            }
        }
        let execute_method_name = field_identifier(&op.name);
        let method_name = format!("{execute_method_name}_request");
        for name in [&execute_method_name, &method_name] {
            if !methods.insert(name.clone()) {
                bail!("Duplicate generated Rust SDK method {name}");
            }
        }
        let (kind, content_type) = request_content_type(op)?;
        let responses = compile_responses(op, &prefix, &mut models, &mut aliases)?;
        operations.push(RustOperationTemplate {
            request_type,
            responses_type,
            streaming_response: responses.iter().any(|response| response.streaming_response),
            responses,
            execute_method_name,
            method_name,
            http_method: op.method.as_str().into(),
            path: op.path.replace('\\', "\\\\").replace('"', "\\\""),
            binary_request: matches!(kind, BodyKind::Binary | BodyKind::File),
            multipart_request: kind == BodyKind::Multipart,
            multipart_field: if has_multipart {
                format!(
                    "multipart: {},",
                    if kind == BodyKind::Multipart {
                        "input.body"
                    } else {
                        "None"
                    }
                )
            } else {
                String::new()
            },
            request_content_type: content_type.replace('\\', "\\\\").replace('"', "\\\""),
        });
    }
    for name in models.keys().chain(aliases.keys()) {
        if names.contains(name) || models.contains_key(name) && aliases.contains_key(name) {
            bail!("Conflicting generated Rust SDK type {name}");
        }
    }
    let has_sse = operations.iter().any(|op| op.streaming_response);
    let data = RustSdkTemplate {
        model_source: render_rust_models(
            &models.into_values().collect::<Vec<_>>(),
            &aliases.into_values().collect::<Vec<_>>(),
        )?,
        has_operations: !operations.is_empty(),
        has_multipart,
        has_multipart_response: operations.iter().any(|op| {
            op.responses
                .iter()
                .any(|response| response.multipart_response)
        }),
        has_sse,
        has_serde: !operations.is_empty(),
        client_name: options.client_struct.clone(),
        sse_source: if has_sse { sse_source() } else { String::new() },
        operations,
    };
    Ok(ramhorns::Template::new(include_str!("http_client.rs.template"))?.render(&data))
}

fn request_content_type(op: &forklaunch_sdk_codegen::SdkOperation) -> Result<(BodyKind, String)> {
    if let Some(name) = &op.request_body {
        forklaunch_sdk_codegen::compile_schema_ref(
            "RequestBody",
            &forklaunch_sdk_codegen::SchemaRef {
                name: name.clone(),
                json_schema: op.request_body_json_schema.clone(),
            },
        )?;
    }
    for field in op
        .request_params
        .iter()
        .chain(&op.request_query)
        .chain(&op.request_headers)
    {
        forklaunch_sdk_codegen::compile_schema_ref(
            "RequestField",
            &forklaunch_sdk_codegen::SchemaRef {
                name: field.schema.clone(),
                json_schema: field.json_schema.clone(),
            },
        )?;
    }
    let kind = op.request_body_kind.unwrap_or(BodyKind::Json);
    if kind == BodyKind::ServerSentEvent {
        bail!("SSE request bodies are unsupported");
    }
    let content_type = forklaunch_sdk_codegen::media::request_media_type(
        op.request_body_content_type
            .as_deref()
            .unwrap_or(match kind {
                BodyKind::Text => "text/plain",
                BodyKind::Binary | BodyKind::File => "application/octet-stream",
                BodyKind::Multipart => "multipart/form-data",
                BodyKind::UrlEncoded => "application/x-www-form-urlencoded",
                _ => "application/json",
            }),
    )?;
    let normalized_content_type = normalized_media_type(&content_type)?;
    if normalized_content_type == "text/event-stream" {
        bail!("SSE request bodies are unsupported");
    }
    if normalized_content_type.starts_with("multipart/") != (kind == BodyKind::Multipart)
        || kind == BodyKind::Multipart && normalized_content_type != "multipart/form-data"
    {
        bail!("Multipart SDK requests require multipart/form-data and the multipart body kind");
    }
    Ok((kind, content_type))
}

fn compile_responses(
    op: &forklaunch_sdk_codegen::SdkOperation,
    prefix: &str,
    models: &mut BTreeMap<String, RustModelTemplate>,
    aliases: &mut BTreeMap<String, RustTypeAliasTemplate>,
) -> Result<Vec<forklaunch_sdk_codegen::render::SdkResponseVariant>> {
    let mut declared = BTreeMap::new();
    for response in &op.responses {
        if !(100..=599).contains(&response.status_code) {
            bail!("Invalid SDK response status code");
        }
        if response.status_code < 200 {
            continue;
        }
        if declared.insert(response.status_code, response).is_some() {
            bail!("Duplicate SDK response status {}", response.status_code);
        }
    }
    let mut responses = Vec::new();
    for (index, (status, response)) in declared.into_iter().enumerate() {
        let root = if index == 0 {
            format!("{prefix}Response")
        } else {
            format!("{prefix}Status{status}Response")
        };
        let (variant, compiled) = compile_response(op.method, response, &root)?;
        for model in compiled.rust_models {
            if models
                .get(&model.name)
                .is_some_and(|existing| existing != &model)
            {
                bail!("Conflicting generated Rust SDK model {}", model.name);
            }
            models.insert(model.name.clone(), model);
        }
        for alias in compiled.rust_aliases {
            if aliases
                .get(&alias.name)
                .is_some_and(|existing| existing != &alias)
            {
                bail!("Conflicting generated Rust SDK alias {}", alias.name);
            }
            aliases.insert(alias.name.clone(), alias);
        }
        responses.push(variant);
    }
    Ok(responses)
}

fn sse_source() -> String {
    let decoder = include_str!("sse_decoder.rs")
        .split("#[cfg(test)]")
        .next()
        .unwrap_or_default();
    format!(
        "pub mod sse {{\n{decoder}\n}}\n{}",
        include_str!("sse_client.rs")
    )
}

fn reserved_type_names() -> BTreeSet<String> {
    [
        "SdkRequest",
        "SdkError",
        "SdkAuth",
        "SdkEventStream",
        "MultipartResponse",
        "MultipartResponsePart",
        "SdkRequestHook",
        "String",
        "Vec",
        "Option",
        "Result",
        "Box",
        "BTreeMap",
        "Arc",
        "Error",
        "Serialize",
        "Deserialize",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use forklaunch_contract::{
        BodyContract, ContractRegistry, HttpMethod, RouteContract, SchemaRef,
    };
    use forklaunch_sdk_codegen::SdkTarget;

    fn plan(routes: Vec<RouteContract>) -> SdkGenerationPlan {
        let mut registry = ContractRegistry::new();
        for route in routes {
            registry.register(route);
        }
        SdkGenerationPlan::from_registry(SdkTarget::Rust, &registry)
    }

    #[test]
    fn native_http_rejects_invalid_graphs_names_and_transports() {
        let options = RustSdkOptions::new("test-sdk");
        for schema in [
            r#"{"$ref":"https://example.invalid/schema"}"#,
            r##"{"$ref":"#"}"##,
        ] {
            let route = RouteContract::get("Read", "Invalid graph", "/value").with_response(
                200,
                Some(BodyContract {
                    schema: SchemaRef::named("Value").with_json_schema(schema),
                    ..BodyContract::json("Value")
                }),
            );
            assert!(generate_rust_http_sdk_source(&plan(vec![route]), &options).is_err());
        }
        for name in ["new", "With Bearer Token", "With Http Client"] {
            assert!(
                generate_rust_http_sdk_source(
                    &plan(vec![RouteContract::get(name, "Collision", "/")]),
                    &options
                )
                .is_err()
            );
        }
        assert!(
            generate_rust_http_sdk_source(
                &plan(vec![]),
                &options.clone().with_client_struct("type")
            )
            .is_err()
        );
        for name in [
            "SdkRequest",
            "String",
            "SdkAuth",
            "SdkEventStream",
            "MultipartResponse",
            "MultipartResponsePart",
            "Deserialize",
        ] {
            assert!(
                generate_rust_http_sdk_source(
                    &plan(vec![]),
                    &options.clone().with_client_struct(name)
                )
                .is_err()
            );
        }
        for kind in [BodyKind::Multipart, BodyKind::UrlEncoded, BodyKind::Unknown] {
            let route = RouteContract::get("Read", "Unsupported response", "/").with_response(
                200,
                Some(BodyContract {
                    kind,
                    ..BodyContract::json("Value")
                }),
            );
            assert!(generate_rust_http_sdk_source(&plan(vec![route]), &options).is_err());
        }
        let route = RouteContract::get("Read", "Duplicate status", "/")
            .with_response(200, None)
            .with_response(200, None);
        assert!(generate_rust_http_sdk_source(&plan(vec![route]), &options).is_err());
    }

    #[test]
    fn native_http_preserves_status_media_null_and_head() {
        let route = RouteContract::get("Read", "All statuses", "/value")
            .with_response(200, Some(BodyContract::json("String")))
            .with_response(
                201,
                Some(BodyContract {
                    schema: SchemaRef::named("Null").with_json_schema(r#"{"type":"null"}"#),
                    ..BodyContract::json("Null")
                }),
            )
            .with_response(204, None)
            .with_response(
                409,
                Some(BodyContract {
                    kind: BodyKind::Text,
                    content_type: Some("text/plain; charset=utf-8".into()),
                    ..BodyContract::json("String")
                }),
            );
        let source =
            generate_rust_http_sdk_source(&plan(vec![route]), &RustSdkOptions::new("test-sdk"))
                .unwrap();
        assert!(source.contains("pub type ReadResponse = String;"));
        assert!(source.contains("pub type ReadStatus201Response = ();"));
        assert!(source.contains("Status204(())"));
        assert!(source.contains("409 => Ok(ReadResponses::Status409(response.text().await?))"));
        let head = RouteContract::new("Inspect", "HEAD", HttpMethod::Head, "/")
            .with_response(200, Some(BodyContract::json("String")));
        let source =
            generate_rust_http_sdk_source(&plan(vec![head]), &RustSdkOptions::new("test-sdk"))
                .unwrap();
        assert!(source.contains("pub type InspectResponse = ();"));
        assert!(!source.contains("response.json::<"));
    }
}
