use axum::Json;
use forklaunch_axum::ForkLaunchRouter;
use forklaunch_contract::{BodyContract, BodyKind, RouteContract, SchemaRef};
use serde_json::{Value, json};

const TREE: &str = r##"{"type":"object","additionalProperties":false,"required":["label","children","parent"],"properties":{"label":{"type":"string"},"children":{"type":"array","items":{"$ref":"#"}},"parent":{"anyOf":[{"$ref":"#"},{"type":"null"}]},"optional":{"$ref":"#"}}}"##;
const FOREST: &str = r##"{"type":"array","items":{"$ref":"#"}}"##;
const CHOICE: &str = r##"{"anyOf":[{"type":"string"},{"type":"array","items":{"$ref":"#"}},{"type":"object","additionalProperties":{"$ref":"#"}}]}"##;
const MUTUAL: &str = r##"{"$ref":"#/$defs/A","$defs":{"A":{"type":"object","additionalProperties":false,"required":["id","peer"],"properties":{"id":{"type":"string"},"peer":{"$ref":"#/$defs/B"}}},"B":{"type":"object","additionalProperties":false,"required":["children"],"properties":{"children":{"type":"array","items":{"$ref":"#/$defs/A"}}}}}}"##;

fn body(name: &str, schema: &str) -> BodyContract {
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
