//! Offline resource identity and lexical-scope index for SDK schema imports.
use super::{ExpansionBudget, queue_anchor};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::BTreeMap;
use url::Url;

const ROOT: &str = "https://forklaunch.invalid/__document__";

pub(super) struct Resources {
    scopes: BTreeMap<usize, String>,
    default_base: String,
    has_ids: bool,
    roots: BTreeMap<String, String>,
    anchors: BTreeMap<String, String>,
}
impl Resources {
    #[allow(clippy::too_many_lines)]
    pub(super) fn index(document: &Value, budget: &mut ExpansionBudget) -> Result<Self> {
        let dialect = document
            .get("$schema")
            .and_then(Value::as_str)
            .unwrap_or("");
        let legacy = ["draft-04", "draft-06", "draft-07"]
            .iter()
            .any(|value| dialect.contains(value));
        let draft4 = dialect.contains("draft-04");
        let draft6 = dialect.contains("draft-06");
        let mut result = Self {
            scopes: BTreeMap::new(),
            default_base: ROOT.into(),
            has_ids: false,
            roots: BTreeMap::from([(ROOT.into(), String::new())]),
            anchors: BTreeMap::new(),
        };
        let mut pending = vec![(
            document,
            String::new(),
            document.get("openapi").is_none(),
            0,
        )];
        let mut parents = BTreeMap::from([(String::new(), ROOT.to_owned())]);
        while let Some((value, pointer, schema, depth)) = pending.pop() {
            if depth > 64 || budget.nodes == 0 {
                bail!("SDK schema resource index exceeds expansion limits");
            }
            budget.nodes -= 1;
            let mut base = parents
                .remove(&pointer)
                .context("Missing schema resource scope")?;
            if schema && value.is_boolean() {
                result
                    .scopes
                    .insert(std::ptr::from_ref(value) as usize, base.clone());
            }
            if let Some(object) = value.as_object() {
                if schema {
                    if !pointer.is_empty()
                        && let Some(nested) = object.get("$schema").and_then(Value::as_str)
                        && nested != dialect
                    {
                        bail!("Mixed schema resource dialects are unsupported by SDK generation");
                    }
                    if !draft4 && let Some(id) = object.get("$id") {
                        result.has_ids = true;
                        let id = id.as_str().context("Schema resource ID must be a string")?;
                        budget.bytes = budget
                            .bytes
                            .checked_sub(id.len())
                            .context("SDK schema resource index exceeds 32 MiB")?;
                        let uri = resolve(&base, id)?;
                        if uri.fragment().is_some_and(|fragment| !fragment.is_empty()) {
                            bail!("Schema resource IDs with nonempty fragments are unsupported");
                        }
                        let mut uri = uri;
                        uri.set_fragment(None);
                        base = uri.into();
                        if let Some(previous) = result.roots.insert(base.clone(), pointer.clone())
                            && previous != pointer
                        {
                            bail!("Duplicate schema resource ID");
                        }
                    }
                    if pointer.is_empty() {
                        result.default_base.clone_from(&base);
                    }
                    result
                        .scopes
                        .insert(std::ptr::from_ref(value) as usize, base.clone());
                    if let Some(anchor) = object.get("$anchor") {
                        if legacy {
                            bail!("$anchor is unavailable in the selected schema dialect");
                        }
                        let anchor = anchor.as_str().context("Schema anchor must be a string")?;
                        let mut bytes = anchor.bytes();
                        if !bytes
                            .next()
                            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                            || !bytes.all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
                        {
                            bail!("Invalid schema anchor name");
                        }
                        budget.bytes = budget
                            .bytes
                            .checked_sub(base.len() + anchor.len() + pointer.len())
                            .context("SDK schema resource index exceeds 32 MiB")?;
                        let key = format!("{base}#{anchor}");
                        if result.anchors.insert(key, pointer.clone()).is_some() {
                            bail!("Duplicate schema anchor");
                        }
                    }
                }
                for (key, child) in object {
                    let path = format!("{pointer}/{}", escape(key));
                    let mut enqueue = |value, path: String, schema| -> Result<()> {
                        budget.bytes = budget
                            .bytes
                            .checked_sub(base.len() + path.len())
                            .context("SDK schema resource index exceeds 32 MiB")?;
                        parents.insert(path.clone(), base.clone());
                        queue_anchor(&mut pending, budget, (value, path, schema, depth + 1))
                    };
                    if schema {
                        if (draft4 && matches!(key.as_str(), "contains" | "propertyNames"))
                            || ((draft4 || draft6)
                                && matches!(
                                    key.as_str(),
                                    "if" | "then"
                                        | "else"
                                        | "dependentSchemas"
                                        | "prefixItems"
                                        | "unevaluatedProperties"
                                        | "unevaluatedItems"
                                        | "contentSchema"
                                ))
                        {
                            continue;
                        }
                        match key.as_str() {
                            "$defs" | "definitions" | "properties" | "patternProperties"
                            | "dependentSchemas" => {
                                if let Some(map) = child.as_object() {
                                    for (name, value) in map {
                                        enqueue(value, format!("{path}/{}", escape(name)), true)?;
                                    }
                                }
                            }
                            "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                                if let Some(values) = child.as_array() {
                                    for (index, value) in values.iter().enumerate() {
                                        enqueue(value, format!("{path}/{index}"), true)?;
                                    }
                                }
                            }
                            "items"
                            | "additionalProperties"
                            | "additionalItems"
                            | "contains"
                            | "not"
                            | "if"
                            | "then"
                            | "else"
                            | "propertyNames"
                            | "unevaluatedProperties"
                            | "unevaluatedItems"
                            | "contentSchema" => enqueue(child, path, true)?,
                            _ => {}
                        }
                    } else if pointer == "/components/schemas" || key == "schema" {
                        enqueue(child, path, true)?;
                    } else if !key.starts_with("x-")
                        && !matches!(
                            key.as_str(),
                            "example" | "examples" | "default" | "enum" | "const"
                        )
                    {
                        enqueue(child, path, false)?;
                    }
                }
            } else if !schema && let Some(values) = value.as_array() {
                for (index, value) in values.iter().enumerate() {
                    let path = format!("{pointer}/{index}");
                    budget.bytes = budget
                        .bytes
                        .checked_sub(base.len() + path.len())
                        .context("SDK schema resource index exceeds 32 MiB")?;
                    parents.insert(path.clone(), base.clone());
                    queue_anchor(&mut pending, budget, (value, path, false, depth + 1))?;
                }
            }
        }
        Ok(result)
    }

    pub(super) fn check_target(&self, target: &Value) -> Result<()> {
        if (self.has_ids || target.get("$id").is_some())
            && !self
                .scopes
                .contains_key(&(std::ptr::from_ref(target) as usize))
        {
            bail!(
                "Resource-aware references to literal data or schema containers require explicit schema definitions"
            );
        }
        Ok(())
    }

    pub(super) fn pointer(&self, schema: &Value, reference: &str) -> Result<String> {
        let base = self
            .scopes
            .get(&(std::ptr::from_ref(schema) as usize))
            .map_or(self.default_base.as_str(), String::as_str);
        let mut uri = resolve(base, reference)?;
        let fragment = uri.fragment().unwrap_or("").to_owned();
        uri.set_fragment(None);
        let root = self.roots.get(uri.as_str()).context("Schema resource is not bundled in the document; SDK generation does not fetch remote schemas")?;
        if fragment.contains("%2F") || fragment.contains("%2f") {
            bail!(
                "Checked schemas require literal JSON-pointer separators; escape slash tokens with ~1"
            );
        }
        if let Some(pointer) = super::document_pointer(&format!("#{fragment}")) {
            return Ok(format!("{root}{pointer}"));
        }
        let fragment = percent_encoding::percent_decode_str(&fragment)
            .decode_utf8()
            .context("Schema reference fragment must be UTF-8")?;
        if fragment.is_empty() || fragment.starts_with('/') {
            Ok(format!("{root}{fragment}"))
        } else {
            self.anchors
                .get(&format!("{uri}#{fragment}"))
                .cloned()
                .context("Missing schema resource anchor")
        }
    }
}
fn escape(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
fn resolve(base: &str, reference: &str) -> Result<Url> {
    if reference
        .chars()
        .any(|c| c.is_control() || c.is_whitespace())
        || reference.contains('\\')
    {
        bail!("Invalid schema resource URI");
    }
    Url::parse(base)?
        .join(reference)
        .context("Invalid schema resource URI")
}

#[cfg(test)]
mod tests {
    use super::super::{DEFINITIONS, ExpansionBudget, REFERENCE, bundle, normalize};
    use serde_json::json;

    #[test]
    fn resource_roots_and_relative_ids_preserve_recursive_identity() {
        let schema = json!({"$id":"https://schemas.test/root.json","type":"object","properties":{
            "child":{"$ref":"nodes/node.json"},"alias":{"$ref":"https://schemas.test/nodes/node.json"}
        },"$defs":{"Node":{"$id":"nodes/node.json","type":"object","properties":{
            "value":{"type":"string"},"next":{"$ref":"#"},"root":{"$ref":"../root.json"}
        }}}});
        let result = normalize(&schema, "Resource").unwrap();
        assert_eq!(
            result["properties"]["child"][REFERENCE],
            result["properties"]["alias"][REFERENCE]
        );
        let name = result["properties"]["child"][REFERENCE].as_str().unwrap();
        assert_eq!(
            result[DEFINITIONS][name]["properties"]["next"][REFERENCE],
            name
        );
        assert_eq!(
            result[DEFINITIONS][name]["properties"]["value"]["type"],
            "string"
        );
    }

    #[test]
    fn anchors_are_scoped_to_their_resource_and_literals_are_preserved() {
        let schema = json!({"$id":"https://schemas.test/root","type":"object","properties":{
            "a":{"$ref":"a#Value"},"b":{"$ref":"b#Value"}
        },"$defs":{
            "A":{"$id":"a","$anchor":"Value","type":"string"},
            "B":{"$id":"b","$anchor":"Value","type":"integer"}
        },"default":{"$id":"a","$anchor":"Value","$ref":"not-a-schema"}});
        let result = normalize(&schema, "Scopes").unwrap();
        assert_eq!(result["properties"]["a"]["type"], "string");
        assert_eq!(result["properties"]["b"]["type"], "integer");
        assert_eq!(result["default"], schema["default"]);
    }

    #[test]
    fn openapi_component_resource_pointers_resolve_in_the_component() {
        let document = json!({"openapi":"3.1.0","components":{"schemas":{
            "Node":{"$id":"https://schemas.test/node","type":"object","properties":{
                "value":{"$ref":"#/$defs/Value"},"next":{"$ref":"https://schemas.test/node"}
            },"$defs":{"Value":{"type":"string"}}}
        }}});
        let result = bundle(
            &document["components"]["schemas"]["Node"],
            &document,
            &mut ExpansionBudget::default(),
        )
        .unwrap();
        assert_eq!(result["properties"]["value"]["type"], "string");
        assert!(result["properties"]["next"]["$ref"].is_string());
    }

    #[test]
    fn malformed_duplicate_missing_and_dynamic_resources_fail_closed() {
        for schema in [
            json!({"$id":12,"type":"string"}),
            json!({"$id":"https://schemas.test/has space","type":"string"}),
            json!({"$id":"https://schemas.test/root#anchor","type":"string"}),
            json!({"$id":"https://schemas.test/root","$ref":"https://remote.test/missing"}),
            json!({"$id":"https://schemas.test/root","$ref":"same","$defs":{"A":{"$id":"same","type":"string"},"B":{"$id":"same","type":"number"}}}),
            json!({"$id":"https://schemas.test/root","$dynamicRef":"#Node","$dynamicAnchor":"Node"}),
            json!({"$id":"https://schemas.test/root","$ref":"#Missing"}),
        ] {
            assert!(normalize(&schema, "Rejected").is_err(), "{schema}");
        }
    }

    #[test]
    fn non_hierarchical_ids_and_literal_target_boundaries() {
        let schema =
            json!({"$id":"urn:fixture:node","type":"object","properties":{"next":{"$ref":"#"}}});
        let result = normalize(&schema, "Urn").unwrap();
        assert_eq!(result["properties"]["next"][REFERENCE], "UrnRef1");
        let schema = json!({"$id":"https://schemas.test/root","$ref":"#/default","default":{"$id":"nested","type":"string"}});
        assert!(normalize(&schema, "Literal").is_err());
        let external = json!({"$id":"https://schemas.test/external","type":"string"});
        assert!(bundle(&external, &json!({}), &mut ExpansionBudget::default()).is_err());
    }

    #[test]
    fn resource_indexes_share_node_and_byte_limits() {
        let schema = json!({"$id":"https://schemas.test/root","$ref":"child","$defs":{"Child":{"$id":"child","type":"string"}}});
        for mut budget in [
            ExpansionBudget {
                nodes: 2,
                bytes: 1024,
            },
            ExpansionBudget {
                nodes: 1000,
                bytes: 10,
            },
        ] {
            assert!(bundle(&schema, &schema, &mut budget).is_err());
        }
    }
}
