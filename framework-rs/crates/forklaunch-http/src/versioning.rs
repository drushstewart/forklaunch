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
    body: Option<Arc<JsonValidator>>,
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
        let mut variants = Vec::new();
        for name in route.versions.keys() {
            let projected = route.for_version(name).ok_or("missing indexed version")?;
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
                    if let Some(validate) = compile_schema(&field.schema)? {
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
                .map(|body| compile_schema(&body.schema))
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
        if self.variants.is_empty() {
            return Ok(None);
        }
        let mut matched = Vec::new();
        let mut selected = None;
        for variant in &self.variants {
            if variant.matches(request) {
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
}
fn compile_schema(schema: &SchemaRef) -> Result<Option<Arc<JsonValidator>>, String> {
    schema
        .json_schema
        .as_deref()
        .map(|encoded| {
            let value: Value = serde_json::from_str(encoded)
                .map_err(|_| "invalid version schema JSON".to_owned())?;
            schema_validation::compile(&value)
        })
        .transpose()
}
impl Variant {
    fn matches(&self, request: &FrameworkRequest) -> bool {
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
        let Some(validate) = &self.body else {
            return true;
        };
        let body = self
            .route
            .request
            .body
            .as_ref()
            .expect("body validator requires contract");
        let bytes = match &request.body {
            Body::Empty => &[][..],
            Body::Text(text) => text.as_bytes(),
            Body::Binary(bytes) => bytes.as_slice(),
        };
        let value = if body.kind == BodyKind::Json {
            match serde_json::from_slice(bytes) {
                Ok(value) => value,
                Err(_) => return false,
            }
        } else {
            match std::str::from_utf8(bytes) {
                Ok(text) => Value::String(text.into()),
                Err(_) => return false,
            }
        };
        validate(&value).is_ok()
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
