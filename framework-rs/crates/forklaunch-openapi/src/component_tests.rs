use super::*;
use serde_json::{Value, json};

#[test]
fn component_names_are_valid_unique_and_preserve_recursive_references() {
    let names = [
        "Ordinary.Name-1_2",
        "Surface User Roles",
        "Vec<String>",
        "slash/~percent% 水",
        "A B",
        "_fl_encoded_412042",
        "",
        "_fl_encoded_",
    ];
    let mut registry = ContractRegistry::new();
    for (index, name) in names.iter().enumerate() {
        let schema = json!({
            "title": name,
            "type": "object",
            "properties": {
                "next": {"$ref": "#"},
                "value": {"$ref": "#/$defs/Value"}
            },
            "$defs": {"Value": {"type": "string"}},
            "examples": [{"$ref": "literal annotation"}]
        });
        let mut body = BodyContract::json(*name);
        body.schema.json_schema = Some(schema.to_string());
        registry.register(
            RouteContract::get(format!("Read{index}"), "Read", format!("/items/{index}"))
                .with_response(200, Some(body)),
        );
    }
    let document: Value = serde_json::from_str(&generate_openapi_json(&registry)).unwrap();
    let components = document["components"]["schemas"].as_object().unwrap();
    assert_eq!(components.len(), names.len());
    assert!(components.contains_key("Ordinary.Name-1_2"));
    for name in components.keys() {
        assert!(!name.is_empty());
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        );
    }
    for (index, name) in names.iter().enumerate() {
        let reference = document["paths"][format!("/items/{index}")]["get"]["responses"]["200"]
            ["content"]["application/json"]["schema"]["$ref"].as_str().unwrap();
        let schema = document
            .pointer(reference.strip_prefix('#').unwrap())
            .unwrap();
        assert_eq!(schema["title"], *name);
        let recursive = schema["properties"]["next"]["$ref"].as_str().unwrap();
        assert_eq!(recursive, reference);
        let child = schema["properties"]["value"]["$ref"].as_str().unwrap();
        assert_eq!(
            document.pointer(child.strip_prefix('#').unwrap()).unwrap()["type"],
            "string"
        );
        assert_eq!(schema["examples"][0]["$ref"], "literal annotation");
    }
}
