//! Import the JSON subset supported by the generated HTTP clients. Resolve
//! document-local references without fetching files or remote URLs.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Context, Result};
use convert_case::{Case, Casing};
use serde_json::{json, Value};

pub(super) fn contract_from_openapi(document: &Value) -> Result<Value> {
    let version = document
        .get("openapi")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !version.starts_with("3.0.") && !version.starts_with("3.1.") {
        bail!("SDK generation requires OpenAPI 3.0 or 3.1");
    }
    let paths = document
        .get("paths")
        .and_then(Value::as_object)
        .context("OpenAPI paths must be an object")?;
    let mut budget = ExpansionBudget::default();
    let mut names = BTreeSet::new();
    let mut routes = Vec::new();
    for (path, item) in paths {
        if path.starts_with("x-") {
            continue;
        }
        if !path.starts_with('/') {
            bail!("OpenAPI operation path must start with /");
        }
        let item = resolve(item, document, 0, &mut budget)?;
        let item = item
            .as_object()
            .context("OpenAPI path item must be an object")?;
        let mut operations: Vec<(&str, &Value)> = [
            "get", "put", "post", "delete", "options", "head", "patch", "trace",
        ]
        .into_iter()
        .filter_map(|method| item.get(method).map(|value| (method, value)))
        .collect();
        if let Some(extended) = item.get("x-forklaunch-http-methods") {
            for (method, operation) in extended
                .as_object()
                .context("x-forklaunch-http-methods must be an object")?
            {
                if !matches!(
                    method.as_str(),
                    "CHECKOUT"
                        | "COPY"
                        | "LINK"
                        | "LOCK"
                        | "M-SEARCH"
                        | "MERGE"
                        | "MKACTIVITY"
                        | "MKCOL"
                        | "MOVE"
                        | "NOTIFY"
                        | "PROPFIND"
                        | "PROPPATCH"
                        | "PURGE"
                        | "REPORT"
                        | "SEARCH"
                        | "SUBSCRIBE"
                        | "UNLINK"
                        | "UNLOCK"
                        | "UNSUBSCRIBE"
                ) {
                    bail!("Unsupported extended OpenAPI HTTP method: {method}");
                }
                operations.push((method.as_str(), operation));
            }
        }
        for (method, operation) in operations {
            let operation = operation
                .as_object()
                .context("OpenAPI operation must be an object")?;
            let mut parameters = BTreeMap::new();
            for list in [item.get("parameters"), operation.get("parameters")] {
                if let Some(list) = list {
                    for parameter in list
                        .as_array()
                        .context("OpenAPI parameters must be an array")?
                    {
                        let name = parameter
                            .get("name")
                            .and_then(Value::as_str)
                            .context("OpenAPI parameter name is required")?;
                        let location = parameter
                            .get("in")
                            .and_then(Value::as_str)
                            .context("OpenAPI parameter location is required")?;
                        parameters.insert((location, name), parameter);
                    }
                }
            }
            let mut request = json!({"params": [], "query": [], "headers": []});
            for ((location, name), parameter) in parameters {
                let field = match location {
                    "path" => "params",
                    "query" => "query",
                    "header" => "headers",
                    _ => bail!("Unsupported OpenAPI parameter location: {location}"),
                };
                let schema = parameter
                    .get("schema")
                    .context("OpenAPI parameter schema is required")?;
                request[field].as_array_mut().unwrap().push(json!({
                    "name": name, "required": location == "path" || parameter.get("required") == Some(&Value::Bool(true)),
                    "schema": {"jsonSchema": schema}
                }));
            }
            if let Some(body) = operation.get("requestBody") {
                let mut imported = response_body(body)?;
                if imported["contentType"].as_str().is_some_and(|media| {
                    media.split(';').next().unwrap_or_default().trim()
                        == "application/x-www-form-urlencoded"
                }) {
                    imported["kind"] = Value::String("urlEncoded".into());
                }
                request["body"] = imported;
            }
            let responses = operation
                .get("responses")
                .and_then(Value::as_object)
                .context("OpenAPI responses must be an object")?;
            let mut success_responses = Vec::new();
            for (code, response) in responses {
                let Ok(status) = code.parse::<u16>() else {
                    if code == "default" || code.starts_with("x-") {
                        continue;
                    }
                    bail!("SDK generation requires explicit numeric OpenAPI response statuses");
                };
                if !(100..600).contains(&status) {
                    bail!("Invalid OpenAPI response status {status}");
                }
                if status < 200 {
                    continue;
                }
                let body = if matches!(status, 204 | 205)
                    || method == "head"
                    || response.get("content").is_none()
                    || response
                        .get("content")
                        .and_then(Value::as_object)
                        .is_some_and(serde_json::Map::is_empty)
                {
                    Value::Null
                } else {
                    response_body(response)?
                };
                success_responses.push(json!({"statusCode":status, "body":body}));
            }
            let mut route = json!({"method": method.to_ascii_uppercase(), "path": path, "request": request,
                "responses": success_responses});
            let operation_name = match operation.get("operationId") {
                Some(Value::String(name)) if !name.trim().is_empty() => name.clone(),
                Some(Value::String(_)) | None => {
                    format!(
                        "{}{}",
                        method,
                        path.chars()
                            .map(|character| if character.is_ascii_alphanumeric() {
                                character
                            } else {
                                ' '
                            })
                            .collect::<String>()
                            .to_case(Case::Pascal)
                    )
                }
                _ => bail!("OpenAPI operationId must be a string"),
            };
            let router = path
                .trim_matches('/')
                .split('/')
                .next()
                .filter(|segment| !segment.is_empty())
                .unwrap_or("health")
                .to_case(Case::Camel);
            let method_name = operation_name.to_case(Case::Camel);
            for name in [&router, &method_name] {
                if !name.starts_with(|character: char| character.is_ascii_alphabetic())
                    || !name
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '_')
                {
                    bail!(
                        "OpenAPI operation/path cannot map to a supported SDK identifier: {name}"
                    );
                }
            }
            if !names.insert((router, method_name)) {
                bail!("OpenAPI operations produce duplicate SDK method names: {operation_name}");
            }
            route["operationName"] = Value::String(operation_name);
            routes.push(route);
        }
    }
    Ok(json!({"routes": routes}))
}

fn response_body(response: &Value) -> Result<Value> {
    let content = response
        .get("content")
        .and_then(Value::as_object)
        .context("OpenAPI response content must be an object")?;
    if content.len() != 1 {
        bail!("SDK generation currently requires one response media type per status");
    }
    let (media_type, media) = content.iter().next().unwrap();
    let media_type = media_type.to_ascii_lowercase();
    let kind = if media_type == "application/json" || media_type.ends_with("+json") {
        "json"
    } else if media_type == "text/event-stream" || media_type.starts_with("multipart/") {
        bail!("Streaming/multipart SDK response transport remains unsupported");
    } else if media_type.starts_with("text/") {
        "text"
    } else {
        "binary"
    };
    let schema = media.get("schema").cloned().unwrap_or_else(|| json!({}));
    Ok(json!({"kind":kind, "contentType":media_type, "schema":{"jsonSchema":schema}}))
}

struct ExpansionBudget {
    nodes: usize,
    bytes: usize,
}
impl Default for ExpansionBudget {
    fn default() -> Self {
        Self {
            nodes: 100_000,
            bytes: 32 * 1024 * 1024,
        }
    }
}

fn clone_literal(value: &Value, budget: &mut ExpansionBudget) -> Result<Value> {
    let size = serde_json::to_vec(value)?.len();
    budget.bytes = budget
        .bytes
        .checked_sub(size)
        .context("OpenAPI expanded references exceed 32 MiB")?;
    Ok(value.clone())
}

fn resolve(
    value: &Value,
    document: &Value,
    depth: usize,
    budget: &mut ExpansionBudget,
) -> Result<Value> {
    if depth > 64 || budget.nodes == 0 {
        bail!("OpenAPI references exceed expansion limits or contain a cycle");
    }
    budget.nodes -= 1;
    let size = match value {
        Value::String(text) => text.len(),
        Value::Object(object) => object.keys().map(String::len).sum(),
        _ => 8,
    };
    budget.bytes = budget
        .bytes
        .checked_sub(size)
        .context("OpenAPI expanded references exceed 32 MiB")?;
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref") {
                let reference = reference
                    .as_str()
                    .context("OpenAPI $ref must be a string")?;
                let pointer = reference
                    .strip_prefix('#')
                    .filter(|pointer| pointer.starts_with('/'))
                    .context("Only document-local OpenAPI references are supported")?;
                let target = document
                    .pointer(pointer)
                    .with_context(|| format!("Missing OpenAPI reference {reference}"))?;
                let mut resolved = resolve(target, document, depth + 1, budget)?;
                for (key, value) in object {
                    if key == "$ref" {
                        continue;
                    }
                    if !matches!(key.as_str(), "description" | "summary" | "title")
                        && !key.starts_with("x-")
                    {
                        bail!("Unsupported validation sibling of OpenAPI $ref: {key}");
                    }
                    resolved
                        .as_object_mut()
                        .context("OpenAPI reference with annotations must resolve to an object")?
                        .insert(key.clone(), clone_literal(value, budget)?);
                }
                Ok(resolved)
            } else {
                let mut resolved = serde_json::Map::new();
                for (key, value) in object {
                    // A property can itself be named "$ref". Named-schema maps
                    // are containers, not reference objects.
                    let value = if key.starts_with("x-")
                        || matches!(
                            key.as_str(),
                            "example" | "examples" | "default" | "const" | "enum"
                        ) {
                        clone_literal(value, budget)?
                    } else if matches!(
                        key.as_str(),
                        "properties" | "patternProperties" | "$defs" | "dependentSchemas"
                    ) {
                        if let Some(properties) = value.as_object() {
                            let mut schemas = serde_json::Map::new();
                            for (name, schema) in properties {
                                budget.bytes = budget
                                    .bytes
                                    .checked_sub(name.len())
                                    .context("OpenAPI expanded references exceed 32 MiB")?;
                                schemas.insert(
                                    name.clone(),
                                    resolve(schema, document, depth + 1, budget)?,
                                );
                            }
                            Value::Object(schemas)
                        } else {
                            bail!("OpenAPI named schema collection must be an object");
                        }
                    } else {
                        resolve(value, document, depth + 1, budget)?
                    };
                    resolved.insert(key.clone(), value);
                }
                Ok(Value::Object(resolved))
            }
        }
        Value::Array(array) => array
            .iter()
            .map(|value| resolve(value, document, depth + 1, budget))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        _ => Ok(value.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_real_operations_and_references_with_parameter_overrides() {
        let spec = json!({"openapi":"3.1.0", "paths":{"/users/{id}": {
            "parameters":[{"in":"path", "name":"id", "schema":{"type":"string"}}, {"in":"query", "name":"dryRun", "schema":{"type":"string"}}],
            "post":{"operationId":"saveUser", "parameters":[{"in":"query","name":"dryRun","schema":{"type":"boolean"}}],
            "requestBody":{"content":{"application/json":{"schema":{"$ref":"#/components/schemas/User"}}}},
            "responses":{"201":{"content":{"application/json":{"schema":{"$ref":"#/components/schemas/User"}}}}}}
        }}, "components":{"schemas":{"User":{"type":"object", "properties":{"displayName":{"type":"string"}}}}}});
        let contract = contract_from_openapi(&spec).unwrap();
        let route = &contract["routes"][0];
        assert_eq!(route["operationName"], "saveUser");
        assert_eq!(route["method"], "POST");
        assert_eq!(route["request"]["query"].as_array().unwrap().len(), 1);
        assert_eq!(route["request"]["params"][0]["required"], true);
        assert_eq!(
            route["request"]["query"][0]["schema"]["jsonSchema"]["type"],
            "boolean"
        );
        assert_eq!(
            route["responses"][0]["body"]["schema"]["jsonSchema"]["properties"]["displayName"]
                ["type"],
            "string"
        );
    }
    #[test]
    fn rejects_external_missing_and_recursive_references() {
        for reference in [
            "https://example.invalid/schema",
            "other.json#/User",
            "#/missing",
            "#/paths/~1users",
        ] {
            let spec = json!({"openapi":"3.1.0", "paths":{"/users":{"$ref":reference}}});
            assert!(contract_from_openapi(&spec).is_err(), "{reference}");
        }
        assert_eq!(
            contract_from_openapi(&json!({"openapi":"3.1.0", "paths":{}})).unwrap()["routes"],
            json!([])
        );
    }
}

#[cfg(test)]
mod schema_reference_tests {
    use super::*;
    #[test]
    fn property_names_and_constant_payloads_are_not_reference_objects() {
        let schema = json!({"type":"object", "properties":{"$ref":{"type":"string"}},
            "const":{"$ref":"https://example.invalid/arbitrary-data"}});
        assert_eq!(
            resolve(&schema, &json!({}), 0, &mut ExpansionBudget::default()).unwrap(),
            schema
        );
    }
    #[test]
    fn expansion_budget_and_validation_siblings_are_rejected() {
        let document = json!({"schema":{"type":"string"}});
        assert!(resolve(
            &json!({"$ref":"#/schema", "minLength":5}),
            &document,
            0,
            &mut ExpansionBudget::default()
        )
        .is_err());
        assert!(resolve(
            &json!([1, 2, 3]),
            &document,
            0,
            &mut ExpansionBudget {
                nodes: 2,
                bytes: 1024
            }
        )
        .is_err());
    }
}

#[cfg(test)]
mod operation_name_tests {
    use super::*;
    #[test]
    fn missing_ids_are_deterministic_and_normalized_collisions_fail() {
        let response = json!({"200":{"content":{"application/json":{"schema":{"type":"string"}}}}});
        let mut spec = json!({"openapi":"3.1.0", "paths":{
            "/users/{id}":{"get":{"responses":response}},
            "/users/search":{"get":{"responses":response}}
        }});
        let contract = contract_from_openapi(&spec).unwrap();
        assert!(contract["routes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|route| route["operationName"] == "getUsersId"));
        spec["paths"]["/users/{id}"]["get"]["operationId"] = json!("find-user");
        spec["paths"]["/users/search"]["get"]["operationId"] = json!("findUser");
        assert!(contract_from_openapi(&spec).is_err());
    }
    #[test]
    fn literal_reference_annotations_cannot_bypass_the_byte_budget() {
        let schema = json!({"const": "x".repeat(1000)});
        assert!(resolve(
            &schema,
            &json!({}),
            0,
            &mut ExpansionBudget {
                nodes: 100,
                bytes: 100
            }
        )
        .is_err());
    }
}

#[cfg(test)]
mod empty_response_tests {
    use super::*;
    #[test]
    fn imports_empty_success_bodies_separately_from_json_null() {
        for (status, content, has_body) in [
            ("204", json!(null), false),
            ("200", json!({}), false),
            (
                "200",
                json!({"application/json":{"schema":{"type":"null"}}}),
                true,
            ),
        ] {
            let response = if content.is_null() {
                json!({"description":"No content"})
            } else {
                json!({"content":content})
            };
            let spec = json!({"openapi":"3.1.0", "paths":{"/item":{"get":{"responses":{status:response}}}}});
            let contract = contract_from_openapi(&spec).unwrap();
            assert_eq!(
                !contract["routes"][0]["responses"][0]["body"].is_null(),
                has_body
            );
        }
    }
}

#[cfg(test)]
mod multiple_response_tests {
    use super::*;
    #[test]
    fn imports_all_explicit_success_statuses_and_error_only_operations() {
        let spec = json!({"openapi":"3.1.0", "paths":{"/item":{"get":{"responses":{
            "200":{"content":{"application/json":{"schema":{"type":"string"}}}},
            "201":{"content":{"application/json":{"schema":{"type":"object","properties":{"createdAt":{"type":"string"}}}}}},
            "204":{"description":"No content"}
        }},"delete":{"responses":{"403":{"description":"Denied"}}}}}});
        let contract = contract_from_openapi(&spec).unwrap();
        assert_eq!(
            contract["routes"][0]["responses"].as_array().unwrap().len(),
            3
        );
        assert_eq!(contract["routes"][1]["responses"][0]["statusCode"], 403);
    }
}

#[cfg(test)]
mod request_media_tests {
    use super::*;
    #[test]
    fn imports_raw_text_and_binary_request_media() {
        for (media, kind) in [
            ("text/plain", "text"),
            ("application/octet-stream", "binary"),
            ("application/x-www-form-urlencoded", "urlEncoded"),
            ("application/vnd.example+json", "json"),
        ] {
            let spec = json!({"openapi":"3.1.0","paths":{"/upload":{"post":{"requestBody":{"required":true,"content":{media:{}}},"responses":{"204":{"description":"Done"}}}}}});
            let contract = contract_from_openapi(&spec).unwrap();
            assert_eq!(contract["routes"][0]["request"]["body"]["kind"], kind);
            assert_eq!(
                contract["routes"][0]["request"]["body"]["contentType"],
                media
            );
        }
    }
}

#[cfg(test)]
mod extended_method_tests {
    use super::*;
    #[test]
    fn imports_extended_methods_with_shared_path_parameters_and_rejects_unknown_tokens() {
        let methods = [
            "CHECKOUT",
            "COPY",
            "LINK",
            "LOCK",
            "M-SEARCH",
            "MERGE",
            "MKACTIVITY",
            "MKCOL",
            "MOVE",
            "NOTIFY",
            "PROPFIND",
            "PROPPATCH",
            "PURGE",
            "REPORT",
            "SEARCH",
            "SUBSCRIBE",
            "UNLINK",
            "UNLOCK",
            "UNSUBSCRIBE",
        ];
        let mut extended = serde_json::Map::new();
        for method in methods {
            extended.insert(method.into(), json!({"operationId":method.to_case(Case::Camel),"responses":{"204":{"description":"Done"}}}));
        }
        let mut document = json!({"openapi":"3.1.0","paths":{"/probe/{id}":{"parameters":[{"name":"id","in":"path","required":true,"schema":{"type":"string"}}],"get":{"operationId":"probe","responses":{"204":{"description":"Done"}}},"x-forklaunch-http-methods":extended}}});
        let contract = contract_from_openapi(&document).unwrap();
        let routes = contract["routes"].as_array().unwrap();
        assert_eq!(routes.len(), 20);
        for method in methods {
            let route = routes
                .iter()
                .find(|route| route["method"] == method)
                .unwrap();
            assert_eq!(route["request"]["params"][0]["name"], "id");
            assert_eq!(route["responses"][0]["body"], Value::Null);
        }
        document["paths"]["/probe/{id}"]["x-forklaunch-http-methods"]["BREW"] = json!({});
        assert!(contract_from_openapi(&document)
            .unwrap_err()
            .to_string()
            .contains("Unsupported extended"));
        document["paths"]["/probe/{id}"]["x-forklaunch-http-methods"] = json!([]);
        assert!(contract_from_openapi(&document)
            .unwrap_err()
            .to_string()
            .contains("must be an object"));
    }
}
