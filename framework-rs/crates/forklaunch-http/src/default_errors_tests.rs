use super::*;
use crate::{
    FrameworkRequest, LocalRouter, ParserErrorFormat, RequestContext, ResponseStatusErrorFormat,
};
use forklaunch_contract::{HttpMethod, RequestContract, VersionedContract};
fn route() -> RouteContract {
    RouteContract::get("Default", "Default", "/default").with_response(200, None)
}
fn profiles(route: &RouteContract) -> DefaultErrorResponses {
    let mut profiles = DefaultErrorResponses::default();
    profiles
        .insert(route, &[404, 500], 128, Duration::from_millis(50))
        .unwrap();
    profiles
}
#[test]
fn registrations_are_explicit_pinned_and_projected() {
    for statuses in [vec![], vec![200], vec![418], vec![404, 404]] {
        assert!(
            DefaultErrorResponses::default()
                .insert(&route(), &statuses, 10, Duration::from_secs(1))
                .is_err()
        );
    }
    for (bytes, time) in [(0, Duration::from_secs(1)), (1, Duration::ZERO)] {
        assert!(
            DefaultErrorResponses::default()
                .insert(&route(), &[404], bytes, time)
                .is_err()
        );
    }
    let versioned = route().with_version(VersionedContract {
        version: "v1".into(),
        request: RequestContract::default(),
        responses: vec![],
    });
    assert!(
        DefaultErrorResponses::default()
            .insert(&versioned, &[404], 10, Duration::from_secs(1))
            .is_err()
    );
    let original = route();
    let profiles = profiles(&original);
    assert!(profiles.validate_routes(&[]).is_err());
    assert!(
        profiles
            .validate_routes(&[original.clone(), original.clone()])
            .is_err()
    );
    assert!(
        profiles
            .validate_route(&original.clone().with_response(404, None))
            .is_err()
    );
    let mut registry = ContractRegistry::new();
    registry.register(original);
    let projected = profiles.project(&registry).unwrap().unwrap();
    assert_eq!(registry.routes()[0].responses.len(), 1);
    assert_eq!(projected.routes()[0].responses.len(), 3);
    assert!(projected.into_artifact().to_json().contains("text/plain"));
}
#[test]
fn local_selected_errors_accept_strings_and_empty_but_never_arbitrary_json() {
    for response in [
        FrameworkResponse::text(404, "value"),
        FrameworkResponse::json(500, "\"value\""),
        FrameworkResponse::text(500, ""),
        FrameworkResponse::new(500),
    ] {
        let contract = route();
        let profiles = profiles(&contract);
        let original = response.clone();
        let mut local = LocalRouter::new();
        local.register(contract, move |_: FrameworkRequest| Ok(response.clone()));
        let result = local
            .execute_with_response_profiles(
                FrameworkRequest::new(HttpMethod::Get, "/default", RequestContext::new("test")),
                ParserErrorFormat::Json,
                ResponseStatusErrorFormat::Json,
                &profiles,
            )
            .unwrap();
        assert_eq!(result.status_code, original.status_code);
        if original.body.is_empty() {
            assert!(result.body.is_empty());
        } else {
            assert_eq!(result.headers["content-type"], "text/plain; charset=utf-8");
            assert_eq!(
                result.body.as_text(),
                Some(if original.body.as_text() == Some("") {
                    ""
                } else {
                    "value"
                })
            );
        }
    }
    for (input, received) in [
        ("{}", "object"),
        ("[]", "array"),
        ("null", "null"),
        ("true", "boolean"),
        ("42", "number"),
    ] {
        let result = profiles(&route())
            .normalize(&route(), FrameworkResponse::json(404, input), "test")
            .unwrap_err();
        assert_eq!(result.status_code, 500);
        assert!(
            result
                .body
                .as_text()
                .unwrap()
                .contains(&format!("received {received}"))
        );
    }
    let original = FrameworkResponse::json(200, "{}");
    assert_eq!(
        profiles(&route())
            .normalize(&route(), original.clone(), "test")
            .unwrap(),
        original
    );
    for mode in [ValidationMode::Warning, ValidationMode::None] {
        let mut contract = route();
        contract.options.response_validation = mode;
        let original = FrameworkResponse::json(500, "{}");
        assert_eq!(
            profiles(&contract)
                .normalize(&contract, original.clone(), "test")
                .unwrap(),
            original
        );
    }
}

#[test]
fn direct_local_framework_errors_obey_selected_returned_response_media() {
    let contract = route();
    let profiles = profiles(&contract);
    let mut local = LocalRouter::new();
    local.register(contract, |_: FrameworkRequest| {
        Ok(FrameworkResponse::text(201, "PRIVATE"))
    });
    let response = local
        .execute_with_response_profiles(
            FrameworkRequest::new(
                HttpMethod::Get,
                "/default",
                RequestContext::new("direct-test"),
            ),
            ParserErrorFormat::Json,
            ResponseStatusErrorFormat::Json,
            &profiles,
        )
        .unwrap();
    assert_eq!(response.status_code, 500);
    assert_eq!(
        response.headers["content-type"],
        "text/plain; charset=utf-8"
    );
    assert!(!response.body.as_text().unwrap().contains("PRIVATE"));
    let contract = RouteContract::get("Uri", "Uri", "/:id")
        .with_param(forklaunch_contract::FieldContract::required("id", "String"))
        .with_response(200, None);
    let mut profiles = DefaultErrorResponses::default();
    profiles
        .insert(&contract, &[400, 500], 1024, Duration::from_secs(1))
        .unwrap();
    let mut local = LocalRouter::new();
    local.register(contract, |_: FrameworkRequest| {
        Ok(FrameworkResponse::text(200, "handler"))
    });
    let response = local
        .execute_with_response_profiles(
            FrameworkRequest::new(HttpMethod::Get, "/%GG", RequestContext::new("direct-test")),
            ParserErrorFormat::TypeScriptText,
            ResponseStatusErrorFormat::Json,
            &profiles,
        )
        .unwrap();
    assert_eq!(
        response.headers["content-type"],
        "text/plain; charset=utf-8"
    );
    // Direct local URI handling still predates the Express pre-context profile;
    // this proves selected status/envelope admission, not pre-context correlation.
    assert_eq!(response.status_code, 400);
}
