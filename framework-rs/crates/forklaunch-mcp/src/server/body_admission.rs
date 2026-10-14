//! Explicit finite body admission for HTTP-backed tools; no schema transforms.
use forklaunch_contract::{
    BodyKind, RouteContract,
    schema_validation::{self, JsonValidator},
};
use serde_json::{Map, Value, json};
use std::{collections::BTreeMap, sync::Arc};

/// Only explicit root defaults or omission are applied; null remains a supplied value.
#[derive(Clone, Debug)]
pub enum MissingToolBody {
    Reject,
    /// Admit absence without inventing a JSON value or wire body.
    Omit,
    Default(Value),
}
/// Schema-bound admission policy. JSON Schema annotations never inject defaults.
#[derive(Clone, Debug)]
pub struct McpBodyAdmission {
    missing: MissingToolBody,
    allow_null: bool,
    max_bytes: usize,
    max_depth: usize,
}
impl McpBodyAdmission {
    /// Bounds apply before schema evaluation and default injection. Nullable text
    /// is encoded as zero bytes after validation, matching fetch; HTTP text parser
    /// framing/projection semantics remain those of the configured HTTP router.
    #[must_use]
    pub const fn new(
        missing: MissingToolBody,
        allow_null: bool,
        max_bytes: usize,
        max_depth: usize,
    ) -> Self {
        Self {
            missing,
            allow_null,
            max_bytes,
            max_depth,
        }
    }
}
#[derive(Clone)]
pub(super) struct Admission {
    route: RouteContract,
    policy: McpBodyAdmission,
    validate: Arc<JsonValidator>,
    schema: Value,
}
/// Opt-in policies for exact projected HTTP tool contracts. Unregistered tools
/// retain native admission. Defaults are trusted configuration, not recursive
/// JSON Schema defaults or arbitrary Zod transformations/unknown-key stripping.
#[derive(Clone, Default)]
pub struct McpBodyAdmissions {
    entries: BTreeMap<String, Arc<Admission>>,
}
impl McpBodyAdmissions {
    /// Register a projected, unversioned tool route with an explicit inline schema.
    /// Versioned callers register each `for_version` projection using its final
    /// tool operation name. Registration is checked again against mounted tools.
    /// # Errors
    /// Rejects unsupported bodies, stale/duplicate routes, invalid schemas/defaults or bounds.
    pub fn insert(
        &mut self,
        route: &RouteContract,
        policy: McpBodyAdmission,
    ) -> Result<(), String> {
        if !route.versions.is_empty()
            || !route.options.mcp
            || policy.max_bytes == 0
            || policy.max_bytes > 2 * 1024 * 1024
            || policy.max_depth == 0
            || policy.max_depth > 64
        {
            return Err("Invalid MCP body admission bounds or route".into());
        }
        let body = route
            .request
            .body
            .as_ref()
            .ok_or("MCP body admission requires a body")?;
        if !matches!(body.kind, BodyKind::Json | BodyKind::Text) {
            return Err("MCP body admission supports JSON/text only".into());
        }
        let source = body
            .schema
            .json_schema
            .as_ref()
            .ok_or("MCP body admission requires an explicit schema")?;
        let schema: Value = serde_json::from_str(source).map_err(|_| "Invalid MCP body schema")?;
        let validate = schema_validation::compile(&schema)?;
        if policy.allow_null && validate(&Value::Null).is_err() {
            return Err("Nullable admission requires a nullable declared schema".into());
        }
        let mut schema = if schema.is_object() {
            schema
        } else {
            json!({"allOf":[schema]})
        };
        let object = schema.as_object_mut().ok_or("Invalid body schema")?;
        object.remove("default");
        let mut restrictions = Vec::new();
        if !policy.allow_null {
            restrictions.push(json!({"not":{"type":"null"}}));
        }
        if body.kind == BodyKind::Text {
            restrictions.push(json!({"type":["string","null"]}));
        }
        if !restrictions.is_empty() {
            object
                .entry("allOf")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or("Invalid body schema")?
                .extend(restrictions);
        }
        let admission = Admission {
            route: route.clone(),
            policy,
            validate,
            schema,
        };
        if let MissingToolBody::Default(value) = &admission.policy.missing {
            admission.check(value).map_err(str::to_owned)?;
        }
        if self.entries.contains_key(&route.operation_name) {
            return Err("Duplicate MCP body admission".into());
        }
        self.entries
            .insert(route.operation_name.clone(), Arc::new(admission));
        Ok(())
    }
    pub(super) fn validate_routes(&self, routes: &[RouteContract]) -> Result<(), String> {
        for (name, entry) in &self.entries {
            let matches: Vec<_> = routes
                .iter()
                .filter(|route| route.operation_name == *name)
                .collect();
            if matches.len() != 1 || matches[0] != &entry.route {
                return Err("Missing or stale MCP body admission".into());
            }
        }
        Ok(())
    }
    pub(super) fn project(
        &self,
        route: &RouteContract,
        tool: &mut rmcp::model::Tool,
    ) -> Result<Option<Arc<Admission>>, String> {
        let admission = self.get(route);
        if let Some(admission) = &admission {
            admission.project(Arc::make_mut(&mut tool.input_schema))?;
        }
        Ok(admission)
    }
    pub(super) fn get(&self, route: &RouteContract) -> Option<Arc<Admission>> {
        self.entries.get(&route.operation_name).cloned()
    }
}
impl Admission {
    fn check(&self, value: &Value) -> Result<(), &'static str> {
        let mut pending = vec![(value, 1usize)];
        let mut bytes = 0usize;
        while let Some((value, depth)) = pending.pop() {
            if depth > self.policy.max_depth {
                return Err("Tool body exceeds admission bounds");
            }
            bytes = bytes.saturating_add(match value {
                Value::String(s) => s.len(),
                Value::Object(map) => map.keys().map(String::len).sum(),
                _ => 1,
            });
            if bytes > self.policy.max_bytes {
                return Err("Tool body exceeds admission bounds");
            }
            let children = match value {
                Value::Array(values) => values.len(),
                Value::Object(values) => values.len(),
                _ => 0,
            };
            if pending.len().saturating_add(children) > self.policy.max_bytes {
                return Err("Tool body exceeds admission bounds");
            }
            match value {
                Value::Array(values) => {
                    pending.extend(values.iter().map(|value| (value, depth + 1)));
                }
                Value::Object(values) => {
                    pending.extend(values.values().map(|value| (value, depth + 1)));
                }
                _ => {}
            }
        }
        if serde_json::to_vec(value)
            .map_err(|_| "Invalid tool body")?
            .len()
            > self.policy.max_bytes
        {
            return Err("Tool body exceeds admission bounds");
        }
        if value.is_null() && !self.policy.allow_null {
            return Err("Invalid tool body");
        }
        if self
            .route
            .request
            .body
            .as_ref()
            .is_some_and(|body| body.kind == BodyKind::Text)
            && !value.is_string()
            && !value.is_null()
        {
            return Err("Invalid text tool body");
        }
        (self.validate)(value).map_err(|_| "Invalid tool body")
    }
    pub(super) fn apply(&self, args: &mut Map<String, Value>) -> Result<bool, &'static str> {
        let value = match args.get("body") {
            Some(value) => value,
            None => match &self.policy.missing {
                MissingToolBody::Reject => return Err("Missing tool body"),
                MissingToolBody::Omit => return Ok(true),
                MissingToolBody::Default(value) => value,
            },
        };
        self.check(value)?;
        // Conversion is transport encoding, after null was admitted as null.
        let encoded = if value.is_null()
            && self
                .route
                .request
                .body
                .as_ref()
                .is_some_and(|body| body.kind == BodyKind::Text)
        {
            Value::String(String::new())
        } else {
            value.clone()
        };
        args.insert("body".into(), encoded);
        Ok(false)
    }
    pub(super) fn project(&self, input: &mut Map<String, Value>) -> Result<(), String> {
        let mut schema = self.schema.clone();
        if let MissingToolBody::Default(value) = &self.policy.missing {
            if !schema.is_object() {
                schema = json!({"allOf":[schema]});
            }
            schema
                .as_object_mut()
                .ok_or("Invalid body schema")?
                .insert("default".into(), value.clone());
        }
        if !matches!(self.policy.missing, MissingToolBody::Reject)
            && let Some(required) = input.get_mut("required").and_then(Value::as_array_mut)
        {
            required.retain(|value| value.as_str() != Some("body"));
        }
        let relocated =
            forklaunch_contract::relocate_schema_json(&schema.to_string(), &["properties", "body"]);
        input
            .get_mut("properties")
            .and_then(Value::as_object_mut)
            .ok_or("Invalid tool schema")?
            .insert(
                "body".into(),
                serde_json::from_str(&relocated).map_err(|_| "Invalid body schema")?,
            );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forklaunch_contract::{BodyContract, SchemaRef};
    fn route() -> RouteContract {
        let mut route = RouteContract::post("admit", "admit", "/admit");
        route.request.body = Some(BodyContract::json("Input"));
        route.request.body.as_mut().unwrap().schema=SchemaRef::named("Input").with_json_schema(r#"{"type":"object","required":["id"],"properties":{"id":{"type":"string"}},"additionalProperties":false}"#);
        route
    }
    #[test]
    fn admission_is_atomic_bounded_and_pinned() {
        let route = route();
        let mut registry = McpBodyAdmissions::default();
        let policy = McpBodyAdmission::new(
            MissingToolBody::Default(json!({"id":"default"})),
            false,
            64,
            4,
        );
        registry.insert(&route, policy.clone()).unwrap();
        assert!(registry.insert(&route, policy).is_err());
        assert!(registry.validate_routes(&[]).is_err());
        let mut changed = route.clone();
        changed.path = "/changed".into();
        assert!(registry.validate_routes(&[changed]).is_err());
        let admission = registry.get(&route).unwrap();
        for value in [
            json!({"body":null}),
            json!({"body":{"id":1}}),
            json!({"body":{"id":"ok","unknown":true}}),
            json!({"body":{"id":"x".repeat(70)}}),
            json!({"body":[[[[[[]]]]]]}),
        ] {
            let mut args = value.as_object().unwrap().clone();
            let before = args.clone();
            assert!(admission.apply(&mut args).is_err());
            assert_eq!(args, before);
        }
        let mut args = Map::new();
        admission.apply(&mut args).unwrap();
        assert_eq!(args["body"], json!({"id":"default"}));
        args.insert("headers".into(), json!({"Content-Length":"999"}));
        assert!(super::super::build_request(&route, &args, None).is_err());
        let mut input: Map<String, Value> =
            serde_json::from_str(&crate::input_schema(&route)).unwrap();
        admission.project(&mut input).unwrap();
        assert!(
            !input["required"]
                .as_array()
                .unwrap()
                .contains(&json!("body"))
        );
        assert_eq!(
            input["properties"]["body"]["default"],
            json!({"id":"default"})
        );
    }
    #[test]
    fn omission_is_trusted_absence_and_keeps_transport_headers_private() {
        use forklaunch_contract::HttpMethod;
        for method in [HttpMethod::Post, HttpMethod::Delete] {
            let mut route = route();
            route.method = method;
            let mut registry = McpBodyAdmissions::default();
            registry
                .insert(
                    &route,
                    McpBodyAdmission::new(MissingToolBody::Omit, false, 64, 4),
                )
                .unwrap();
            let admission = registry.get(&route).unwrap();
            let mut args = Map::new();
            assert!(admission.apply(&mut args).unwrap());
            assert!(args.is_empty());
            assert!(super::super::build_request(&route, &args, None).is_err());
            let request =
                super::super::build_request_with_body_omission(&route, &args, None, true).unwrap();
            assert_eq!(
                request
                    .headers()
                    .get("content-length")
                    .map(|v| v.to_str().unwrap()),
                (method == HttpMethod::Post).then_some("0")
            );
            assert_eq!(request.headers()["content-type"], "application/json");
            for header in ["Content-Length", "Transfer-Encoding"] {
                let args = json!({"headers":{header:"invalid"}})
                    .as_object()
                    .unwrap()
                    .clone();
                assert!(
                    super::super::build_request_with_body_omission(&route, &args, None, true)
                        .is_err()
                );
            }
            args.insert("contentType".into(), json!("invalid"));
            assert!(
                super::super::build_request_with_body_omission(&route, &args, None, true).is_err()
            );
            args.remove("contentType");
            args.insert("body".into(), Value::Null);
            assert!(admission.apply(&mut args).is_err());
            args.insert("body".into(), json!({"id":"ok"}));
            assert!(!admission.apply(&mut args).unwrap());
        }
    }
    #[test]
    fn invalid_defaults_nullable_policies_and_bounds_reject_before_mount() {
        for policy in [
            McpBodyAdmission::new(MissingToolBody::Reject, true, 64, 4),
            McpBodyAdmission::new(MissingToolBody::Default(json!({})), false, 64, 4),
            McpBodyAdmission::new(MissingToolBody::Reject, false, 0, 4),
            McpBodyAdmission::new(MissingToolBody::Reject, false, 64, 65),
        ] {
            assert!(
                McpBodyAdmissions::default()
                    .insert(&route(), policy)
                    .is_err()
            );
        }
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    use forklaunch_contract::{BodyContract, SchemaRef};
    #[test]
    fn projected_schemas_preserve_local_refs_and_exact_null_requiredness() {
        for nullable in [false, true] {
            let mut route = RouteContract::post("Projection", "Projection", "/projection");
            route.request.body = Some(BodyContract::json("Input"));
            route.request.body.as_mut().unwrap().schema=SchemaRef::named("Input").with_json_schema(r##"{"definitions":{"Input":{"type":["string","null"]}},"$ref":"#/definitions/Input","default":"ignored"}"##);
            for missing in [
                MissingToolBody::Reject,
                MissingToolBody::Omit,
                MissingToolBody::Default(json!("trusted")),
            ] {
                let required = matches!(missing, MissingToolBody::Reject);
                let has_default = matches!(missing, MissingToolBody::Default(_));
                let mut registry = McpBodyAdmissions::default();
                registry
                    .insert(&route, McpBodyAdmission::new(missing, nullable, 64, 4))
                    .unwrap();
                let admission = registry.get(&route).unwrap();
                let mut input: Map<String, Value> =
                    serde_json::from_str(&crate::input_schema(&route)).unwrap();
                admission.project(&mut input).unwrap();
                let validate = schema_validation::compile(&Value::Object(input.clone())).unwrap();
                assert_eq!(validate(&json!({})).is_err(), required);
                assert_eq!(validate(&json!({"body":null})).is_ok(), nullable);
                assert!(validate(&json!({"body":"ok"})).is_ok());
                assert!(validate(&json!({"body":1})).is_err());
                assert_eq!(
                    input["properties"]["body"].get("default"),
                    has_default.then_some(&json!("trusted"))
                );
            }
        }
    }
}
