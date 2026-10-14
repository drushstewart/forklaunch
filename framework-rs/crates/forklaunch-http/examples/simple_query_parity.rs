//! JSON-line bridge for the actual Node/default Express query oracle.
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let input: Option<String> =
            serde_json::from_str(&line.expect("stdin line")).expect("JSON input");
        println!(
            "{}",
            serde_json::to_string(&forklaunch_http::parse_typescript_simple_query(
                input.as_deref()
            ))
            .expect("JSON projection")
        );
    }
}
