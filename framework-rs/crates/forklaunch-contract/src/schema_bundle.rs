//! Copy referenced schemas out of literal data before rewriting schema edges.
use std::collections::{BTreeMap, BTreeSet};

use forklaunch_common::{FrameworkError, FrameworkResult};
use serde_json::{Map, Value};

use super::schema::{local_pointer, schema_graph};

/// Bundle local schema targets stored inside literal assertions or annotations.
/// Original const/enum/default/example values remain byte-for-byte JSON values.
/// Recursive references use reserved, collision-checked definition names.
/// Use only within a checked profile that rejects active resource IDs and
/// dynamic scope; ignored annotation IDs do not establish a reference base.
///
/// # Errors
/// Rejects invalid definition containers or graphs/copies exceeding 64 levels,
/// 100,000 JSON nodes, or 32 MiB. This helper does not resolve resource IDs.
pub fn bundle_literal_schema_references_unscoped(root: &Value) -> FrameworkResult<Value> {
    bundle_with_resource_policy(root, false)
}

pub(super) fn bundle_with_resource_policy(
    root: &Value,
    preserve_resource_bases: bool,
) -> FrameworkResult<Value> {
    let mut budget = CopyBudget::default();
    let mut output = budget.copy(root)?;
    let graph = schema_graph(root, preserve_resource_bases)?;
    let targets: Vec<_> = graph
        .reference_targets
        .iter()
        .filter(|target| inside_literal(target, &graph.literal_ranges, ""))
        .collect();
    if targets.is_empty() {
        return Ok(output);
    }
    let definitions = output
        .as_object_mut()
        .ok_or_else(|| invalid("schema root must be an object"))?
        .entry("definitions")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| invalid("schema definitions must be an object"))?;
    let mut copies = BTreeMap::new();
    let mut index = 0;
    for target in targets {
        let name = loop {
            let name = format!("ForklaunchLiteral{index}");
            index += 1;
            if !definitions.contains_key(&name) {
                break name;
            }
        };
        let schema = root
            .pointer(target)
            .ok_or_else(|| invalid("literal schema target is missing"))?;
        if !schema.is_object() && !schema.is_boolean() {
            return Err(invalid(
                "literal schema target must be an object or boolean",
            ));
        }
        definitions.insert(name.clone(), budget.copy(schema)?);
        copies.insert(target.clone(), format!("/definitions/{name}"));
    }
    let locations: BTreeSet<_> = graph.locations.into_iter().collect();
    for location in &locations {
        if !inside_literal(location, &graph.literal_ranges, "") {
            rewrite_reference(&mut output, location, &copies)?;
        }
    }
    for (source, destination) in &copies {
        let prefix = format!("{source}/");
        for location in locations.range(source.clone()..) {
            if location != source && !location.starts_with(&prefix) {
                break;
            }
            if !inside_literal(location, &graph.literal_ranges, source) {
                rewrite_reference(
                    &mut output,
                    &format!("{destination}{}", &location[source.len()..]),
                    &copies,
                )?;
            }
        }
    }
    CopyBudget::default().check(&output)?;
    Ok(output)
}

fn inside_literal(location: &str, literals: &BTreeSet<String>, scope: &str) -> bool {
    (location.len() > scope.len() && literals.contains(location))
        || location
            .rmatch_indices('/')
            .any(|(end, _)| end > scope.len() && literals.contains(&location[..end]))
}

fn rewrite_reference(
    output: &mut Value,
    location: &str,
    copies: &BTreeMap<String, String>,
) -> FrameworkResult<()> {
    let node = output
        .pointer_mut(location)
        .ok_or_else(|| invalid("bundled schema location is missing"))?;
    if let Some(object) = node.as_object_mut()
        && let Some(target) = object
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(local_pointer)
        && let Some(destination) = copies.get(&target)
    {
        object.insert("$ref".into(), Value::String(format!("#{destination}")));
    }
    Ok(())
}

struct CopyBudget {
    nodes: usize,
    bytes: usize,
}
impl Default for CopyBudget {
    fn default() -> Self {
        Self {
            nodes: 100_000,
            bytes: 32 * 1024 * 1024,
        }
    }
}
impl CopyBudget {
    fn copy(&mut self, value: &Value) -> FrameworkResult<Value> {
        self.check(value)?;
        Ok(value.clone())
    }
    fn check(&mut self, value: &Value) -> FrameworkResult<()> {
        let mut pending = vec![(value, 0)];
        while let Some((node, depth)) = pending.pop() {
            if depth > 64 || self.nodes == 0 {
                return Err(invalid("schema bundling exceeds depth/node limits"));
            }
            self.nodes -= 1;
            let children = match node {
                Value::Array(values) => values.len(),
                Value::Object(values) => values.len(),
                _ => 0,
            };
            if children > self.nodes.saturating_sub(pending.len()) {
                return Err(invalid("schema bundling exceeds node limit"));
            }
            match node {
                Value::Array(values) => pending.extend(values.iter().map(|v| (v, depth + 1))),
                Value::Object(values) => pending.extend(values.values().map(|v| (v, depth + 1))),
                _ => {}
            }
        }
        serde_json::to_writer(&mut *self, value)
            .map_err(|_| invalid("schema bundling exceeds 32 MiB"))
    }
}
impl std::io::Write for CopyBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other("schema copy byte limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn invalid(message: &str) -> FrameworkError {
    FrameworkError::new("schema_bundling_invalid", message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nested_recursive_literal_targets_are_copied_without_changing_assertions() {
        let schema = json!({"properties":{"item":{"$ref":"#/default"}},"default":{"type":"object","properties":{"next":{"$ref":"#/default"},"value":{"$ref":"#/enum/0"}},"const":{"$ref":"#/default"}},"enum":[{"type":"string","minLength":2}],"definitions":{"ForklaunchLiteral0":{"type":"boolean"}}});
        let output = bundle_literal_schema_references_unscoped(&schema).unwrap();
        assert_eq!(output["default"], schema["default"]);
        assert_eq!(output["enum"], schema["enum"]);
        let target = local_pointer(output["properties"]["item"]["$ref"].as_str().unwrap()).unwrap();
        let copy = output.pointer(&target).unwrap();
        assert_eq!(copy["properties"]["next"]["$ref"], format!("#{target}"));
        assert_eq!(copy["const"], schema["default"]["const"]);
        let value = local_pointer(copy["properties"]["value"]["$ref"].as_str().unwrap()).unwrap();
        assert_eq!(output.pointer(&value), Some(&schema["enum"][0]));
        assert_eq!(
            output["definitions"]["ForklaunchLiteral0"],
            schema["definitions"]["ForklaunchLiteral0"]
        );
        assert!(super::super::schema::schema_locations(&output, false).is_ok());
        assert_eq!(
            bundle_literal_schema_references_unscoped(&output).unwrap(),
            output
        );
    }

    #[test]
    fn dictionary_aliases_cannot_change_simultaneous_schema_and_literal_roles() {
        let schema = json!({"type":"object","properties":{"item":{"$ref":"#/properties"},"const":{"$ref":"#/definitions/Value"}},"definitions":{"Value":{"type":"string"}}});
        assert!(
            bundle_literal_schema_references_unscoped(&schema)
                .unwrap_err()
                .message()
                .contains("dictionaries")
        );
        assert_eq!(
            super::super::schema::relocate_schema_json(&schema.to_string(), &["payload"]),
            schema.to_string()
        );
    }

    #[test]
    fn literal_copies_cannot_exceed_shared_budgets_or_mutate_the_input() {
        let schema = json!({"$ref":"#/default","default":{"type":"string"}});
        let original = schema.clone();
        assert!(
            CopyBudget {
                nodes: 2,
                bytes: 1000
            }
            .copy(&schema)
            .is_err()
        );
        assert!(
            CopyBudget {
                nodes: 100,
                bytes: 2
            }
            .copy(&schema)
            .is_err()
        );
        assert!(
            bundle_literal_schema_references_unscoped(&json!({"$ref":"#/const","const":42}))
                .is_err()
        );
        assert_eq!(schema, original);
    }
}
