//! JSON-lines bridge for the actual Better Auth/Better Call conformance fixture.
use forklaunch_iam_better_auth::{cookies, credentials};
use serde_json::{Value, json};
use std::io::{self, BufRead};

fn main() {
    for line in io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let text = |name: &str| input[name].as_str().unwrap();
        let output = match text("op") {
            "hash" => json!(credentials::hash_password(text("password")).unwrap()),
            "verify" => json!(credentials::verify_password(text("password"), text("hash"))),
            "sign" => json!(cookies::sign_cookie_value(
                text("value"),
                text("secret").as_bytes()
            )),
            "cookie" => {
                let value = cookies::cookie_value(text("header"), text("name"));
                json!({
                    "value": value,
                    "token": value.as_deref().and_then(|value| cookies::verify_cookie_value(value, text("secret").as_bytes()))
                })
            }
            "serialize" => {
                let cookie = cookies::SessionCookie {
                    secure: input["secure"].as_bool().unwrap(),
                };
                json!({
                    "name": cookie.name(),
                    "header": cookie.serialize(text("value"), text("secret").as_bytes(), input["persistent"].as_bool().unwrap()),
                    "clear": cookie.clear()
                })
            }
            operation => panic!("unknown operation {operation}"),
        };
        println!("{output}");
    }
}
