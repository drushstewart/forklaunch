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

#[test]
fn path_error_inference_is_conservative_and_preserves_explicit_contracts() {
    use forklaunch_contract::{RequestContract, VersionedContract};
    let path_route = |index| {
        RouteContract::get(format!("Path{index}"), "Path", format!("/items{index}/:id"))
            .with_param(FieldContract::required("id", "String"))
    };
    let mut registry = ContractRegistry::new();
    registry.register(path_route(0));
    registry.register(path_route(1).with_query(FieldContract::required("q", "String")));
    registry.register(path_route(2).with_request_body(BodyContract::json("Input")));
    let mut headers = path_route(3);
    headers
        .request
        .headers
        .push(FieldContract::required("x-custom", "String"));
    registry.register(headers);
    let mut constrained = path_route(4);
    constrained.request.params[0].schema.json_schema =
        Some(r#"{"type":"string","minLength":2}"#.into());
    registry.register(constrained);
    registry.register(path_route(5).with_response(400, Some(BodyContract::json("CustomError"))));
    registry.register(path_route(6).with_version(VersionedContract {
        version: "v1".into(),
        request: RequestContract::default(),
        responses: vec![],
    }));
    // Unmatched required metadata still produces a native missing-param JSON400.
    let mut missing_param = path_route(7);
    missing_param
        .request
        .params
        .push(FieldContract::required("ghost", "String"));
    registry.register(missing_param);
    registry.register(
        RouteContract::get("Flat", "Flat", "/flat")
            .with_param(FieldContract::required("id", "String")),
    );
    let options = OpenApiOptions::from_registry(&registry).with_typescript_path_errors();
    let document: Value =
        serde_json::from_str(&generate_openapi_json_with_options(&registry, &options)).unwrap();
    assert_eq!(
        document["paths"]["/items0/{id}"]["get"]["responses"]["400"]["content"]["text/plain"]["schema"]
            ["type"],
        "string"
    );
    for index in [1, 2, 3, 4, 6, 7] {
        assert!(
            document["paths"][format!("/items{index}/{{id}}")]["get"]["responses"]
                .get("400")
                .is_none()
        );
    }
    assert!(
        document["paths"]["/items5/{id}"]["get"]["responses"]["400"]["content"]
            .get("application/json")
            .is_some()
    );
    assert!(
        document["paths"]["/flat"]["get"]["responses"]
            .get("400")
            .is_none()
    );
    let native: Value = serde_json::from_str(&generate_openapi_json(&registry)).unwrap();
    assert!(
        native["paths"]["/items0/{id}"]["get"]["responses"]
            .get("400")
            .is_none()
    );
    let versioned: Value =
        serde_json::from_str(&generate_versioned_openapi_json(&registry, &options)).unwrap();
    assert!(
        versioned["v1"]["paths"]["/items6/{id}"]["get"]["responses"]
            .get("400")
            .is_none()
    );
}
