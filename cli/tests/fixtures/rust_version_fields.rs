use axum::{Extension, Json, extract::OriginalUri, http::HeaderMap};
use forklaunch_axum::{ForkLaunchRouter, VersionedHeaders, VersionedParams, VersionedQuery};
use forklaunch_contract::{
    BodyContract, FieldContract, RequestContract, ResponseContract, RouteContract,
    VersionedContract,
};
use forklaunch_http::{
    ContractValidationLocation as Location, SelectedVersion, VersionBodyHooksRegistry,
    VersionFieldHooks,
};
use serde_json::{Value, json};

fn field(name: &str, schema: Value) -> FieldContract {
    let mut field = FieldContract::optional(name, format!("Field_{name}"));
    field.schema.json_schema = Some(schema.to_string());
    field
}
pub fn contracts() -> Vec<RouteContract> {
    let mut id = field("id", json!({"type":"string","minLength":1}));
    id.required = true;
    let mut route =
        RouteContract::get("Read Fields", "Versioned fields", "/fields/:id").with_param(id);
    for (version, minimum) in [("first", 0), ("last", 10)] {
        let mut response = BodyContract::json("FieldsResult");
        response.schema.json_schema = Some(json!({"type":"object"}).to_string());
        let mut count = field("count", json!({"type":"integer","minimum":minimum}));
        count.schema.name = format!("{version}Count");
        route = route.with_version(VersionedContract {
            version: version.into(),
            request: RequestContract {
                query: vec![count],
                headers: vec![field("x-code", json!({"type":"integer","minimum":0}))],
                ..Default::default()
            },
            responses: vec![ResponseContract::new(200, Some(response))],
        });
    }
    vec![route]
}
pub fn register(router: ForkLaunchRouter) -> ForkLaunchRouter {
    let route = contracts().remove(0);
    let mut hooks = VersionBodyHooksRegistry::default();
    for version in ["first", "last"] {
        let last = version == "last";
        hooks
            .insert_field(
                &route,
                version,
                Location::PathParameter,
                "id",
                VersionFieldHooks::default()
                    .with_preprocess(|value| Ok(json!(value.as_str().ok_or("string")?.trim())))
                    .with_transform(|value| {
                        Ok(json!(value.as_str().ok_or("string")?.to_uppercase()))
                    }),
            )
            .unwrap();
        hooks
            .insert_field(
                &route,
                version,
                Location::Query,
                "count",
                VersionFieldHooks::default()
                    .with_optional_preprocess(move |value| {
                        Ok(Some(json!(match value {
                            None =>
                                if last {
                                    10
                                } else {
                                    5
                                },
                            Some(value) => value
                                .as_str()
                                .ok_or("number")?
                                .parse::<i64>()
                                .map_err(|_| "number")?,
                        })))
                    })
                    .with_optional_transform(move |value| {
                        let number = value.and_then(Value::as_i64).ok_or("number")?;
                        if last && number == 13 {
                            return Err("private transform detail".into());
                        }
                        Ok(if number == 42 {
                            None
                        } else {
                            Some(if number == 0 {
                                Value::Null
                            } else {
                                json!(number + if last { 100 } else { 0 })
                            })
                        })
                    }),
            )
            .unwrap();
        hooks
            .insert_field(
                &route,
                version,
                Location::Header,
                "X-Code",
                VersionFieldHooks::default()
                    .with_optional_preprocess(|value| {
                        Ok(Some(json!(match value {
                            None => 0,
                            Some(value) => value
                                .as_str()
                                .ok_or("header")?
                                .parse::<i64>()
                                .map_err(|_| "header")?,
                        })))
                    })
                    .with_transform(|value| {
                        let number = value.as_i64().ok_or("header")?;
                        Ok(if number == 0 {
                            json!(false)
                        } else {
                            json!(number * 2)
                        })
                    }),
            )
            .unwrap();
    }
    router.with_version_body_hooks(hooks).get(
        route,
        |Extension(selected): Extension<SelectedVersion>,
         VersionedParams(params): VersionedParams<Value>,
         VersionedQuery(query): VersionedQuery<Value>,
         VersionedHeaders(headers): VersionedHeaders<Value>,
         OriginalUri(uri): OriginalUri,
         wire: HeaderMap| async move {
            Json(
                json!({"version":selected.version,"id":params["id"],"count":query.get("count"),
            "countPresent":query.get("count").is_some(),"header":headers.get("x-code"),
            "headerPresent":headers.get("x-code").is_some(),"rawQuery":uri.query().unwrap_or(""),
            "rawHeader":wire.get("x-code").and_then(|value|value.to_str().ok())}),
            )
        },
    )
}
