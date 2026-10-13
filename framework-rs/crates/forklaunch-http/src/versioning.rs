//! Declaration-ordered request matching for versioned HTTP contracts.
use crate::{
    Body, ContractValidationError, ContractValidationLocation, FrameworkRequest,
    validate_request_fields,
};
use forklaunch_contract::{
    BodyKind, RouteContract, SchemaRef,
    schema_validation::{self, JsonValidator},
};
use serde_json::Value;
use std::sync::Arc;
#[path = "versioning_form.rs"]
mod form;
#[path = "versioning_multipart.rs"]
mod multipart;
pub use multipart::MultipartLimits;

/// Available to local handlers and as an Axum request extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedVersion {
    pub version: String,
    pub matched_versions: Vec<String>,
}

#[derive(Clone)]
struct FieldValidator {
    location: ContractValidationLocation,
    name: String,
    validate: Arc<JsonValidator>,
}
#[derive(Clone)]
struct Variant {
    name: String,
    route: RouteContract,
    fields: Vec<FieldValidator>,
    body: Option<BodyValidator>,
}

#[derive(Clone)]
enum BodyValidator {
    Value(Arc<JsonValidator>),
    Bytes(Arc<JsonValidator>),
    Opaque,
}

/// Reusable schema plan; live routers compile it once during construction.
#[derive(Default, Clone)]
pub struct VersionedRoutePlan {
    variants: Vec<Variant>,
}
impl std::fmt::Debug for VersionedRoutePlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VersionedRoutePlan")
            .field("variant_count", &self.variants.len())
            .finish()
    }
}
impl VersionedRoutePlan {
    /// # Errors
    /// Rejects invalid embedded schemas or excessive version counts.
    pub fn compile(route: &RouteContract) -> Result<Self, String> {
        if route.versions.len() > 1024 {
            return Err("route exceeds 1024 versions".into());
        }
        let parser = parser_kind(route.parsing_body());
        let mut remaining_bytes = 32 * 1024 * 1024_usize;
        let mut variants = Vec::new();
        for name in route.versions.keys() {
            let projected = route.for_version(name).ok_or("missing indexed version")?;
            if parser_kind(projected.request.body.as_ref()) != parser {
                return Err("All API versions must use the same body parser".into());
            }
            if parser == Some(BodyKind::ServerSentEvent) {
                return Err("SSE request bodies are not supported".into());
            }
            let mut fields = Vec::new();
            for (location, entries) in [
                (
                    ContractValidationLocation::PathParameter,
                    &projected.request.params,
                ),
                (ContractValidationLocation::Query, &projected.request.query),
                (
                    ContractValidationLocation::Header,
                    &projected.request.headers,
                ),
            ] {
                for field in entries {
                    if let Some(validate) = compile_schema(&field.schema, &mut remaining_bytes)? {
                        fields.push(FieldValidator {
                            location,
                            name: field.name.clone(),
                            validate,
                        });
                    }
                }
            }
            let body = projected
                .request
                .body
                .as_ref()
                .map(|body| compile_body(body, &mut remaining_bytes))
                .transpose()?
                .flatten();
            variants.push(Variant {
                name: name.clone(),
                route: projected,
                fields,
                body,
            });
        }
        Ok(Self { variants })
    }

    /// Return the last matching declaration, retaining every matching name.
    /// Validation options control rejection at the caller, not version discovery.
    /// # Errors
    /// Returns a validation error when no declared version accepts the request.
    pub fn select(
        &self,
        request: &FrameworkRequest,
    ) -> Result<Option<(RouteContract, SelectedVersion)>, Vec<ContractValidationError>> {
        self.select_with_multipart_limits(request, MultipartLimits::default())
    }

    /// Select a version with the same limits as a configured multipart extractor.
    /// # Errors
    /// Returns validation errors when parsing or every candidate schema fails.
    pub fn select_with_multipart_limits(
        &self,
        request: &FrameworkRequest,
        limits: MultipartLimits,
    ) -> Result<Option<(RouteContract, SelectedVersion)>, Vec<ContractValidationError>> {
        if self.variants.is_empty() {
            return Ok(None);
        }
        let body_value = self.body_value(request, limits);
        let mut matched = Vec::new();
        let mut selected = None;
        for variant in &self.variants {
            if variant.matches(request, body_value.as_ref()) {
                matched.push(variant.name.clone());
                selected = Some(variant);
            }
        }
        selected
            .map(|variant| {
                Some((
                    variant.route.clone(),
                    SelectedVersion {
                        version: variant.name.clone(),
                        matched_versions: matched,
                    },
                ))
            })
            .ok_or_else(|| {
                vec![ContractValidationError::new(
                    ContractValidationLocation::Body,
                    "request does not match any declared API version",
                )]
            })
    }
    // Versions share a parser. Decode once, even when many schemas are tried.
    fn body_value(&self, request: &FrameworkRequest, limits: MultipartLimits) -> Option<Value> {
        let variant = self.variants.iter().find(|variant| {
            matches!(
                variant.body,
                Some(BodyValidator::Value(_) | BodyValidator::Bytes(_))
            )
        })?;
        let kind = variant.route.request.body.as_ref()?.kind;
        let bytes = match &request.body {
            Body::Empty => &[][..],
            Body::Text(text) => text.as_bytes(),
            Body::Binary(bytes) => bytes.as_slice(),
        };
        match kind {
            BodyKind::Json | BodyKind::Unknown => serde_json::from_slice(bytes).ok(),
            BodyKind::File | BodyKind::Binary => Some(Value::Array(
                bytes.iter().copied().map(Value::from).collect(),
            )),
            BodyKind::UrlEncoded => form::parse(bytes),
            BodyKind::Multipart => multipart::parse(
                bytes,
                crate::header_value(&request.headers, "content-type")?,
                limits,
            ),
            _ => std::str::from_utf8(bytes)
                .ok()
                .map(|text| Value::String(text.into())),
        }
    }
}
fn parser_kind(body: Option<&forklaunch_contract::BodyContract>) -> Option<BodyKind> {
    body.map(|body| match body.kind {
        BodyKind::File => BodyKind::Binary,
        BodyKind::Unknown => BodyKind::Json,
        kind => kind,
    })
}
fn decode_schema(schema: &SchemaRef, remaining_bytes: &mut usize) -> Result<Option<Value>, String> {
    schema
        .json_schema
        .as_deref()
        .map(|encoded| {
            *remaining_bytes = remaining_bytes
                .checked_sub(encoded.len())
                .ok_or("Version schemas exceed 32 MiB")?;
            serde_json::from_str(encoded).map_err(|_| "invalid version schema JSON".to_owned())
        })
        .transpose()
}
fn compile_schema(
    schema: &SchemaRef,
    remaining_bytes: &mut usize,
) -> Result<Option<Arc<JsonValidator>>, String> {
    decode_schema(schema, remaining_bytes)?
        .as_ref()
        .map(schema_validation::compile)
        .transpose()
}
fn compile_body(
    body: &forklaunch_contract::BodyContract,
    remaining_bytes: &mut usize,
) -> Result<Option<BodyValidator>, String> {
    let Some(schema) = decode_schema(&body.schema, remaining_bytes)? else {
        return Ok(None);
    };
    let validator = schema_validation::compile(&schema)?;
    if !matches!(body.kind, BodyKind::File | BodyKind::Binary) {
        return Ok(Some(BodyValidator::Value(validator)));
    }
    if schema.is_boolean() || schema.get("type").and_then(Value::as_str) == Some("array") {
        return Ok(Some(BodyValidator::Bytes(validator)));
    }
    // OpenAPI's binary string describes opaque wire bytes, not UTF-8 text.
    // Do not silently discard assertions that cannot apply to that representation.
    let opaque = schema.as_object().is_some_and(|object| {
        object.get("type").and_then(Value::as_str) == Some("string")
            && object.get("format").and_then(Value::as_str) == Some("binary")
            && object.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "type"
                        | "format"
                        | "$schema"
                        | "$id"
                        | "title"
                        | "description"
                        | "default"
                        | "examples"
                        | "example"
                        | "deprecated"
                        | "readOnly"
                        | "writeOnly"
                )
            })
    });
    if opaque {
        Ok(Some(BodyValidator::Opaque))
    } else {
        Err("Versioned binary/file schemas require an opaque binary string, a byte-array schema, or a boolean schema".into())
    }
}
impl Variant {
    fn matches(&self, request: &FrameworkRequest, body_value: Option<&Value>) -> bool {
        if !validate_request_fields(&self.route.request, request).is_empty() {
            return false;
        }
        for field in &self.fields {
            let value = match field.location {
                ContractValidationLocation::PathParameter => {
                    request.params.get(&field.name).map(String::as_str)
                }
                ContractValidationLocation::Query => {
                    request.query.get(&field.name).map(String::as_str)
                }
                ContractValidationLocation::Header => {
                    crate::header_value(&request.headers, &field.name)
                }
                _ => None,
            };
            if let Some(value) = value
                && (field.validate)(&Value::String(value.to_owned())).is_err()
            {
                return false;
            }
        }
        match &self.body {
            None | Some(BodyValidator::Opaque) => true,
            Some(BodyValidator::Value(validate) | BodyValidator::Bytes(validate)) => {
                body_value.is_some_and(|value| validate(value).is_ok())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FrameworkResponse, HttpMethod, LocalRouter, RequestContext};
    use forklaunch_contract::{
        BodyContract, FieldContract, RequestContract, ResponseContract, VersionedContract,
    };
    use serde_json::json;

    fn contract() -> RouteContract {
        let mut route = RouteContract::post("Versioned", "Versioned", "/items/:id");
        route
            .request
            .params
            .push(FieldContract::required("id", "Id"));
        for (name, minimum, status) in [("z-first", 0, 201), ("a-last", 10, 202)] {
            let mut body = BodyContract::json("Input");
            body.schema = body.schema.with_json_schema(json!({"type":"object","required":["value"],"properties":{"value":{"type":"integer","minimum":minimum}},"additionalProperties":false}).to_string());
            route = route.with_version(VersionedContract {
                version: name.into(),
                request: RequestContract::default().with_body(body),
                responses: vec![ResponseContract::new(status, None)],
            });
        }
        route
    }
    fn request(body: &str) -> FrameworkRequest {
        let mut request = FrameworkRequest::new(
            HttpMethod::Post,
            "/items/one",
            RequestContext::new("version-test"),
        )
        .with_header("content-type", "application/json")
        .with_body(Body::Text(body.into()));
        request.params.insert("id".into(), "one".into());
        request
    }
    #[test]
    fn text_versions_normalize_empty_local_bodies_without_altering_bytes() {
        let mut route = contract();
        for version in route.versions.values_mut() {
            let body = version.request.body.as_mut().unwrap();
            body.kind = BodyKind::Text;
            body.content_type = Some("text/plain".into());
            body.schema.json_schema = Some(r#"{"type":"string"}"#.into());
        }
        let mut router = LocalRouter::new();
        router.register(route, |request: FrameworkRequest| {
            assert_eq!(request.version.unwrap().version, "a-last");
            assert_eq!(request.body, Body::Text(String::new()));
            Ok(FrameworkResponse::new(202))
        });
        let request =
            FrameworkRequest::new(HttpMethod::Post, "/items/one", RequestContext::new("empty"))
                .with_header("content-type", "text/plain");
        assert_eq!(router.execute(request).unwrap().status_code, 202);
    }

    #[test]
    fn last_matching_declaration_selects_response_contract_and_all_matches() {
        let route = contract();
        let plan = VersionedRoutePlan::compile(&route).unwrap();
        for (body, name, matches, status) in [
            (r#"{"value":1}"#, "z-first", 1, 201),
            (r#"{"value":11}"#, "a-last", 2, 202),
        ] {
            let (selected, version) = plan.select(&request(body)).unwrap().unwrap();
            assert_eq!(version.version, name);
            assert_eq!(version.matched_versions.len(), matches);
            assert_eq!(selected.responses[0].status_code, status);
            assert_eq!(selected.request.params, route.request.params);
        }
        for invalid in [
            r#"{"value":-1}"#,
            r#"{"value":"10"}"#,
            r#"{"value":11,"extra":true}"#,
            "not json",
        ] {
            assert!(plan.select(&request(invalid)).is_err());
        }
        let mut missing = request(r#"{"value":1}"#);
        missing.params.clear();
        assert!(plan.select(&missing).is_err());
    }
    #[test]
    fn local_dispatch_exposes_selection_and_checks_selected_response_status() {
        let mut router = LocalRouter::new();
        router.register(contract(), |request: FrameworkRequest| {
            let version = request.version.unwrap();
            Ok(FrameworkResponse::new(if version.version == "a-last" {
                202
            } else {
                201
            }))
        });
        assert_eq!(
            router
                .execute(request(r#"{"value":1}"#))
                .unwrap()
                .status_code,
            201
        );
        assert_eq!(
            router
                .execute(request(r#"{"value":11}"#))
                .unwrap()
                .status_code,
            202
        );
        assert_eq!(
            router
                .execute(request(r#"{"value":-1}"#))
                .unwrap()
                .status_code,
            400
        );
        let mut invalid = LocalRouter::new();
        invalid.register(contract(), |_: FrameworkRequest| {
            Ok(FrameworkResponse::new(201))
        });
        assert_eq!(
            invalid
                .execute(request(r#"{"value":11}"#))
                .unwrap()
                .status_code,
            500
        );
    }
    #[test]
    fn binary_versions_validate_bytes_and_reject_unsupported_string_assertions() {
        let mut route = contract();
        for version in route.versions.values_mut() {
            let body = version.request.body.as_mut().unwrap();
            body.kind = BodyKind::Binary;
            body.content_type = Some("application/octet-stream".into());
            body.schema.json_schema = Some(json!({"type":"array","items":{"type":"integer","minimum":0,"maximum":255},"minItems":if version.version=="a-last" {2} else {0}}).to_string());
        }
        let plan = VersionedRoutePlan::compile(&route).unwrap();
        for (bytes, expected) in [
            (vec![], "z-first"),
            (vec![255], "z-first"),
            (vec![0, 255], "a-last"),
        ] {
            let input = request("")
                .with_header("content-type", "application/octet-stream")
                .with_body(Body::Binary(bytes));
            assert_eq!(plan.select(&input).unwrap().unwrap().1.version, expected);
        }
        let body = route.versions["a-last"].request.body.as_mut().unwrap();
        body.schema.json_schema =
            Some(json!({"type":"string","format":"binary","minLength":1}).to_string());
        assert!(
            VersionedRoutePlan::compile(&route)
                .unwrap_err()
                .contains("opaque binary string")
        );
        for schema in [
            json!({"type":"string","format":"binary","title":"File"}),
            json!(true),
            json!(false),
        ] {
            route.versions["a-last"]
                .request
                .body
                .as_mut()
                .unwrap()
                .schema
                .json_schema = Some(schema.to_string());
            let plan = VersionedRoutePlan::compile(&route).unwrap();
            let input = request("")
                .with_header("content-type", "application/octet-stream")
                .with_body(Body::Binary(vec![255]));
            assert_eq!(
                plan.select(&input).unwrap().unwrap().1.version,
                if schema == json!(false) {
                    "z-first"
                } else {
                    "a-last"
                }
            );
        }
    }
    #[test]
    fn malformed_and_unbundled_version_schemas_fail_during_construction() {
        for schema in [
            r#"{"$ref":"https://unbundled.invalid/schema"}"#,
            r#"{"minimum":"bad"}"#,
        ] {
            let mut route = contract();
            route.versions["z-first"]
                .request
                .body
                .as_mut()
                .unwrap()
                .schema
                .json_schema = Some(schema.into());
            assert!(VersionedRoutePlan::compile(&route).is_err());
        }
    }
}
