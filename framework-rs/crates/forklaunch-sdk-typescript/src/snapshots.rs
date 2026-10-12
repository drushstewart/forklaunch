//! Reviewed output baselines: these catch drift beyond individual field assertions.
use std::{fs, path::Path};

use forklaunch_contract::{
    BodyContract, BodyKind, ContractRegistry, FieldContract, RouteContract, SchemaRef,
    ServiceContract,
};
use forklaunch_sdk_codegen::{SdkGenerationPlan, SdkTarget};
use forklaunch_sdk_rust::{RustSdkOptions, generate_rust_http_sdk_source};
use serde_json::json;

use crate::{TypeScriptHttpSdkOptions, generate_typescript_http_sdk_source};

fn registry() -> ContractRegistry {
    let mut registry = ContractRegistry::for_service(ServiceContract::new("catalog", "1.0.0"));
    let item = BodyContract {
        schema: SchemaRef::named("Item").with_json_schema(
            json!({
                "type":"object", "required":["id","name"], "additionalProperties":false,
                "properties":{
                    "id":{"type":"string"}, "name":{"type":"string","minLength":1},
                    "note":{"type":["string","null"]},
                    "tags":{"type":"array","items":{"type":"string"}}
                }
            })
            .to_string(),
        ),
        ..BodyContract::json("Item")
    };
    registry.register(
        RouteContract::get("Get Item", "Retrieve an item", "/items/:id")
            .with_param(FieldContract::required("id", "String"))
            .with_query(FieldContract::optional("includeDetails", "bool"))
            .with_header(FieldContract::optional("x-request-id", "String"))
            .with_response(200, Some(item.clone()))
            .with_response_header(200, FieldContract::required("etag", "String"))
            .with_response(
                404,
                Some(BodyContract {
                    kind: BodyKind::Text,
                    schema: SchemaRef::named("String"),
                    content_type: Some("text/plain".into()),
                }),
            ),
    );
    registry.register(
        RouteContract::post("Create Item", "Create an item", "/items")
            .with_request_body(item.clone())
            .with_response(201, Some(item)),
    );
    registry.register(
        RouteContract::delete("Delete Item", "Delete an item", "/items/:id")
            .with_param(FieldContract::required("id", "String"))
            .with_response(204, None),
    );
    registry
}

fn assert_snapshot(name: &str, actual: &str) {
    let actual = actual
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/snapshots")
        .join(name);
    if std::env::var_os("FORKLAUNCH_UPDATE_SNAPSHOTS").as_deref() == Some(std::ffi::OsStr::new("1"))
    {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must never rewrite snapshots"
        );
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &actual).unwrap();
    }
    let expected = fs::read_to_string(&path).expect("Missing reviewed snapshot");
    assert!(
        expected == actual,
        "Generated output differs from {}. Review the diff after running FORKLAUNCH_UPDATE_SNAPSHOTS=1 cargo test -p forklaunch-sdk-typescript snapshots",
        path.display()
    );
}

#[test]
fn openapi_and_both_http_sdks_match_reviewed_snapshots() {
    let registry = registry();
    let mut openapi: serde_json::Value =
        serde_json::from_str(&forklaunch_openapi::generate_openapi_json(&registry)).unwrap();
    openapi.sort_all_objects();
    assert_eq!(
        openapi["paths"]["/items/{id}"]["get"]["operationId"],
        "getItem"
    );
    assert!(
        openapi["paths"]["/items/{id}"]["delete"]["responses"]["204"]
            .get("content")
            .is_none()
    );
    assert_snapshot(
        "catalog.openapi.json",
        &(serde_json::to_string_pretty(&openapi).unwrap() + "\n"),
    );
    let rust = generate_rust_http_sdk_source(
        &SdkGenerationPlan::from_registry(SdkTarget::Rust, &registry),
        &RustSdkOptions::new("catalog-sdk"),
    )
    .unwrap();
    let typescript = generate_typescript_http_sdk_source(
        &SdkGenerationPlan::from_registry(SdkTarget::TypeScript, &registry),
        &TypeScriptHttpSdkOptions::new("catalog"),
    )
    .unwrap();
    assert_snapshot("catalog.rust.snap", &rust);
    assert_snapshot("catalog.typescript.snap", &typescript);
}
