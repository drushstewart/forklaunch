//! Conformance bridge for the real HTTP version matcher.
use forklaunch_contract::{
    BodyContract, RequestContract, ResponseContract, RouteContract, VersionedContract,
};
use forklaunch_http::{Body, FrameworkRequest, HttpMethod, RequestContext, VersionedRoutePlan};
use serde_json::{Value, json};
use std::io::{self, Read};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let fixtures: Vec<Value> = serde_json::from_str(&input)?;
    let mut output = Vec::new();
    for fixture in fixtures {
        let mut contract = RouteContract::post("Versioned", "Versioned", "/versioned");
        for version in fixture["versions"].as_array().unwrap() {
            let mut body = BodyContract::json("Input");
            body.schema = body.schema.with_json_schema(version["schema"].to_string());
            contract = contract.with_version(VersionedContract {
                version: version["name"].as_str().unwrap().into(),
                request: RequestContract::default().with_body(body),
                responses: vec![ResponseContract::new(200, None)],
            });
        }
        let plan = VersionedRoutePlan::compile(&contract)?;
        let mut cases = Vec::new();
        for case in fixture["cases"].as_array().unwrap() {
            let request = FrameworkRequest::new(
                HttpMethod::Post,
                "/versioned",
                RequestContext::new("fixture"),
            )
            .with_header("content-type", "application/json")
            .with_body(Body::Text(case.to_string()));
            cases.push(match plan.select(&request) {
                Ok(Some((_,version)))=>json!({"version":version.version,"matched":version.matched_versions,"status":200}),
                _=>json!({"status":400})
            });
        }
        output.push(json!({"order":contract.versions.keys().collect::<Vec<_>>(),"cases":cases}));
    }
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
