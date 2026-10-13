use forklaunch_validation::{ParseError, pretty_print_parse_errors};
use serde::Deserialize;
use std::io::{self, BufRead};

#[derive(Deserialize)]
struct Case {
    errors: Option<Vec<ParseError>>,
    prefix: Option<String>,
}

fn main() {
    for line in io::stdin().lock().lines() {
        let case: Case = serde_json::from_str(&line.unwrap()).unwrap();
        println!(
            "{}",
            serde_json::to_string(&pretty_print_parse_errors(
                case.errors.as_deref(),
                case.prefix.as_deref()
            ))
            .unwrap()
        );
    }
}
