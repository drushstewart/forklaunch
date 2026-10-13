//! Conformance bridge for the real HTTP version matcher.
use forklaunch_contract::{
    BodyContract, BodyKind, RequestContract, ResponseContract, RouteContract, VersionedContract,
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
        if let Some(kinds) = fixture["parserKinds"].as_array() {
            output.push(parser_registration(kinds));
            continue;
        }
        let text = fixture["kind"] == "text";
        let file = fixture["kind"] == "file";
        let form = fixture["kind"] == "form";
        let multipart = fixture["kind"] == "multipart";
        for version in fixture["versions"].as_array().unwrap() {
            let mut body = BodyContract::json("Input");
            if text {
                body.kind = BodyKind::Text;
                body.content_type = Some("text/plain".into());
            }
            if file {
                body.kind = BodyKind::File;
                body.content_type = Some("application/octet-stream".into());
            }
            if form {
                body.kind = BodyKind::UrlEncoded;
                body.content_type = Some("application/x-www-form-urlencoded".into());
            }
            if multipart {
                body.kind = BodyKind::Multipart;
                body.content_type = Some("multipart/form-data".into());
            }
            body.schema = body.schema.with_json_schema(version["schema"].to_string());
            contract = contract.with_version(VersionedContract {
                version: version["name"].as_str().unwrap().into(),
                request: RequestContract::default().with_body(body),
                responses: vec![ResponseContract::new(200, None)],
            });
        }
        let plan = VersionedRoutePlan::compile(&contract)
            .map_err(|error| format!("fixture {}: {error}", output.len()))?;
        let mut cases = Vec::new();
        for case in fixture["cases"].as_array().unwrap() {
            let request = FrameworkRequest::new(
                HttpMethod::Post,
                "/versioned",
                RequestContext::new("fixture"),
            )
            .with_header(
                "content-type",
                if multipart {
                    "multipart/form-data; boundary=boundary"
                } else if form {
                    "application/x-www-form-urlencoded"
                } else if file {
                    "application/octet-stream"
                } else if text {
                    "text/plain"
                } else {
                    "application/json"
                },
            )
            .with_body(if file || multipart {
                Body::Binary(
                    case.as_array()
                        .unwrap()
                        .iter()
                        .map(|v| u8::try_from(v.as_u64().unwrap()).unwrap())
                        .collect(),
                )
            } else {
                Body::Text(if text || form {
                    case.as_str().unwrap().into()
                } else {
                    case.to_string()
                })
            });
            cases.push(match plan.select(&request) {
                Ok(Some((_,version)))=>json!({"version":version.version,"matched":version.matched_versions,"status":200}),
                Err(errors)=>json!({"status":forklaunch_http::validation_error_response(&errors,"fixture").status_code}),
                _=>json!({"status":400})
            });
        }
        output.push(json!({"order":contract.versions.keys().collect::<Vec<_>>(),"cases":cases}));
    }
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn parser_registration(kinds: &[Value]) -> Value {
    let mut contract = RouteContract::post("Versioned", "Versioned", "/versioned");
    for (index, kind) in kinds.iter().enumerate() {
        let kind = match kind.as_str().unwrap() {
            "none" => None,
            "json" => Some(BodyKind::Json),
            "text" => Some(BodyKind::Text),
            "file" => Some(BodyKind::File),
            "binary" => Some(BodyKind::Binary),
            "urlEncoded" => Some(BodyKind::UrlEncoded),
            "multipart" => Some(BodyKind::Multipart),
            _ => unreachable!(),
        };
        let body = kind.map(|kind| BodyContract {
            kind,
            ..BodyContract::json("Input")
        });
        contract = contract.with_version(VersionedContract {
            version: format!("v{index}"),
            request: RequestContract {
                body,
                ..Default::default()
            },
            responses: vec![],
        });
    }
    json!({"accepted":VersionedRoutePlan::compile(&contract).is_ok()})
}
