//! Compiled JSON Schema validation for generated clients and raw socket contracts.
use crate::codec::{JsonValidator, PayloadError};
use serde_json::Value;
use std::sync::Arc;

pub(crate) fn compile(schema: &Value) -> Result<Arc<JsonValidator>, PayloadError> {
    schema_budget(schema)?;
    // Match the TypeScript AJV full-format profile: email domains
    // require a dot, rather than accepting local-only mailboxes.
    let email = regex::Regex::new(r"(?i)^[a-z0-9!#$%&'*+/=?^_`{|}~-]+(?:\.[a-z0-9!#$%&'*+/=?^_`{|}~-]+)*@(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$")
        .expect("static email pattern");
    let uuid =
        regex::Regex::new(r"(?i)^(?:urn:uuid:)?[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$")
            .expect("static UUID pattern");
    let mut options = jsonschema::options()
        .with_format("email", move |value| email.is_match(value))
        .with_format("uuid", move |value| uuid.is_match(value))
        .with_retriever(NoResources)
        .should_validate_formats(true)
        .with_pattern_options(
            jsonschema::PatternOptions::fancy_regex()
                .backtrack_limit(100_000)
                .size_limit(1024 * 1024)
                .dfa_size_limit(1024 * 1024),
        );
    if schema.get("$schema").is_none() {
        options = options.with_draft(jsonschema::Draft::Draft7);
    }
    let validator = options
        .build(schema)
        .map_err(|error| PayloadError::Invalid(format!("invalid socket schema: {error}")))?;
    Ok(Arc::new(move |value| {
        validator.validate(value).map_err(|error| error.to_string())
    }))
}
struct NoResources;
impl jsonschema::Retrieve for NoResources {
    fn retrieve(
        &self,
        _uri: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err("socket schema resource retrieval is disabled".into())
    }
}
fn schema_budget(schema: &Value) -> Result<(), PayloadError> {
    let mut pending = vec![(schema, 0)];
    let mut remaining = 100_000_usize;
    while let Some((value, depth)) = pending.pop() {
        if depth > 64 || remaining == 0 {
            return Err(PayloadError::Invalid(
                "socket schema exceeds depth/node limits".into(),
            ));
        }
        remaining -= 1;
        let children = match value {
            Value::Array(values) => values.len(),
            Value::Object(values) => values.len(),
            _ => 0,
        };
        if children > remaining.saturating_sub(pending.len()) {
            return Err(PayloadError::Invalid(
                "socket schema exceeds node limit".into(),
            ));
        }
        match value {
            Value::Array(values) => pending.extend(values.iter().map(|value| (value, depth + 1))),
            Value::Object(values) => {
                pending.extend(values.values().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
    }
    serde_json::to_writer(&mut SchemaBytes(32 * 1024 * 1024), schema)
        .map_err(|_| PayloadError::Invalid("socket schema exceeds 32 MiB".into()))
}
struct SchemaBytes(usize);
impl std::io::Write for SchemaBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other("schema byte limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::PayloadCodec;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    #[test]
    fn schema_validation_precedes_deserialization_and_preserves_original_outgoing_strings() {
        #[derive(Serialize, Deserialize)]
        struct Input {
            message: String,
        }
        let codec = PayloadCodec::<Input>::json_schema(&json!({"type":"object","required":["message"],"properties":{"message":{"type":"string","minLength":1}},"additionalProperties":false})).unwrap();
        assert!(codec.decode(br#"{"message":"hello"}"#).is_ok());
        assert!(
            codec
                .decode(br#"{"message":"hello","extra":true}"#)
                .is_err()
        );
        assert!(codec.decode(br#"{"message":""}"#).is_err());
        assert!(
            codec
                .encode(&Input {
                    message: String::new()
                })
                .is_err()
        );
        let string = PayloadCodec::<String>::json_schema(&json!({"type":"string"})).unwrap();
        assert_eq!(string.encode(&"42".into()).unwrap(), b"42");
        assert!(string.decode(b"42").is_err()); // The canonical raw-string wire ambiguity is retained.
        assert_eq!(string.decode(b"hello").unwrap(), "hello");
        let bounded = string.with_max_bytes(4).unwrap();
        assert!(matches!(
            bounded.encode(&"large".into()),
            Err(PayloadError::Limit { .. })
        ));
    }
    #[test]
    fn integer_spellings_follow_javascript_json_numbers_before_typed_deserialization() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Integers {
            value: i64,
            nested: Vec<i64>,
        }
        for dialect in [
            "http://json-schema.org/draft-04/schema#",
            "http://json-schema.org/draft-07/schema#",
        ] {
            let schema = json!({"$schema":dialect,"type":"object","required":["value","nested"],"properties":{"value":{"type":"integer"},"nested":{"type":"array","items":{"type":"integer"}}}});
            let codec = PayloadCodec::<Integers>::json_schema(&schema).unwrap();
            assert_eq!(
                codec
                    .decode(br#"{"value":1.0,"nested":[1e0,-0.0]}"#)
                    .unwrap(),
                Integers {
                    value: 1,
                    nested: vec![1, 0]
                }
            );
            assert!(codec.decode(br#"{"value":1.5,"nested":[]}"#).is_err());
            assert!(codec.decode(br#"{"value":"1","nested":[]}"#).is_err());
        }
        let codec = PayloadCodec::<f64>::json_schema(
            &json!({"$schema":"http://json-schema.org/draft-04/schema#","type":"integer"}),
        )
        .unwrap();
        assert_eq!(codec.encode(&1.0).unwrap(), b"1.0");
        assert!(codec.encode(&1.5).is_err());
        let value = PayloadCodec::<Value>::json_schema(
            &json!({"$schema":"http://json-schema.org/draft-04/schema#","type":"integer"}),
        )
        .unwrap();
        assert!(
            value
                .decode(b"-0.0")
                .unwrap()
                .as_f64()
                .unwrap()
                .is_sign_negative()
        );
        // Generic serde codecs retain their explicitly stricter integer decoding.
        assert!(PayloadCodec::<i64>::serde().decode(b"1.0").is_err());
    }

    #[test]
    fn invalid_schemas_and_external_retrieval_fail_without_reading_resources() {
        for schema in [
            json!({"type":"invalid"}),
            json!({"$ref":"#/missing"}),
            json!({"$ref":"https://example.com/unavailable"}),
            json!({"$ref":"file:///unavailable"}),
        ] {
            assert!(PayloadCodec::<Value>::json_schema(&schema).is_err());
        }
        let file =
            std::env::temp_dir().join(format!("forklaunch-ws-schema-{}.json", std::process::id()));
        std::fs::write(&file, r#"{"type":"string"}"#).unwrap();
        let result = PayloadCodec::<Value>::json_schema(
            &json!({"$ref":format!("file://{}",file.display())}),
        );
        std::fs::remove_file(file).unwrap();
        let Err(error) = result else {
            panic!(
                "external file retrieval must remain disabled even when dependency features enable it"
            );
        };
        assert!(
            error
                .to_string()
                .contains("socket schema resource retrieval is disabled"),
            "{error}"
        );
    }
    #[test]
    fn schema_compilation_bounds_depth_nodes_bytes_and_regex_size() {
        let mut schema = json!(true);
        for _ in 0..66 {
            schema = json!({"properties":{"next":schema}});
        }
        assert!(PayloadCodec::<Value>::json_schema(&schema).is_err());
        assert!(PayloadCodec::<Value>::json_schema(&json!({"enum":vec![0;100_001]})).is_err());
        assert!(
            PayloadCodec::<Value>::json_schema(&json!({"title":"x".repeat(32 * 1024 * 1024)}))
                .is_err()
        );
        assert!(PayloadCodec::<Value>::json_schema(&json!({"pattern":"["})).is_err());
    }
    #[test]
    fn json_schema_payload_conformance() {
        let Ok(input) = std::env::var("FORKLAUNCH_TEST_WS_JSON_SCHEMA_INPUT") else {
            return;
        };
        let cases: Vec<Value> = serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
        let results: Vec<_> = cases
            .iter()
            .map(|case| {
                let codec = PayloadCodec::<Value>::json_schema(&case["schema"]).unwrap();
                if case["action"] == "encode" {
                    match codec.encode(&case["value"]) {
                        Ok(bytes) => json!({"ok":true,"bytes":bytes}),
                        Err(_) => json!({"ok":false}),
                    }
                } else {
                    let bytes: Vec<u8> = serde_json::from_value(case["bytes"].clone()).unwrap();
                    match codec.decode(&bytes) {
                        Ok(value) => json!({"ok":true,"value":value}),
                        Err(_) => json!({"ok":false}),
                    }
                }
            })
            .collect();
        std::fs::write(
            std::env::var("FORKLAUNCH_TEST_WS_JSON_SCHEMA_OUTPUT").unwrap(),
            serde_json::to_vec(&results).unwrap(),
        )
        .unwrap();
    }
}
