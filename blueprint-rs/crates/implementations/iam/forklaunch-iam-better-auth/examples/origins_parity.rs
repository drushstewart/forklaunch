//! Actual-library origin/CSRF matcher bridge; no network or credentials.
use forklaunch_iam_better_auth::origins::{OriginPolicy, RequestOrigins, matches_origin};
use serde_json::{Value, json};
use std::io::{self, Read};
fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    let cases: Vec<Value> = serde_json::from_str(&input).unwrap();
    let results: Vec<_> = cases
        .iter()
        .map(|case| {
            if case["op"] == "guard" {
                let headers = &case["headers"];
                let request = RequestOrigins {
                    cookie_present: headers.get("cookie").is_some(),
                    origin: headers["origin"].as_str(),
                    referer: headers["referer"].as_str(),
                    fetch_site: headers["sec-fetch-site"].as_str(),
                    fetch_mode: headers["sec-fetch-mode"].as_str(),
                    fetch_dest: headers["sec-fetch-dest"].as_str(),
                    request_origin: case["requestOrigin"]
                        .as_str()
                        .unwrap_or("http://localhost:3000"),
                };
                let policy = OriginPolicy::new("http://localhost:3000", vec![]).unwrap();
                match policy.mutation(&request, case["login"].as_bool().unwrap(), &case["body"]) {
                    Ok(()) => json!("allowed"),
                    Err(error) => json!(error.code),
                }
            } else {
                json!(matches_origin(
                    case["value"].as_str().unwrap(),
                    case["pattern"].as_str().unwrap(),
                    case["relative"].as_bool().unwrap()
                ))
            }
        })
        .collect();
    println!("{}", serde_json::to_string(&results).unwrap());
}
