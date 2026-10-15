//! Streaming case-transformation oracle; keeps the full-scalar corpus bounded.
use forklaunch_validation::typescript_coerce::{
    ZodJsonOutcome, ZodJsonSchema, ZodJsonStringCheck, ZodStringCheck, ZodStringRule,
    ZodStringStep, zod_json,
};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
fn checks(input: &Value) -> Vec<ZodJsonStringCheck<'_>> {
    input["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|check| match check["name"].as_str().unwrap() {
            "toLowerCase" => ZodJsonStringCheck::Lowercase,
            "toUpperCase" => ZodJsonStringCheck::Uppercase,
            name => ZodJsonStringCheck::Scalar(ZodStringCheck {
                message: check["message"].as_str(),
                rule: match name {
                    "trim" => ZodStringRule::Step(ZodStringStep::Trim),
                    "min" => ZodStringRule::Step(ZodStringStep::Min(
                        u32::try_from(check["argument"].as_u64().unwrap()).unwrap(),
                    )),
                    "max" => ZodStringRule::Step(ZodStringStep::Max(
                        u32::try_from(check["argument"].as_u64().unwrap()).unwrap(),
                    )),
                    "length" => ZodStringRule::Step(ZodStringStep::Length(
                        u32::try_from(check["argument"].as_u64().unwrap()).unwrap(),
                    )),
                    "startsWith" => ZodStringRule::StartsWith(check["argument"].as_str().unwrap()),
                    "endsWith" => ZodStringRule::EndsWith(check["argument"].as_str().unwrap()),
                    _ => panic!("unknown check"),
                },
            }),
        })
        .collect()
}
fn convert(input: &Value, schema: &ZodJsonSchema<'_>) -> Value {
    let ZodJsonOutcome::Valid(Some(value)) = zod_json(Some(input), schema).unwrap() else {
        panic!("valid scalar")
    };
    value
}
fn main() {
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    for line in io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line.unwrap()).unwrap();
        if input["scalars"] == true {
            let lower = ZodJsonSchema::CheckedString(vec![ZodJsonStringCheck::Lowercase]);
            let upper = ZodJsonSchema::CheckedString(vec![ZodJsonStringCheck::Uppercase]);
            for cp in 0..=0x0010_ffff {
                if let Some(ch) = char::from_u32(cp) {
                    let value = json!(ch.to_string());
                    writeln!(
                        output,
                        "{}",
                        json!([cp, convert(&value, &lower), convert(&value, &upper)])
                    )
                    .unwrap();
                }
            }
        } else {
            let schema = ZodJsonSchema::CheckedString(checks(&input));
            let before = input.clone();
            let result = match zod_json(input.get("value"), &schema) {
                Ok(ZodJsonOutcome::Valid(value)) => json!({"ok":true,"value":value}),
                Ok(ZodJsonOutcome::Invalid(errors)) => json!({"ok":false,"errors":errors}),
                Err(reason) => json!({"unsupported":format!("{reason:?}")}),
            };
            assert_eq!(input, before);
            writeln!(output, "{result}").unwrap();
        }
    }
}
