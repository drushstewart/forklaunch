//! Parsed non-body fields for the selected version; wire values remain intact.
use super::{
    FieldValidator, SchemaValidationFailure, Variant, VersionBodyHooks, schema_execution_error,
};
use crate::{ContractValidationLocation, FrameworkRequest};
use serde_json::Value;
use std::collections::BTreeMap;

/// Value callbacks around a versioned parameter, query or header schema.
/// Absence and explicit JSON null remain distinct, as with body callbacks.
pub type VersionFieldHooks = VersionBodyHooks;

/// Values produced by the selected version's declared field schemas and hooks.
/// Parameters/query contain declared fields. Headers retain original wire keys;
/// only truthy transformed values replace existing headers, matching the TS
/// request parser. Defaults do not add previously absent header keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionFieldValues {
    pub params: BTreeMap<String, Value>,
    pub query: BTreeMap<String, Value>,
    pub headers: BTreeMap<String, Value>,
}

impl VersionFieldValues {
    /// Preserve existing wire header keys and overlay only truthy projected values.
    /// Matching uses exact lower-case wire keys, as the TypeScript request parser.
    #[must_use]
    pub fn project_wire_headers(&self, headers: &crate::Headers) -> BTreeMap<String, Value> {
        headers
            .iter()
            .map(|(key, raw)| {
                let value = self
                    .headers
                    .get(key)
                    .filter(|value| truthy(value))
                    .cloned()
                    .unwrap_or_else(|| Value::String(raw.clone()));
                (key.clone(), value)
            })
            .collect()
    }
}

impl Variant {
    pub(super) fn parse_fields(
        &self,
        request: &FrameworkRequest,
        parsed_query: Option<&BTreeMap<String, Value>>,
    ) -> Result<Option<VersionFieldValues>, crate::ContractValidationError> {
        let mut output = VersionFieldValues {
            headers: request
                .headers
                .iter()
                .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                .collect(),
            ..Default::default()
        };
        let mut valid = true;
        for field in &self.fields {
            let raw = match field.location {
                ContractValidationLocation::PathParameter => {
                    request.params.get(&field.name).map(String::as_str)
                }
                ContractValidationLocation::Query => {
                    request.query.get(&field.name).map(String::as_str)
                }
                ContractValidationLocation::Header => {
                    crate::header_value(&request.headers, &field.name)
                }
                _ => unreachable!("only request fields are compiled"),
            };
            let raw = raw.map(|value| Value::String(value.to_owned()));
            let value = if field.location == ContractValidationLocation::Query {
                parsed_query.map_or(raw.as_ref(), |query| query.get(&field.name))
            } else {
                raw.as_ref()
            };
            let value = match field.parse(value) {
                Ok(value) => value,
                Err(SchemaValidationFailure::Invalid(_)) => {
                    valid = false;
                    continue;
                }
                Err(SchemaValidationFailure::Operational) => return Err(schema_execution_error()),
            };
            if let Some(value) = value {
                match field.location {
                    ContractValidationLocation::PathParameter => {
                        output.params.insert(field.name.clone(), value);
                    }
                    ContractValidationLocation::Query => {
                        output.query.insert(field.name.clone(), value);
                    }
                    ContractValidationLocation::Header if truthy(&value) => {
                        for (key, target) in &mut output.headers {
                            if key.eq_ignore_ascii_case(&field.name) {
                                *target = value.clone();
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(valid.then_some(output))
    }
}

impl FieldValidator {
    // Errors reject this version; None is a successful absent output.
    fn parse(&self, raw: Option<&Value>) -> Result<Option<Value>, SchemaValidationFailure> {
        let candidate = self
            .hooks
            .preprocess(raw)
            .map_err(|_| SchemaValidationFailure::Invalid(String::new()))?;
        if candidate.is_none() && self.required {
            return Err(SchemaValidationFailure::Invalid(String::new()));
        }
        if let (Some(validate), Some(value)) = (&self.validate, candidate.as_deref()) {
            validate(value)?;
        }
        self.hooks
            .transform(candidate)
            .map_err(|_| SchemaValidationFailure::Invalid(String::new()))
            .map(|value| value.map(std::borrow::Cow::into_owned))
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(value) => !value.is_empty(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        FrameworkResponse, HttpMethod, LocalRouter, RequestContext, VersionBodyHooksRegistry,
    };
    use forklaunch_contract::{
        FieldContract, RequestContract, ResponseContract, RouteContract, VersionedContract,
    };
    use serde_json::json;

    fn contract() -> RouteContract {
        let mut field = FieldContract::required("count", "Count");
        field.schema.json_schema = Some(json!({"type":"integer","minimum":0}).to_string());
        RouteContract::get("Fields", "Fields", "/fields").with_version(VersionedContract {
            version: "one".into(),
            request: RequestContract {
                query: vec![field],
                ..Default::default()
            },
            responses: vec![ResponseContract::new(204, None)],
        })
    }

    #[test]
    fn defaults_and_absent_outputs_reach_local_handlers_without_wire_mutation() {
        let route = contract();
        let mut hooks = VersionBodyHooksRegistry::default();
        hooks
            .insert_field(
                &route,
                "one",
                ContractValidationLocation::Query,
                "count",
                VersionFieldHooks::default()
                    .with_optional_preprocess(|_| Ok(Some(json!(9))))
                    .with_optional_transform(|value| {
                        assert_eq!(value, Some(&json!(9)));
                        Ok(None)
                    }),
            )
            .unwrap();
        let mut router = LocalRouter::new();
        router.register_with_body_hooks(route, &hooks, |request: FrameworkRequest| {
            assert!(request.query.is_empty());
            assert!(request.version.unwrap().parsed_fields.query.is_empty());
            Ok(FrameworkResponse::new(204))
        });
        let response = router
            .execute(FrameworkRequest::new(
                HttpMethod::Get,
                "/fields",
                RequestContext::new("fields"),
            ))
            .unwrap();
        assert_eq!(response.status_code, 204);
    }

    #[test]
    fn registration_rejects_unknown_fields_removed_versions_and_ambiguous_routes() {
        let route = contract();
        let mut hooks = VersionBodyHooksRegistry::default();
        for (version, location, name) in [
            ("missing", ContractValidationLocation::Query, "count"),
            ("one", ContractValidationLocation::Body, "count"),
            ("one", ContractValidationLocation::Query, "unknown"),
        ] {
            assert!(
                hooks
                    .insert_field(
                        &route,
                        version,
                        location,
                        name,
                        VersionFieldHooks::default()
                    )
                    .is_err()
            );
        }
        hooks
            .insert_field(
                &route,
                "one",
                ContractValidationLocation::Query,
                "count",
                VersionFieldHooks::default(),
            )
            .unwrap();
        assert!(hooks.validate_routes(&[]).is_err());
        assert!(
            hooks
                .validate_routes(&[route.clone(), route.clone()])
                .is_err()
        );
        let mut removed = route;
        removed.versions.clear();
        assert!(hooks.compile(&removed).is_err());
    }
}
