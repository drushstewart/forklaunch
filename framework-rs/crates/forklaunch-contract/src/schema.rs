//! Contract schemas are standalone documents; embedding changes their JSON pointer base.
use serde_json::Value;

/// Relocate document-local JSON pointers when embedding a standalone schema.
///
/// Location segments are escaped as JSON pointer tokens. Schema literals and
/// annotations are preserved; malformed JSON is returned unchanged. Resources
/// with their own `$id` retain their reference base.
#[must_use]
pub fn relocate_schema_json(schema: &str, location: &[&str]) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(schema) else {
        return schema.into();
    };
    let root = value.clone();
    let base = location
        .iter()
        .fold(String::from("#"), |mut base, segment| {
            base.push('/');
            base.push_str(&segment.replace('~', "~0").replace('/', "~1"));
            base
        });
    visit(&mut value, &root, &base);
    value.to_string()
}

fn visit(value: &mut Value, root: &Value, base: &str) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if object
        .get("$id")
        .and_then(Value::as_str)
        .is_some_and(|id| !id.is_empty())
    {
        return;
    }
    if let Some(reference) = object
        .get("$ref")
        .and_then(Value::as_str)
        .map(str::to_owned)
        && let Some(pointer) = reference.strip_prefix('#')
        && (pointer.is_empty() || pointer.starts_with('/'))
        && root.pointer(pointer).is_some()
    {
        object.insert("$ref".into(), Value::String(format!("{base}{pointer}")));
    }
    for (key, value) in object {
        match key.as_str() {
            "properties" | "patternProperties" | "dependentSchemas" | "dependencies" | "$defs"
            | "definitions" => {
                if let Some(schemas) = value.as_object_mut() {
                    for schema in schemas.values_mut() {
                        visit(schema, root, base);
                    }
                }
            }
            "additionalProperties"
            | "additionalItems"
            | "unevaluatedProperties"
            | "unevaluatedItems"
            | "contentSchema"
            | "contains"
            | "not"
            | "if"
            | "then"
            | "else"
            | "propertyNames" => visit(value, root, base),
            "items" => {
                if let Some(schemas) = value.as_array_mut() {
                    for schema in schemas {
                        visit(schema, root, base);
                    }
                } else {
                    visit(value, root, base);
                }
            }
            "oneOf" | "anyOf" | "allOf" | "prefixItems" => {
                if let Some(schemas) = value.as_array_mut() {
                    for schema in schemas {
                        visit(schema, root, base);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tuple_and_conditional_schema_refs_move_but_embedded_resources_keep_their_base() {
        let input = json!({"type":"array", "items":[{"$ref":"#"}],
            "contains":{"if":{"$ref":"#"},"then":{"$ref":"#"}},
            "$defs":{"Resource":{"$id":"https://example.invalid/nested","type":"object","properties":{"self":{"$ref":"#"}}}},
            "unevaluatedItems":{"$ref":"#"}});
        let value: Value = serde_json::from_str(&relocate_schema_json(
            &input.to_string(),
            &["properties", "body"],
        ))
        .unwrap();
        assert_eq!(value["items"][0]["$ref"], "#/properties/body");
        assert_eq!(value["contains"]["if"]["$ref"], "#/properties/body");
        assert_eq!(value["unevaluatedItems"]["$ref"], "#/properties/body");
        assert_eq!(value["$defs"]["Resource"], input["$defs"]["Resource"]);
        let resource = input["$defs"]["Resource"].to_string();
        assert_eq!(
            serde_json::from_str::<Value>(&relocate_schema_json(
                &resource,
                &["properties", "body"]
            ))
            .unwrap(),
            input["$defs"]["Resource"]
        );
    }

    #[test]
    fn standalone_recursive_refs_are_rebased_without_rewriting_literals_or_names() {
        let input = json!({"$ref":"#/$defs/A", "$defs":{"A":{"type":"object","properties":{
            "$ref":{"type":"string"}, "self":{"$ref":"#"}, "child":{"$ref":"#/$defs/A"}
        },"default":{"$ref":"#"},"examples":[{"$ref":"#/$defs/A"}]}}});
        let value: Value = serde_json::from_str(&relocate_schema_json(
            &input.to_string(),
            &["components", "schemas", "Tree/Node~"],
        ))
        .unwrap();
        let base = "#/components/schemas/Tree~1Node~0";
        assert_eq!(value["$ref"], format!("{base}/$defs/A"));
        assert_eq!(value["$defs"]["A"]["properties"]["self"]["$ref"], base);
        assert_eq!(value["$defs"]["A"]["properties"]["$ref"]["type"], "string");
        assert_eq!(
            value["$defs"]["A"]["default"],
            input["$defs"]["A"]["default"]
        );
        assert_eq!(
            value["$defs"]["A"]["examples"],
            input["$defs"]["A"]["examples"]
        );
    }
}
