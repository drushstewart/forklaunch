use axum::Json;
use forklaunch_axum::ForkLaunchRouter;
use forklaunch_contract::{BodyContract, BodyKind, RouteContract, SchemaRef};
use serde_json::{Value, json};

const TREE: &str = r##"{"type":"object","additionalProperties":false,"required":["label","children","parent"],"properties":{"label":{"type":"string"},"children":{"type":"array","items":{"$ref":"#"}},"parent":{"anyOf":[{"$ref":"#"},{"type":"null"}]},"optional":{"$ref":"#"},"metadata":{"type":"object"}},"examples":[{"label":"literal","children":[],"parent":null,"metadata":{"$ref":"#"}}]}"##;
const FOREST: &str = r##"{"type":"array","items":{"$ref":"#"}}"##;
const CHOICE: &str = r##"{"anyOf":[{"type":"string"},{"type":"array","items":{"$ref":"#"}},{"type":"object","additionalProperties":{"$ref":"#"}}]}"##;
const MUTUAL: &str = r##"{"$ref":"#MutualA","$defs":{"A":{"$anchor":"MutualA","type":"object","additionalProperties":false,"required":["id","peer"],"properties":{"id":{"type":"string"},"peer":{"$ref":"#MutualB"}}},"B":{"$anchor":"MutualB","type":"object","additionalProperties":false,"required":["children"],"properties":{"children":{"type":"array","items":{"$ref":"#MutualA"}}}}}}"##;

fn body(name: &str, schema: &str) -> BodyContract {
    let schema = if std::env::var_os("FORKLAUNCH_TEST_SCHEMA_RESOURCES").is_some() {
        let mut value: Value = serde_json::from_str(schema).unwrap();
        value["$id"] = json!(format!("https://schemas.fixture.test/{name}/index.json"));
        if name == "Mutual" {
            value["$ref"] = json!("nodes/a#MutualA");
            value["$defs"]["A"]["$id"] = json!("nodes/a");
            value["$defs"]["B"]["$id"] = json!("nodes/b");
            value["$defs"]["A"]["properties"]["peer"]["$ref"] = json!("b#MutualB");
            value["$defs"]["B"]["properties"]["children"]["items"]["$ref"] = json!("a#MutualA");
        }
        value.to_string()
    } else { schema.to_owned() };
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
