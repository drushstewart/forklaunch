use axum::Json;
use forklaunch_axum::ForkLaunchRouter;
use forklaunch_contract::{BodyContract, BodyKind, RouteContract, SchemaRef};
use serde_json::{Value, json};

const TREE: &str = r##"{"type":"object","additionalProperties":false,"required":["label","children","parent"],"properties":{"label":{"type":"string"},"children":{"type":"array","items":{"$ref":"#"}},"parent":{"anyOf":[{"$ref":"#"},{"type":"null"}]},"optional":{"$ref":"#"},"metadata":{"type":"object"}},"examples":[{"label":"literal","children":[],"parent":null,"metadata":{"$ref":"#"}}]}"##;
const FOREST: &str = r##"{"type":"array","items":{"$ref":"#"}}"##;
const CHOICE: &str = r##"{"anyOf":[{"type":"string"},{"type":"array","items":{"$ref":"#"}},{"type":"object","additionalProperties":{"$ref":"#"}}]}"##;
const MUTUAL: &str = r##"{"$ref":"#MutualA","$defs":{"A":{"$anchor":"MutualA","type":"object","additionalProperties":false,"required":["id","peer"],"properties":{"id":{"type":"string"},"peer":{"$ref":"#MutualB"}}},"B":{"$anchor":"MutualB","type":"object","additionalProperties":false,"required":["children"],"properties":{"children":{"type":"array","items":{"$ref":"#MutualA"}}}}}}"##;

fn body(name: &str, schema: &str) -> BodyContract {
    let schema = if let Ok(mode) = std::env::var("FORKLAUNCH_TEST_SCHEMA_RESOURCES") {
        let mut value: Value = serde_json::from_str(schema).unwrap();
        value["$id"] = json!(format!("https://schemas.fixture.test/{name}/index.json"));
        if name == "Mutual" {
            value["$ref"] = json!("nodes/a#MutualA");
            value["$defs"]["A"]["$id"] = json!("nodes/a");
            value["$defs"]["B"]["$id"] = json!("nodes/b");
            value["$defs"]["A"]["properties"]["peer"]["$ref"] = json!("b#MutualB");
            value["$defs"]["B"]["properties"]["children"]["items"]["$ref"] = json!("a#MutualA");
        }
        if mode == "siblings" && name == "Mutual" {
            value["minProperties"] = json!(1);
        }
        if let Some(draft) = mode.strip_prefix("fragment").or_else(|| mode.strip_prefix("uri")).or_else(|| mode.strip_prefix("pointer")) {
            value["$schema"] = json!(format!("http://json-schema.org/draft-0{draft}/schema#"));
            let id = if draft == "4" { "id" } else { "$id" };
            if draft == "4" { value["id"] = value.as_object_mut().unwrap().remove("$id").unwrap(); }
            if name == "Mutual" {
                value["$ref"] = json!("#MutualA");
                for (key, anchor) in [("A", "#MutualA"), ("B", "#MutualB")] {
                    let child = &mut value["$defs"][key];
                    child.as_object_mut().unwrap().remove("$anchor");
                    child.as_object_mut().unwrap().remove("$id");
                    child[id] = json!(anchor);
                }
                value["$defs"]["A"]["properties"]["peer"]["$ref"] = json!("#MutualB");
                value["$defs"]["B"]["properties"]["children"]["items"]["$ref"] = json!("#MutualA");
                if mode.starts_with("uri") {
                    value["$ref"] = json!("nodes#MutualA");
                    value["$defs"]["A"][id] = json!("nodes#MutualA");
                    value["$defs"]["B"][id] = json!("nodes#MutualB");
                }
                if mode.starts_with("pointer") {
                    value["$ref"] = json!("nodes#%2FMutualA");
                    value["$defs"]["A"][id] = json!("nodes#%2FMutualA");
                    value["$defs"]["B"][id] = json!("nodes#/MutualB");
                    value["$defs"]["A"]["properties"]["peer"]["$ref"] = json!("#/MutualB");
                    value["$defs"]["B"]["properties"]["children"]["items"]["$ref"] = json!("#%2FMutualA");
                }
            }
        }
        if mode == "dynamic" && name == "Tree" {
            value["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
            value["$dynamicAnchor"] = json!("node");
            for pointer in ["/properties/children/items", "/properties/parent/anyOf/0", "/properties/optional"] {
                value.pointer_mut(pointer).unwrap().as_object_mut().unwrap().remove("$ref");
                value.pointer_mut(pointer).unwrap()["$dynamicRef"] = json!("#node");
            }
        }
        if mode == "composed" && name == "Tree" {
            value["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
            value["$dynamicAnchor"] = json!("node");
            value["$defs"] = json!({"NodeConstraints":{"minProperties":1}});
            for pointer in ["/properties/children/items", "/properties/parent/anyOf/0", "/properties/optional"] {
                value.pointer_mut(pointer).unwrap()["$ref"] = json!("#/$defs/NodeConstraints");
                value.pointer_mut(pointer).unwrap()["$dynamicRef"] = json!("#node");
            }
        }
        if mode == "recursive" && name == "Tree" {
            value["$schema"] = json!("https://json-schema.org/draft/2019-09/schema");
            value["$recursiveAnchor"] = json!(true);
            for pointer in ["/properties/children/items", "/properties/parent/anyOf/0", "/properties/optional"] {
                value.pointer_mut(pointer).unwrap().as_object_mut().unwrap().remove("$ref");
                value.pointer_mut(pointer).unwrap()["$recursiveRef"] = json!("#");
            }
        }
        if mode == "legacy" {
            value["$schema"] = json!("http://json-schema.org/draft-04/schema#");
            value["id"] = value.as_object_mut().unwrap().remove("$id").unwrap();
            if name == "Tree" { value["properties"]["label"]["const"] = json!("ignored-legacy-annotation"); }
        }
        if name == "Mutual" && (mode == "legacy" || mode == "mixed") {
            value["$ref"] = json!("nodes/a");
            for key in ["A", "B"] {
                let child = &mut value["$defs"][key];
                child.as_object_mut().unwrap().remove("$anchor");
                if mode == "legacy" || key == "A" {
                    child["$schema"] = json!("http://json-schema.org/draft-04/schema#");
                    child["id"] = child.as_object_mut().unwrap().remove("$id").unwrap();
                }
            }
            value["$defs"]["A"]["properties"]["peer"]["$ref"] = json!("b");
            value["$defs"]["B"]["properties"]["children"]["items"]["$ref"] = json!("a");
        }
        value.to_string()
    } else { schema.to_owned() };
    let name = if std::env::var("FORKLAUNCH_TEST_COMPONENT_NAMES").as_deref() == Ok("1") {
        match name {
            "Tree" => "A B",
            "Forest" => "_fl_encoded_412042",
            "Choice" => "Vec<String>",
            "Mutual" => "Mutual /~水%",
            name => name,
        }
    } else { name };
    BodyContract {
        kind: BodyKind::Json,
        schema: SchemaRef::named(name).with_json_schema(schema),
        content_type: Some("application/json".into()),
    }
}

pub fn contracts() -> Vec<RouteContract> {
    vec![
        RouteContract::get("Read Tree", "Self recursive tree", "/trees")
            .with_response(200, Some(body("Tree", TREE))),
        RouteContract::post("Write Tree", "Recursive request and response", "/trees")
            .with_request_body(body("Tree", TREE))
            .with_response(200, Some(body("Tree", TREE))),
        RouteContract::get("Read Forest", "Recursive arrays", "/forests")
            .with_response(200, Some(body("Forest", FOREST))),
        RouteContract::get("Read Choice", "Recursive object/array union", "/choices")
            .with_response(200, Some(body("Choice", CHOICE))),
        RouteContract::get("Read Mutual", "Mutually recursive records", "/mutual")
            .with_response(200, Some(body("Mutual", MUTUAL))),
    ]
}

pub fn tree() -> Value {
    json!({"label":"水 🦀","children":[{"label":"leaf","children":[],"parent":null}],"parent":null})
}

pub fn register(router: ForkLaunchRouter) -> ForkLaunchRouter {
    let mut routes = contracts().into_iter();
    router
        .get(routes.next().unwrap(), || async { Json(tree()) })
        .post(
            routes.next().unwrap(),
            |Json(value): Json<Value>| async move { Json(value) },
        )
        .get(routes.next().unwrap(), || async { Json(json!([[], [[]]])) })
        .get(routes.next().unwrap(), || async {
            Json(json!({"nested":["leaf",{}]}))
        })
        .get(routes.next().unwrap(), || async {
            Json(json!({"id":"outer","peer":{"children":[{"id":"inner","peer":{"children":[]}}]}}))
        })
}
