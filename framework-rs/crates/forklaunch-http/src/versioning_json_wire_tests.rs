use super::*;
use crate::{Body, FrameworkRequest, HttpMethod, RequestContext};
use forklaunch_contract::{
    BodyContract, RequestContract, ResponseContract, RouteContract, VersionedContract,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
fn route() -> RouteContract {
    let mut route = RouteContract::post("Wire", "Wire", "/wire");
    for version in ["first", "last"] {
        let mut body = BodyContract::json("Input");
        body.schema = body.schema.with_json_schema(
            json!({"type":"object","required":["flag"],"properties":{"flag":{"const":"ok"}}})
                .to_string(),
        );
        route = route.with_version(VersionedContract {
            version: version.into(),
            request: RequestContract::default().with_body(body),
            responses: vec![ResponseContract::new(200, None)],
        });
    }
    route
}
fn request(media: &str, body: &str, framed: bool) -> FrameworkRequest {
    let mut request = FrameworkRequest::new(HttpMethod::Post, "/wire", RequestContext::new("wire"))
        .with_body(Body::Text(body.into()));
    request.headers.insert("content-type".into(), media.into());
    if framed {
        request
            .headers
            .insert("content-length".into(), body.len().to_string());
    }
    request
}
#[test]
fn json_wire_skips_before_callbacks_and_defaults_without_changing_raw_bytes() {
    let route = route();
    let mut hooks = VersionBodyHooksRegistry::default();
    hooks.enable_typescript_json_wire(&route).unwrap();
    let verifies = Arc::new(AtomicUsize::new(0));
    let count = verifies.clone();
    hooks
        .insert_parser(
            &route,
            BodyParserHooks::default().with_verify(move |_, _, charset| {
                assert_eq!(charset, Some("utf-8"));
                count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
        )
        .unwrap();
    for version in ["first", "last"] {
        hooks
            .insert(
                &route,
                version,
                VersionBodyHooks::default().with_optional_preprocess(|value| {
                    Ok(Some(value.cloned().unwrap_or_else(|| json!({"flag":"ok"}))))
                }),
            )
            .unwrap();
    }
    let plan = hooks.compile(&route).unwrap();
    for (media, body, framed, expected_verify) in [
        ("text/plain", "{", true, 0),
        ("application/problem+json", "{", true, 0),
        ("application/json", "{", false, 0),
        ("application/json", r#"{"flag":"ok"}"#, true, 1),
    ] {
        let request = request(media, body, framed);
        let original = request.body.clone();
        let before = verifies.load(Ordering::SeqCst);
        let (_, selected) = plan.select(&request).unwrap().unwrap();
        assert_eq!(selected.matched_versions, ["first", "last"]);
        assert_eq!(selected.version, "last");
        assert_eq!(selected.parsed_body.as_deref(), Some(&json!({"flag":"ok"})));
        assert_eq!(request.body, original);
        assert_eq!(verifies.load(Ordering::SeqCst) - before, expected_verify);
    }
    assert!(
        plan.select(&request("application/json", "{", true))
            .unwrap_err()
            .iter()
            .all(ContractValidationError::is_parser_error)
    );
    assert!(plan.select(&request("application/json", "", true)).is_err());
    // Native default still parses wire bytes regardless of framing/media.
    assert!(
        VersionedRoutePlan::compile(&route)
            .unwrap()
            .select(&request("text/plain", "{", true))
            .is_err()
    );
}
#[test]
fn json_wire_registration_is_pinned_and_prepared_selection_does_not_repeat_verifier() {
    let route = route();
    let mut hooks = VersionBodyHooksRegistry::default();
    hooks.enable_typescript_json_wire(&route).unwrap();
    assert!(hooks.enable_typescript_json_wire(&route).is_err());
    assert!(
        VersionBodyHooksRegistry::default()
            .enable_typescript_json_wire(&RouteContract::get("No", "No", "/no"))
            .is_err()
    );
    assert!(hooks.validate_routes(&[]).is_err());
    let mut changed = route.clone();
    changed.options.body_parser.json_strict = false;
    assert!(hooks.compile(&changed).is_err());
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    hooks
        .insert_parser(
            &route,
            BodyParserHooks::default().with_verify(move |_, _, _| {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
        )
        .unwrap();
    let plan = hooks.compile(&route).unwrap();
    let mut request = request("application/json", r#"{"flag":"ok"}"#, true);
    let prepared = plan.prepare_selection(&request, MultipartLimits::default());
    plan.select_reusing(&mut request, Some(&prepared))
        .unwrap()
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn prepared_selection_is_invalidated_by_consuming_plan_mutators() {
    let mut route = route();
    for version in route.versions.values_mut() {
        version
            .request
            .query
            .push(forklaunch_contract::FieldContract::optional(
                "query", "String",
            ));
    }
    let plan = VersionedRoutePlan::compile(&route).unwrap();
    let request = request("application/json", r#"{"flag":"ok"}"#, true);
    let prepared = plan.prepare_selection(&request, MultipartLimits::default());
    assert!(plan.clone().resolve_prepared(&prepared, &request).is_ok());
    for changed in [
        plan.clone()
            .with_body_hooks("first", VersionBodyHooks::default())
            .unwrap(),
        plan.clone()
            .with_parser_hooks(BodyParserHooks::default())
            .unwrap(),
        plan.clone()
            .with_field_hooks(
                "first",
                ContractValidationLocation::Query,
                "query",
                VersionFieldHooks::default(),
            )
            .unwrap(),
        plan.with_typescript_json_wire().unwrap(),
    ] {
        assert!(changed.resolve_prepared(&prepared, &request).is_err());
    }
    let mut registry = VersionBodyHooksRegistry::default();
    registry.enable_typescript_json_wire(&route).unwrap();
    let plan = registry.compile(&route).unwrap();
    let prepared = plan.prepare_selection(&request, MultipartLimits::default());
    let changed = plan
        .with_parser_hooks(
            BodyParserHooks::default().with_verify(|_, _, _| Err("REPLACED_VERIFIER".into())),
        )
        .unwrap();
    assert!(changed.resolve_prepared(&prepared, &request).is_err());
    assert!(changed.select(&request).is_err());
}

#[test]
fn json_wire_rejects_unmatched_projection_modes_including_stale_registrations() {
    let original = route();
    let mut registry = VersionBodyHooksRegistry::default();
    registry.enable_typescript_json_wire(&original).unwrap();
    for mode in [
        forklaunch_contract::ValidationMode::None,
        forklaunch_contract::ValidationMode::Warning,
    ] {
        let mut changed = original.clone();
        changed.options.request_validation = mode;
        assert!(
            VersionBodyHooksRegistry::default()
                .enable_typescript_json_wire(&changed)
                .is_err()
        );
        assert!(registry.compile(&changed).is_err());
    }
}
