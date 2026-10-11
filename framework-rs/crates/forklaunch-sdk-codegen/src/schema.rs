//! Bounded, document-local schema graphs. References remain edges, including cycles.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};

pub(crate) struct ExpansionBudget {
    pub(crate) nodes: usize,
    pub(crate) bytes: usize,
}
impl Default for ExpansionBudget {
    fn default() -> Self {
        Self {
            nodes: 100_000,
            bytes: 32 * 1024 * 1024,
        }
    }
}

pub(crate) fn clone_literal(value: &Value, budget: &mut ExpansionBudget) -> Result<Value> {
    // Literal annotations are data, but must obey the same resource bounds
    // before recursive serialization/cloning runs on a caller-supplied Value.
    let mut pending = vec![(value, 0)];
    while let Some((value, depth)) = pending.pop() {
        if depth > 64 || budget.nodes == 0 {
            bail!("SDK schema literals exceed expansion limits");
        }
        budget.nodes -= 1;
        let children = match value {
            Value::Array(values) => values.len(),
            Value::Object(values) => values.len(),
            _ => 0,
        };
        if children > budget.nodes.saturating_sub(pending.len()) {
            bail!("SDK schema literals exceed expansion limits");
        }
        match value {
            Value::Array(values) => pending.extend(values.iter().map(|value| (value, depth + 1))),
            Value::Object(values) => {
                pending.extend(values.values().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
    }
    let mut counter = LiteralByteBudget {
        remaining: budget.bytes,
    };
    serde_json::to_writer(&mut counter, value).context("SDK schema graph exceeds 32 MiB")?;
    budget.bytes = counter.remaining;
    Ok(value.clone())
}

struct LiteralByteBudget {
    remaining: usize,
}
impl std::io::Write for LiteralByteBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other("SDK schema byte budget exceeded"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) const REFERENCE: &str = "x-forklaunch-sdk-reference";
pub(crate) const DEFINITIONS: &str = "x-forklaunch-sdk-definitions";

pub(crate) fn bundle(
    schema: &Value,
    document: &Value,
    budget: &mut ExpansionBudget,
) -> Result<Value> {
    let mut graph = Graph {
        document,
        budget,
        names: BTreeMap::new(),
        definitions: Map::new(),
    };
    let mut root = graph.visit(schema, 0)?;
    // TypeScript cannot represent a cycle consisting only of aliases/unions.
    // Object properties and array items provide the necessary structural guard.

    let mut done = BTreeSet::new();
    for name in graph.definitions.keys() {
        check_aliases(name, &graph.definitions, &mut Vec::new(), &mut done, 0)?;
    }

    let original = graph.definitions.clone();
    graph
        .definitions
        .retain(|_, schema| schema.get("$ref").is_none());
    for definition in graph.definitions.values_mut() {
        collapse(definition, &original)?;
    }
    collapse(&mut root, &original)?;

    let edges = graph
        .definitions
        .iter()
        .map(|(name, schema)| {
            let mut targets = BTreeSet::new();
            dependencies(schema, &mut targets);
            (name.clone(), targets)
        })
        .collect::<BTreeMap<_, _>>();

    let mut inline = BTreeMap::new();
    for name in edges.keys() {
        safe_inline(name, &edges, &mut inline, 0)?;
    }

    let definitions = graph.definitions.clone();
    graph.definitions.retain(|name, _| !inline[name]);
    for definition in graph.definitions.values_mut() {
        expand(definition, &definitions, &inline, graph.budget, 0)?;
    }
    expand(&mut root, &definitions, &inline, graph.budget, 0)?;
    if let Some(reference) = root.get("$ref").and_then(Value::as_str) {
        let mut expanded = graph.definitions[reference.strip_prefix("#/$defs/").unwrap()].clone();
        for (key, value) in root.as_object().unwrap() {
            if key != "$ref" {
                expanded[key] = value.clone();
            }
        }
        root = expanded;
    }
    if !graph.definitions.is_empty() {
        root.as_object_mut()
            .context("Referenced schema must be an object")?
            .insert("$defs".into(), Value::Object(graph.definitions));
    }
    Ok(root)
}

struct Graph<'a> {
    document: &'a Value,
    budget: &'a mut ExpansionBudget,
    names: BTreeMap<String, String>,
    definitions: Map<String, Value>,
}

impl Graph<'_> {
    #[allow(clippy::too_many_lines)]
    fn visit(&mut self, schema: &Value, depth: usize) -> Result<Value> {
        if depth > 64 || self.budget.nodes == 0 {
            bail!("SDK schema graph exceeds expansion limits");
        }
        self.budget.nodes -= 1;
        let Some(object) = schema.as_object() else {
            if !schema.is_boolean() {
                bail!("JSON schemas must be objects or booleans");
            }
            return clone_literal(schema, self.budget);
        };
        if object.contains_key("$id")
            || object.contains_key("$dynamicRef")
            || object.contains_key("$recursiveRef")
        {
            bail!("Schema resource IDs and dynamic references are unsupported by SDK generation");
        }
        let draft4 = self
            .document
            .get("$schema")
            .and_then(Value::as_str)
            .is_some_and(|dialect| dialect.contains("/draft-04/"));
        if draft4 && object.contains_key("id") {
            bail!("Schema resource IDs are unsupported by SDK generation");
        }
        if object.contains_key(REFERENCE) || object.contains_key(DEFINITIONS) {
            bail!("Reserved SDK schema annotation");
        }
        if let Some(reference) = object.get("$ref") {
            let reference = reference.as_str().context("Schema $ref must be a string")?;
            self.budget.bytes = self
                .budget
                .bytes
                .checked_sub(reference.len() + object.keys().map(String::len).sum::<usize>())
                .context("SDK schema graph exceeds 32 MiB")?;
            let pointer = reference
                .strip_prefix('#')
                .filter(|pointer| pointer.is_empty() || pointer.starts_with('/'))
                .context("Only document-local schema references are supported")?;
            let target = self
                .document
                .pointer(pointer)
                .with_context(|| format!("Missing schema reference {reference}"))?;
            let name = if let Some(name) = self.names.get(reference) {
                name.clone()
            } else {
                let name = format!("Ref{}", self.names.len() + 1);
                // Reserve before visiting to terminate self and mutual recursion.
                self.names.insert(reference.into(), name.clone());
                let resolved = self.visit(target, depth + 1)?;
                self.definitions.insert(name.clone(), resolved);
                name
            };
            let mut resolved = json!({"$ref": format!("#/$defs/{name}")});
            for (key, value) in object {
                if key == "$ref" {
                    continue;
                }
                if !matches!(
                    key.as_str(),
                    "description" | "summary" | "title" | "$schema" | "$defs" | "definitions"
                ) && !key.starts_with("x-")
                {
                    bail!("Unsupported validation sibling of schema $ref: {key}");
                }
                if key != "$defs" && key != "definitions" {
                    resolved[key] = clone_literal(value, self.budget)?;
                }
            }
            return Ok(resolved);
        }
        let mut result = Map::new();
        for (key, value) in object {
            // Draft 4 treats these later validation keywords as annotations.
            // Keep literal data intact; only schema positions pass through visit.
            if draft4
                && matches!(
                    key.as_str(),
                    "const"
                        | "contains"
                        | "propertyNames"
                        | "if"
                        | "then"
                        | "else"
                        | "dependentSchemas"
                        | "prefixItems"
                )
            {
                continue;
            }
            self.budget.bytes = self
                .budget
                .bytes
                .checked_sub(key.len())
                .context("SDK schema graph exceeds 32 MiB")?;
            let value = match key.as_str() {
                // Only reachable definitions participate in the graph.
                "$defs" | "definitions" => continue,
                "properties" | "patternProperties" | "dependentSchemas" => {
                    let mut properties = Map::new();
                    for (name, schema) in value
                        .as_object()
                        .context("Named schemas must be an object")?
                    {
                        self.budget.bytes = self
                            .budget
                            .bytes
                            .checked_sub(name.len())
                            .context("SDK schema graph exceeds 32 MiB")?;
                        properties.insert(name.clone(), self.visit(schema, depth + 1)?);
                    }
                    Value::Object(properties)
                }
                "items"
                | "additionalProperties"
                | "contains"
                | "not"
                | "if"
                | "then"
                | "else"
                | "propertyNames" => self.visit(value, depth + 1)?,
                "oneOf" | "anyOf" | "allOf" | "prefixItems" => Value::Array(
                    value
                        .as_array()
                        .context("Schema alternatives must be an array")?
                        .iter()
                        .map(|value| self.visit(value, depth + 1))
                        .collect::<Result<_>>()?,
                ),
                _ => clone_literal(value, self.budget)?,
            };
            result.insert(key.clone(), value);
        }
        Ok(Value::Object(result))
    }
}

/// Assign operation-independent, service-scoped names after importing the schema.
pub(crate) fn normalize(schema: &Value, prefix: &str) -> Result<Value> {
    let mut bundled = bundle(schema, schema, &mut ExpansionBudget::default())?;
    let definitions = bundled
        .as_object_mut()
        .and_then(|object| object.remove("$defs"));

    lower(&mut bundled, prefix);
    if let Some(Value::Object(definitions)) = definitions {
        let mut named = Map::new();
        for (name, mut schema) in definitions {
            lower(&mut schema, prefix);
            named.insert(format!("{prefix}{name}"), schema);
        }
        bundled[DEFINITIONS] = Value::Object(named);
    }
    Ok(bundled)
}

fn visit_alias_edges(
    value: &Value,
    definitions: &Map<String, Value>,
    active: &mut Vec<String>,
    done: &mut BTreeSet<String>,
    depth: usize,
) -> Result<()> {
    if depth > 64 {
        bail!("SDK schema graph exceeds expansion limits");
    }
    if let Some(reference) = value.get("$ref").and_then(Value::as_str) {
        check_aliases(
            reference.strip_prefix("#/$defs/").unwrap(),
            definitions,
            active,
            done,
            depth + 1,
        )?;
    }
    for key in ["oneOf", "anyOf", "allOf"] {
        if let Some(variants) = value.get(key).and_then(Value::as_array) {
            for variant in variants {
                visit_alias_edges(variant, definitions, active, done, depth + 1)?;
            }
        }
    }
    Ok(())
}

fn check_aliases(
    name: &str,
    definitions: &Map<String, Value>,
    active: &mut Vec<String>,
    done: &mut BTreeSet<String>,
    depth: usize,
) -> Result<()> {
    if depth > 64 {
        bail!("SDK schema graph exceeds expansion limits");
    }
    if done.contains(name) {
        return Ok(());
    }
    if active.iter().any(|item| item == name) {
        bail!("Recursive schema aliases require an object or array guard");
    }
    active.push(name.into());

    visit_alias_edges(&definitions[name], definitions, active, done, depth + 1)?;
    active.pop();
    done.insert(name.into());
    Ok(())
}

fn canonical<'a>(mut name: &'a str, definitions: &'a Map<String, Value>) -> Result<&'a str> {
    let mut depth = 0;
    while let Some(reference) = definitions[name].get("$ref").and_then(Value::as_str) {
        depth += 1;
        if depth > 64 {
            bail!("SDK schema graph exceeds expansion limits");
        }
        name = reference.strip_prefix("#/$defs/").unwrap();
    }
    Ok(name)
}

fn collapse(value: &mut Value, definitions: &Map<String, Value>) -> Result<()> {
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
        let name = canonical(reference.strip_prefix("#/$defs/").unwrap(), definitions)?;
        object.insert("$ref".into(), json!(format!("#/$defs/{name}")));
    }
    for (key, value) in object {
        match key.as_str() {
            "properties" | "patternProperties" | "dependentSchemas" => {
                if let Some(properties) = value.as_object_mut() {
                    for schema in properties.values_mut() {
                        collapse(schema, definitions)?;
                    }
                }
            }
            "items"
            | "additionalProperties"
            | "contains"
            | "not"
            | "if"
            | "then"
            | "else"
            | "propertyNames" => collapse(value, definitions)?,
            "oneOf" | "anyOf" | "allOf" | "prefixItems" => {
                if let Some(variants) = value.as_array_mut() {
                    for schema in variants {
                        collapse(schema, definitions)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn dependencies(value: &Value, names: &mut BTreeSet<String>) {
    let Some(object) = value.as_object() else {
        return;
    };
    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
        names.insert(reference.strip_prefix("#/$defs/").unwrap().into());
    }
    for (key, value) in object {
        match key.as_str() {
            "properties" | "patternProperties" | "dependentSchemas" => {
                if let Some(properties) = value.as_object() {
                    for schema in properties.values() {
                        dependencies(schema, names);
                    }
                }
            }
            "items"
            | "additionalProperties"
            | "contains"
            | "not"
            | "if"
            | "then"
            | "else"
            | "propertyNames" => dependencies(value, names),
            "oneOf" | "anyOf" | "allOf" | "prefixItems" => {
                if let Some(variants) = value.as_array() {
                    for schema in variants {
                        dependencies(schema, names);
                    }
                }
            }
            _ => {}
        }
    }
}

fn safe_inline(
    name: &str,
    edges: &BTreeMap<String, BTreeSet<String>>,
    memo: &mut BTreeMap<String, bool>,
    depth: usize,
) -> Result<bool> {
    if depth > 64 {
        bail!("SDK schema graph exceeds expansion limits");
    }
    if let Some(result) = memo.get(name) {
        return Ok(*result);
    }
    memo.insert(name.into(), false);
    let mut result = true;
    for child in &edges[name] {
        if !safe_inline(child, edges, memo, depth + 1)? {
            result = false;
            break;
        }
    }
    memo.insert(name.into(), result);
    Ok(result)
}

fn expand(
    value: &mut Value,
    definitions: &Map<String, Value>,
    inline: &BTreeMap<String, bool>,
    budget: &mut ExpansionBudget,
    depth: usize,
) -> Result<()> {
    if depth > 64 || budget.nodes == 0 {
        bail!("SDK schema graph exceeds expansion limits");
    }
    budget.nodes -= 1;
    if let Some(reference) = value.get("$ref").and_then(Value::as_str) {
        let name = reference.strip_prefix("#/$defs/").unwrap();
        if inline[name] {
            let mut replacement = clone_literal(&definitions[name], budget)?;
            for (key, annotation) in value.as_object().unwrap() {
                if key != "$ref" {
                    replacement
                        .as_object_mut()
                        .context("Schema reference annotations require an object target")?
                        .insert(key.clone(), clone_literal(annotation, budget)?);
                }
            }
            *value = replacement;
            return expand(value, definitions, inline, budget, depth + 1);
        }
    }
    if let Some(object) = value.as_object_mut() {
        for (key, value) in object {
            match key.as_str() {
                "properties" | "patternProperties" | "dependentSchemas" => {
                    if let Some(properties) = value.as_object_mut() {
                        for schema in properties.values_mut() {
                            expand(schema, definitions, inline, budget, depth + 1)?;
                        }
                    }
                }
                "items"
                | "additionalProperties"
                | "contains"
                | "not"
                | "if"
                | "then"
                | "else"
                | "propertyNames" => expand(value, definitions, inline, budget, depth + 1)?,
                "oneOf" | "anyOf" | "allOf" | "prefixItems" => {
                    if let Some(variants) = value.as_array_mut() {
                        for schema in variants {
                            expand(schema, definitions, inline, budget, depth + 1)?;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn lower(value: &mut Value, prefix: &str) {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.remove("$ref") {
                let name = reference
                    .as_str()
                    .unwrap()
                    .strip_prefix("#/$defs/")
                    .unwrap();
                object.insert(REFERENCE.into(), json!(format!("{prefix}{name}")));
            }
            for (key, value) in object {
                match key.as_str() {
                    "properties" | "patternProperties" | "dependentSchemas" => {
                        if let Some(properties) = value.as_object_mut() {
                            for schema in properties.values_mut() {
                                lower(schema, prefix);
                            }
                        }
                    }
                    "items"
                    | "additionalProperties"
                    | "contains"
                    | "not"
                    | "if"
                    | "then"
                    | "else"
                    | "propertyNames"
                    | "oneOf"
                    | "anyOf"
                    | "allOf"
                    | "prefixItems" => lower(value, prefix),
                    _ => {}
                }
            }
        }
        Value::Array(values) => values.iter_mut().for_each(|value| lower(value, prefix)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_data_cannot_bypass_graph_resource_limits() {
        let mut deep = json!(null);
        for _ in 0..66 {
            deep = json!([deep]);
        }
        assert!(normalize(&json!({"type":"string","default":deep}), "Literal").is_err());
        let mut budget = ExpansionBudget {
            nodes: 2,
            bytes: 100,
        };
        assert!(clone_literal(&json!([0, 1]), &mut budget).is_err());
        let mut budget = ExpansionBudget { nodes: 2, bytes: 3 };
        assert!(clone_literal(&json!("\""), &mut budget).is_err());
        let mut budget = ExpansionBudget { nodes: 2, bytes: 4 };
        assert_eq!(
            clone_literal(&json!("\""), &mut budget).unwrap(),
            json!("\"")
        );
        assert_eq!(budget.bytes, 0);
    }

    #[test]
    fn self_and_mutual_recursion_terminate_with_named_edges() {
        let schema = json!({"$ref":"#/$defs/Node", "$defs":{
            "Node":{"type":"object","properties":{"child":{"$ref":"#/$defs/Node"},"peer":{"$ref":"#/$defs/Peer"}}},
            "Peer":{"type":"array","items":{"$ref":"#/$defs/Node"}}
        }});
        let value = normalize(&schema, "Tree").unwrap();
        assert_eq!(value["type"], "object");
        assert_eq!(
            value[DEFINITIONS]["TreeRef1"]["properties"]["child"][REFERENCE],
            "TreeRef1"
        );
        assert_eq!(
            value[DEFINITIONS]["TreeRef2"]["items"][REFERENCE],
            "TreeRef1"
        );
        assert_eq!(value[DEFINITIONS].as_object().unwrap().len(), 2);
    }

    #[test]
    fn root_references_property_names_and_literals_are_distinct() {
        let schema = json!({"type":"object","properties":{
            "$ref":{"type":"string"}, "nested":{"$ref":"#"}
        },"const":{"$ref":"https://example.invalid/literal"}});
        let value = normalize(&schema, "Root").unwrap();
        assert_eq!(value["properties"]["$ref"]["type"], "string");
        assert_eq!(value["properties"]["nested"][REFERENCE], "RootRef1");
        assert_eq!(value["const"], schema["const"]);
    }

    #[test]
    fn invalid_references_reserved_markers_and_unguarded_aliases_fail() {
        for schema in [
            json!({"$ref":"#/missing"}),
            json!({"$ref":"https://example.invalid"}),
            json!({"$ref":"#anchor"}),
            json!({"$ref":"#"}),
            json!({"anyOf":[{"$ref":"#"},{"type":"string"}]}),
            json!({REFERENCE:"Forged"}),
            json!({"$id":"https://example.invalid/schema","type":"object","properties":{"child":{"$ref":"#"}}}),
            json!({"$dynamicRef":"#node"}),
            json!({"$ref":"#/$defs/A","title":"annotation","$defs":{"A":true}}),
            json!({"$ref":"#/$defs/A","$defs":{"A":42}}),
            json!({"$ref":"#/$defs/A","minLength":3,"$defs":{"A":{"type":"string"}}}),
        ] {
            assert!(normalize(&schema, "Invalid").is_err(), "{schema}");
        }
    }

    #[test]
    fn cached_wide_graphs_cannot_bypass_recursive_processing_limits() {
        for aliases in [true, false] {
            let mut definitions = Map::new();
            let mut properties = Map::new();
            for index in 0..300 {
                let name = format!("Node{index:04}");
                let schema = if index == 0 {
                    json!({"type":"string"})
                } else {
                    let reference = json!({"$ref":format!("#/$defs/Node{:04}", index - 1)});
                    if aliases {
                        reference
                    } else {
                        json!({"type":"object","properties":{"next":reference}})
                    }
                };
                definitions.insert(name.clone(), schema);
                properties.insert(name.clone(), json!({"$ref":format!("#/$defs/{name}")}));
            }
            let schema = json!({"type":"object","properties":properties,"$defs":definitions});
            assert!(normalize(&schema, "Wide").is_err());
        }
    }

    #[test]
    fn shared_targets_do_not_expand_exponentially_and_limits_still_apply() {
        let schema = json!({"type":"object","properties":{"a":{"$ref":"#/$defs/A"},"b":{"$ref":"#/$defs/A"}},"$defs":{"A":{"type":"string"}}});
        let value = normalize(&schema, "Shared").unwrap();
        assert!(value.get(DEFINITIONS).is_none());
        assert_eq!(value["properties"]["a"]["type"], "string");
        assert!(
            bundle(
                &schema,
                &schema,
                &mut ExpansionBudget {
                    nodes: 2,
                    bytes: 1024
                }
            )
            .is_err()
        );
        assert!(
            bundle(
                &json!({"const":"x".repeat(200)}),
                &schema,
                &mut ExpansionBudget {
                    nodes: 100,
                    bytes: 100
                }
            )
            .is_err()
        );
    }
}
