//! Adapt unscoped multipart file schemas to the parser's byte-array values.
use forklaunch_contract::{bundle_literal_schema_references_unscoped, schema_locations};
use serde_json::{Value, json};

pub(super) fn lower(schema: &Value) -> Result<Option<Value>, String> {
    let mut root = schema.clone();
    if let Some(object) = root.as_object_mut() {
        object
            .entry("$schema")
            .or_insert_with(|| json!("http://json-schema.org/draft-07/schema#"));
    }
    // Keep resource-scoped schemas on their existing validator path. The
    // unscoped helper explicitly rejects IDs/anchors rather than rebasing them.
    let Ok(mut runtime) = bundle_literal_schema_references_unscoped(&root) else {
        return Ok(None);
    };
    let locations = schema_locations(&runtime, false).map_err(|error| error.to_string())?;
    let mut changed = false;
    for location in locations {
        let Some(node) = runtime.pointer_mut(&location) else {
            continue;
        };
        if node.get("type").and_then(Value::as_str) != Some("string")
            || node.get("format").and_then(Value::as_str) != Some("binary")
        {
            continue;
        }
        if !super::opaque_binary_schema(node) {
            return Err("Multipart opaque file schemas cannot include string assertions; use a native byte-array schema for byte constraints".into());
        }
        let object = node.as_object_mut().expect("binary schema object");
        // TypeScript's file validator accepts both Buffer and string parts.
        object.insert("type".into(), json!(["array", "string"]));
        object.remove("format");
        object.remove("oneOf"); // Exact legacy TS File marker checked above.
        object.insert(
            "items".into(),
            json!({"type":"integer","minimum":0,"maximum":255}),
        );
        changed = true;
    }
    Ok(changed.then_some(runtime))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_references_are_lowered_without_changing_literal_data_or_exports() {
        let schema = json!({"type":"object","required":["file","metadata"],"properties":{"file":{"$ref":"#/default/file"},"metadata":{"const":{"type":"string","format":"binary"}}},"default":{"file":{"type":"string","format":"binary","title":"Upload"}}});
        let original = schema.clone();
        let lowered = lower(&schema).unwrap().unwrap();
        assert_eq!(schema, original);
        assert_eq!(lowered["default"], schema["default"]);
        assert_eq!(
            lowered["properties"]["metadata"],
            schema["properties"]["metadata"]
        );
        let validate = forklaunch_contract::schema_validation::compile(&lowered).unwrap();
        assert!(
            validate(&json!({"file":[0,255],"metadata":{"type":"string","format":"binary"}}))
                .is_ok()
        );
        assert!(
            validate(&json!({"file":"text","metadata":{"type":"string","format":"binary"}}))
                .is_ok()
        );
        for file in [json!([-1]), json!([256]), json!([1.5])] {
            assert!(
                validate(&json!({"file":file,"metadata":{"type":"string","format":"binary"}}))
                    .is_err()
            );
        }
    }
    #[test]
    fn opaque_assertions_are_not_discarded_and_scoped_schemas_keep_their_profile() {
        assert!(lower(&json!({"type":"object","properties":{"file":{"type":"string","format":"binary","minLength":1}}})).is_err());
        assert!(lower(&json!({"$id":"https://example.invalid/file","type":"array","items":{"type":"integer"},"default":{"type":"string","format":"binary"}})).unwrap().is_none());
        for schema in [
            json!({"type":"string","format":"binary","title":"File","oneOf":[{}]}),
            json!({"type":"string","format":"binary","title":"Manual","oneOf":[{},{},{},{"type":["string"]}]}),
            json!({"type":"string","format":"binary","title":"File","oneOf":[{},{},{},{"type":["string"]}],"maxLength":1}),
        ] {
            assert!(lower(&schema).is_err());
        }
    }
}
