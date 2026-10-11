//! Shared response media classification and typed payload compilation.
use anyhow::{Context, Result, bail};
use forklaunch_contract::{HttpMethod, SchemaRef};
use serde_json::json;

use crate::{BodyKind, SchemaModels, SdkResponse, compile_schema_ref, render::SdkResponseVariant};

/// Compile a response into the same model and decoder used by generated CLI SDKs.
///
/// # Errors
/// Rejects unsupported media, invalid status codes and invalid schema graphs.
pub fn compile_response(
    method: HttpMethod,
    response: &SdkResponse,
    root_name: &str,
) -> Result<(SdkResponseVariant, SchemaModels)> {
    if !(200..=599).contains(&response.status_code) {
        bail!("Invalid SDK response status code");
    }
    let empty = method == HttpMethod::Head
        || matches!(response.status_code, 204 | 205)
        || response.body.is_none();
    let kind = response.body_kind.unwrap_or(BodyKind::Json);
    let (decoder, default_mime) = match kind {
        _ if response.body.is_none() => ("empty", "application/json"),
        BodyKind::Json => ("json", "application/json"),
        BodyKind::Text => ("text", "text/plain"),
        BodyKind::Binary | BodyKind::File => ("binary", "application/octet-stream"),
        BodyKind::ServerSentEvent => ("sse", "text/event-stream"),
        _ => bail!(
            "Unsupported SDK response body kind {}",
            kind.as_artifact_str()
        ),
    };
    let content_type = normalized_media_type(
        response
            .body_content_type
            .as_deref()
            .unwrap_or(default_mime),
    )?;
    if (content_type == "text/event-stream") != (decoder == "sse") {
        bail!("SSE SDK responses require the serverSentEvent body kind and text/event-stream");
    }
    if content_type.starts_with("multipart/") {
        bail!("Multipart SDK response transport remains unsupported");
    }
    let decoder = if empty { "empty" } else { decoder };
    let schema = match decoder {
        "empty" => SchemaRef::named("Null").with_json_schema(json!({"type":"null"}).to_string()),
        "text" | "binary" => SchemaRef::named("String"),
        _ => SchemaRef {
            name: response.body.clone().unwrap_or_default(),
            json_schema: response.body_json_schema.clone(),
        },
    };
    let mut models = compile_schema_ref(root_name, &schema)?;
    if decoder == "binary" {
        for alias in &mut models.rust_aliases {
            if alias.name == models.rust_type {
                alias.target_type = "Vec<u8>".into();
            }
        }
    }
    let rust_response_type = match decoder {
        "sse" => format!("Box<SdkEventStream<{}>>", models.rust_type),
        "json" if !models.rust_models.is_empty() => format!("Box<{}>", models.rust_type),
        _ => models.rust_type.clone(),
    };
    let response_body_type = match decoder {
        "empty" => "undefined".into(),
        "binary" => "Uint8Array".into(),
        "sse" => format!("AsyncIterable<{}>", models.typescript_type),
        _ => models.typescript_type.clone(),
    };
    Ok((
        SdkResponseVariant {
            response_type: root_name.into(),
            response_body_type,
            rust_response_type,
            status_code: response.status_code,
            status_variant: format!("Status{}", response.status_code),
            empty_response: empty,
            json_response: decoder == "json",
            text_response: decoder == "text",
            binary_response: decoder == "binary",
            streaming_response: decoder == "sse",
            response_kind: decoder.into(),
            content_type,
        },
        models,
    ))
}

/// Normalize a declared MIME type, ignoring case and parameters.
///
/// # Errors
/// Rejects invalid MIME tokens.
pub fn normalized_media_type(value: &str) -> Result<String> {
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
    Ok(value)
}

/// Normalize a schema type name for Rust declarations.
#[must_use]
pub fn type_identifier(name: &str) -> String {
    crate::models::rust_type_name(name)
}

/// Normalize a Rust method/field identifier, including reserved keywords.
#[must_use]
pub fn field_identifier(name: &str) -> String {
    crate::models::rust_field_ident(name)
}

/// Validate an explicitly supplied Rust client identifier.
///
/// # Errors
/// Rejects punctuation, leading digits and reserved Rust keywords.
pub fn validate_identifier(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let first = chars.next().context("Rust client identifier is empty")?;
    if !(first.is_ascii_alphabetic() || first == '_')
        || name == "_"
        || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        || crate::models::is_rust_keyword(name)
    {
        bail!("Invalid Rust client identifier {name}");
    }
    Ok(())
}
