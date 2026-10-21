//! Adapt the v0.1 native export protocol to the portable SDK document format.
use anyhow::{Context, Result};
use forklaunch_core::{
    codegen::{SdkGenerationPlan, SdkTarget},
    typescript::{TypeScriptHttpSdkOptions, generate_typescript_operation_descriptors},
};
use serde_json::Value;

pub(super) fn enrich(document: &mut Value, artifact: &Value, service: &str) -> Result<()> {
    let mut public = artifact.clone();
    public["routes"]
        .as_array_mut()
        .context("Contract routes missing")?
        .retain(|route| route["options"]["openapi"] != false);
    let plan = SdkGenerationPlan::from_artifact_json(
        SdkTarget::TypeScript,
        &serde_json::to_string(&public)?,
    )?;
    let descriptors =
        generate_typescript_operation_descriptors(&plan, &TypeScriptHttpSdkOptions::new(service))?;
    forklaunch_core::openapi::attach_portable_operation_descriptors(document, &descriptors)
}

/// Custom TypeScript exporters share the ordinary build compiler's schema and
/// operation normalization before exposing their document to the public SDK.
pub(super) fn enrich_imported(document: &mut Value, service: &str) -> Result<()> {
    let imported = forklaunch_core::build::openapi_artifact_sources(
        &serde_json::to_string(document)?,
        service,
    )?;
    enrich(
        document,
        &serde_json::from_str(&imported.contract)?,
        service,
    )
}

#[cfg(test)]
mod tests {
    use forklaunch_core::contract::{ContractRegistry, RouteContract, ServiceContract};

    use super::*;

    #[test]
    fn typescript_hook_exports_share_the_build_compiler_and_reject_forged_descriptors() {
        let mut document = serde_json::json!({
            "openapi":"3.1.0", "info":{"title":"Notes","version":"1.0.0"},
            "paths":{"/notes/{id}":{"get":{
                "operationId":"getNote", "tags":["notes"],
                "parameters":[{"in":"path","name":"id","required":true,"schema":{"type":"string"}}],
                "responses":{"200":{"description":"Note","content":{"application/json":{"schema":{
                    "type":"object","required":["title"],"properties":{"title":{"type":"string"}},"additionalProperties":false
                }}}}}
            }}}
        });
        let original = document.clone();
        enrich_imported(&mut document, "notes").unwrap();
        let descriptor = &document["paths"]["/notes/{id}"]["get"]["x-forklaunch-operation"];
        assert_eq!(descriptor["path"], "/notes/{id}");
        assert_eq!(descriptor["method"], "GET");
        assert_eq!(
            document["paths"]["/notes/{id}"]["get"]["responses"],
            original["paths"]["/notes/{id}"]["get"]["responses"]
        );
        let once = document.clone();
        enrich_imported(&mut document, "notes").unwrap();
        assert_eq!(document, once);
        document["paths"]["/notes/{id}"]["get"]["x-forklaunch-operation"]["path"] =
            "/forged".into();
        assert!(enrich_imported(&mut document, "notes").is_err());
    }

    #[test]
    fn versioned_exports_match_compile_time_descriptors_and_reject_forgery() {
        use forklaunch_core::contract::{
            BodyContract, RequestContract, ResponseContract, VersionedContract,
        };
        let mut registry = ContractRegistry::for_service(ServiceContract::new("notes", "1.0.0"));
        let mut route = RouteContract::post("Write", "Versioned note", "/notes");
        for (label, kind, status) in [("v1", "string", 200), ("v2", "integer", 201)] {
            let mut body = BodyContract::json("Value");
            body.schema = body.schema.with_json_schema(serde_json::json!({"type": "object", "properties": {"value": {"type": kind}}, "required": ["value"]}).to_string());
            route = route.with_version(VersionedContract {
                version: label.into(),
                request: RequestContract::default().with_body(body.clone()),
                responses: vec![ResponseContract::new(status, Some(body))],
            });
        }
        registry.register(route);
        let artifact = serde_json::from_str(&registry.clone().into_artifact().to_json()).unwrap();
        let mut document =
            serde_json::from_str(&forklaunch_core::openapi::generate_openapi_json(&registry))
                .unwrap();
        enrich(&mut document, &artifact, "notes").unwrap();
        let expected: Value = serde_json::from_str(
            &forklaunch_core::openapi::generate_portable_openapi_json(&registry).unwrap(),
        )
        .unwrap();
        assert_eq!(document, expected);
        let variants = &document["paths"]["/notes"]["post"]["x-forklaunch-operations"];
        assert_eq!(variants.as_array().unwrap().len(), 2);
        assert_eq!(variants[0]["operation"], "writeVersionV1");
        assert_eq!(variants[1]["operation"], "writeVersionV2");
        enrich(&mut document, &artifact, "notes").unwrap();
        document["paths"]["/notes"]["post"]["x-forklaunch-operations"][0]["path"] =
            "/forged".into();
        assert!(enrich(&mut document, &artifact, "notes").is_err());
    }

    #[test]
    fn exported_documents_match_compile_descriptors_without_exposing_private_routes() {
        let mut registry = ContractRegistry::for_service(ServiceContract::new("notes", "1.0.0"));
        registry.register(RouteContract::get("Get Note", "Read a note", "/notes/:id"));
        let mut private = RouteContract::get("Internal", "Private", "/private");
        private.options.openapi = false;
        registry.register(private);
        let artifact = serde_json::from_str(&registry.clone().into_artifact().to_json()).unwrap();
        let mut document: Value =
            serde_json::from_str(&forklaunch_core::openapi::generate_openapi_json(&registry))
                .unwrap();
        enrich(&mut document, &artifact, "notes").unwrap();
        let expected: Value = serde_json::from_str(
            &forklaunch_core::openapi::generate_portable_openapi_json(&registry).unwrap(),
        )
        .unwrap();
        assert_eq!(document, expected);
        assert!(document["paths"].get("/private").is_none());
        enrich(&mut document, &artifact, "notes").unwrap();
        document["paths"]["/notes/{id}"]["get"]["x-forklaunch-operation"]["path"] =
            "/forged".into();
        assert!(enrich(&mut document, &artifact, "notes").is_err());
    }
}
