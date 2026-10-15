use forklaunch_validation::{
    typescript_coerce::{
        ZodFloatBound, ZodJsonField, ZodJsonLiteral, ZodJsonOutcome, ZodJsonSchema, ZodNumberCheck,
        ZodNumberStep, ZodStringCheck, ZodStringRule, ZodStringStep, zod_json_at_path,
    },
    zod_array::ZodArrayStep,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn schema(spec: &Value) -> ZodJsonSchema<'_> {
    let inner = || Box::new(schema(&spec["inner"]));
    match spec["kind"].as_str().unwrap() {
        "string" => ZodJsonSchema::String(
            spec["checks"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|check| ZodStringCheck {
                    message: check["message"].as_str(),
                    rule: match check["name"].as_str().unwrap() {
                        "trim" => ZodStringRule::Step(ZodStringStep::Trim),
                        "min" => ZodStringRule::Step(ZodStringStep::Min(
                            check["argument"].as_u64().unwrap().try_into().unwrap(),
                        )),
                        "endsWith" => ZodStringRule::EndsWith(check["argument"].as_str().unwrap()),
                        _ => panic!("unknown string check"),
                    },
                })
                .collect(),
        ),
        "number" => ZodJsonSchema::Number(
            spec["checks"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|check| ZodNumberCheck {
                    message: check["message"].as_str(),
                    step: match check["name"].as_str().unwrap() {
                        "int" => ZodNumberStep::Int,
                        "finite" => ZodNumberStep::Finite,
                        "min" => ZodNumberStep::MinFloat(ZodFloatBound::new(
                            check["argument"].as_f64().unwrap(),
                        )),
                        _ => panic!("unknown number check"),
                    },
                })
                .collect(),
        ),
        "union" => ZodJsonSchema::Union(
            spec["branches"]
                .as_array()
                .unwrap()
                .iter()
                .map(schema)
                .collect(),
        ),
        "literal" => ZodJsonSchema::Literal(match spec["literal"]["kind"].as_str().unwrap() {
            "string" => ZodJsonLiteral::String(spec["literal"]["value"].as_str().unwrap()),
            "boolean" => ZodJsonLiteral::Boolean(spec["literal"]["value"].as_bool().unwrap()),
            "number" => ZodJsonLiteral::Number(
                spec["literal"]["value"]
                    .as_f64()
                    .unwrap_or_else(|| spec["literal"]["value"].as_str().unwrap().parse().unwrap()),
            ),
            _ => panic!("unsupported literal kind"),
        }),
        "boolean" => ZodJsonSchema::Boolean,
        "unknown" => ZodJsonSchema::Unknown,
        "optional" => ZodJsonSchema::Optional(inner()),
        "nullable" => ZodJsonSchema::Nullable(inner()),
        "default" => ZodJsonSchema::Default {
            schema: inner(),
            value: &spec["value"],
        },
        "array" => ZodJsonSchema::Array {
            element: inner(),
            checks: spec["checks"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|check| {
                    let length = || check["argument"].as_u64().unwrap().try_into().unwrap();
                    match check["name"].as_str().unwrap() {
                        "min" => ZodArrayStep::Min(length()),
                        "max" => ZodArrayStep::Max(length()),
                        "length" => ZodArrayStep::Length(length()),
                        _ => panic!("unknown array check"),
                    }
                })
                .collect(),
        },
        "object" => ZodJsonSchema::Object(
            spec["fields"]
                .as_array()
                .unwrap()
                .iter()
                .map(|field| ZodJsonField {
                    name: field[0].as_str().unwrap(),
                    schema: schema(&field[1]),
                })
                .collect(),
        ),
        _ => panic!("unknown schema"),
    }
}
fn tag(value: Value) -> Value {
    match value {
        Value::Number(number) => {
            json!({"$number":format!("{:016x}",number.as_f64().unwrap().to_bits())})
        }
        Value::Array(values) => Value::Array(values.into_iter().map(tag).collect()),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, tag(value)))
                .collect(),
        ),
        other => other,
    }
}
fn main() {
    for line in io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let before = input.clone();
        let raw = input["raw"]
            .as_str()
            .map(|value| serde_json::from_str::<Value>(value).unwrap());
        let path: Vec<_> = input["path"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|part| part.as_str().unwrap())
            .collect();
        let result = match zod_json_at_path(
            raw.as_ref().or_else(|| input.get("value")),
            &schema(&input["schema"]),
            &path,
        ) {
            Ok(ZodJsonOutcome::Valid(value)) => match value {
                Some(value) => json!({"ok":true,"absent":false,"value":tag(value)}),
                None => json!({"ok":true,"absent":true}),
            },
            Ok(ZodJsonOutcome::Invalid(errors)) => json!({"ok":false,"errors":errors}),
            Err(reason) => json!({"unsupported":format!("{reason:?}")}),
        };
        assert_eq!(input, before);
        println!("{result}");
    }
}
