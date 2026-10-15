use super::*;
use serde_json::json;

#[test]
fn defaults_preserve_absence_order_and_null() {
    let fallback = json!("default");
    let optional_default = ZodJsonSchema::Optional(Box::new(ZodJsonSchema::Default {
        schema: Box::new(ZodJsonSchema::String(vec![])),
        value: &fallback,
    }));
    assert_eq!(
        zod_json(None, &optional_default),
        Ok(ZodJsonOutcome::Valid(None))
    );
    let default_optional = ZodJsonSchema::Default {
        schema: Box::new(ZodJsonSchema::Optional(Box::new(ZodJsonSchema::String(
            vec![],
        )))),
        value: &fallback,
    };
    assert_eq!(
        zod_json(None, &default_optional),
        Ok(ZodJsonOutcome::Valid(Some(fallback.clone())))
    );
    assert!(matches!(
        zod_json(Some(&Value::Null), &default_optional),
        Ok(ZodJsonOutcome::Invalid(_))
    ));
}

#[test]
fn preflight_rejects_depth_and_width_before_evaluation() {
    let mut schema = ZodJsonSchema::Unknown;
    for _ in 0..65 {
        schema = ZodJsonSchema::Optional(Box::new(schema));
    }
    assert_eq!(
        zod_json(None, &schema),
        Err(ZodJsonUnsupported::SchemaDepth)
    );
    let mut input = Value::Null;
    for _ in 0..65 {
        input = Value::Array(vec![input]);
    }
    assert_eq!(
        zod_json(Some(&input), &ZodJsonSchema::Unknown),
        Err(ZodJsonUnsupported::InputDepth)
    );
    let input = Value::Array(vec![Value::Null; MAX_NODES]);
    assert_eq!(
        zod_json(Some(&input), &ZodJsonSchema::Unknown),
        Err(ZodJsonUnsupported::InputNodes)
    );
    let schema = ZodJsonSchema::String(vec![
        ZodStringCheck {
            rule: super::super::ZodStringRule::Step(super::super::ZodStringStep::Trim),
            message: None
        };
        MAX_NODES
    ]);
    assert_eq!(
        zod_json(None, &schema),
        Err(ZodJsonUnsupported::SchemaNodes)
    );
}

#[test]
fn schema_and_evaluation_admission_are_distinct_from_parse_errors() {
    let schema = ZodJsonSchema::Object(vec![
        ZodJsonField {
            name: "a",
            schema: ZodJsonSchema::Unknown,
        },
        ZodJsonField {
            name: "a",
            schema: ZodJsonSchema::Unknown,
        },
    ]);
    assert_eq!(
        zod_json(None, &schema),
        Err(ZodJsonUnsupported::DuplicateField)
    );
    let schema = ZodJsonSchema::Object(vec![ZodJsonField {
        name: "toString",
        schema: ZodJsonSchema::Unknown,
    }]);
    assert_eq!(
        zod_json(None, &schema),
        Err(ZodJsonUnsupported::PrototypeField)
    );
    let mut element = ZodJsonSchema::Unknown;
    for _ in 0..60 {
        element = ZodJsonSchema::Optional(Box::new(element));
    }
    let schema = ZodJsonSchema::Array {
        element: Box::new(element),
        checks: vec![],
    };
    assert_eq!(
        zod_json(Some(&json!(vec![0; 2000])), &schema),
        Err(ZodJsonUnsupported::EvaluationNodes)
    );
}

#[test]
fn status_retains_dirty_vs_aborted_for_future_composition() {
    let schema = ZodJsonSchema::String(vec![ZodStringCheck {
        rule: super::super::ZodStringRule::Step(super::super::ZodStringStep::Min(2)),
        message: None,
    }]);
    let mut remaining = Budget::new();
    assert!(matches!(
        evaluate(Some(&json!("x")), &schema, &mut remaining)
            .unwrap()
            .status,
        Status::Dirty
    ));
    assert!(matches!(
        evaluate(Some(&json!(null)), &schema, &mut remaining)
            .unwrap()
            .status,
        Status::Aborted
    ));
}

#[test]
fn numeric_keys_order_errors_and_unknown_fields_strip_atomically() {
    let schema = ZodJsonSchema::Object(vec![
        ZodJsonField {
            name: "10",
            schema: ZodJsonSchema::String(vec![]),
        },
        ZodJsonField {
            name: "2",
            schema: ZodJsonSchema::String(vec![]),
        },
        ZodJsonField {
            name: "a",
            schema: ZodJsonSchema::String(vec![]),
        },
    ]);
    let ZodJsonOutcome::Invalid(errors) = zod_json(Some(&json!({})), &schema).unwrap() else {
        panic!("expected diagnostics");
    };
    assert_eq!(
        errors
            .iter()
            .map(|e| e.path[0].as_str())
            .collect::<Vec<_>>(),
        ["2", "10", "a"]
    );
    let input = json!({"10":"x","2":"y","a":"z","extra":true});
    assert_eq!(
        zod_json(Some(&input), &schema),
        Ok(ZodJsonOutcome::Valid(Some(
            json!({"10":"x","2":"y","a":"z"})
        )))
    );
    assert_eq!(input["extra"], true);
}

#[test]
fn repeated_container_checks_consume_evaluation_budget_before_application() {
    let string_check = ZodStringCheck {
        rule: super::super::ZodStringRule::Step(super::super::ZodStringStep::Trim),
        message: None,
    };
    let string = ZodJsonSchema::String(vec![string_check; 1000]);
    let number = ZodJsonSchema::Number(vec![
        ZodNumberCheck {
            step: super::super::ZodNumberStep::Finite,
            message: None,
        };
        1000
    ]);
    let nested_array = ZodJsonSchema::Array {
        element: Box::new(ZodJsonSchema::Unknown),
        checks: vec![ZodArrayStep::Min(0); 1000],
    };
    for (element, value) in [
        (string, json!("x")),
        (number, json!(1)),
        (nested_array, json!([])),
    ] {
        let schema = ZodJsonSchema::Array {
            element: Box::new(element),
            checks: vec![],
        };
        let input = Value::Array(vec![value; 200]);
        assert!(admit_schema(&schema).is_ok());
        assert!(admit_value(&input).is_ok());
        assert_eq!(
            zod_json(Some(&input), &schema),
            Err(ZodJsonUnsupported::EvaluationNodes)
        );
    }
}

#[test]
fn aggregate_byte_admission_and_repeated_default_output_are_bounded() {
    let oversized = json!("x".repeat(MAX_ADMISSION_BYTES + 1));
    assert_eq!(
        zod_json(Some(&oversized), &ZodJsonSchema::Unknown),
        Err(ZodJsonUnsupported::InputBytes)
    );
    let default = json!("x".repeat(1024 * 1024));
    for leaf in [ZodJsonSchema::String(vec![]), ZodJsonSchema::Unknown] {
        let schema = ZodJsonSchema::Array {
            element: Box::new(ZodJsonSchema::Object(vec![ZodJsonField {
                name: "payload",
                schema: ZodJsonSchema::Default {
                    schema: Box::new(leaf),
                    value: &default,
                },
            }])),
            checks: vec![],
        };
        let input = Value::Array(vec![json!({}); 100]);
        assert!(admit_schema(&schema).is_ok());
        assert!(admit_value(&input).is_ok());
        assert_eq!(
            zod_json(Some(&input), &schema),
            Err(ZodJsonUnsupported::EvaluationBytes)
        );
    }
}

#[test]
fn message_and_rule_times_string_work_is_charged_before_checks() {
    let message = "x".repeat(1024 * 1024);
    let rule = super::super::ZodStringRule::Step(super::super::ZodStringStep::Min(2));
    let schema = ZodJsonSchema::Array {
        element: Box::new(ZodJsonSchema::String(vec![ZodStringCheck {
            rule,
            message: Some(&message),
        }])),
        checks: vec![],
    };
    assert_eq!(
        zod_json(Some(&json!(vec!["x"; 100])), &schema),
        Err(ZodJsonUnsupported::EvaluationBytes)
    );
    let schema = ZodJsonSchema::String(vec![
        ZodStringCheck {
            rule,
            message: Some(&message)
        };
        17
    ]);
    assert_eq!(
        zod_json(None, &schema),
        Err(ZodJsonUnsupported::SchemaBytes)
    );
    let schema = ZodJsonSchema::String(vec![
        ZodStringCheck {
            rule,
            message: None
        };
        1000
    ]);
    let input = json!("x".repeat(1024 * 1024));
    let mut budget = Budget::new();
    assert!(matches!(
        evaluate(Some(&input), &schema, &mut budget),
        Err(ZodJsonUnsupported::EvaluationBytes)
    ));
    assert_eq!(budget.nodes, MAX_NODES);
}

#[test]
fn unknown_normalizes_large_integer_values_without_mutating_input() {
    let input: Value =
        serde_json::from_str("{\"n\":[9007199254740993,18446744073709551615]}").unwrap();
    let original = input.to_string();
    let ZodJsonOutcome::Valid(Some(output)) =
        zod_json(Some(&input), &ZodJsonSchema::Unknown).unwrap()
    else {
        panic!("valid normalized output");
    };
    assert_eq!(
        output["n"][0].as_f64().unwrap().to_bits(),
        9_007_199_254_740_992.0f64.to_bits()
    );
    assert_eq!(
        output["n"][1].as_f64().unwrap().to_bits(),
        18_446_744_073_709_551_616.0f64.to_bits()
    );
    assert_eq!(input.to_string(), original);
}

#[test]
fn arbitrary_precision_nonfinite_values_are_rejected_in_every_input_position() {
    // The normal serde profile rejects this literal at construction. The second
    // test command enables arbitrary_precision, as IAM dependency unification does.
    let Ok(value) = serde_json::from_str::<Value>("1e309") else {
        return;
    };
    for input in [
        value.clone(),
        json!([value.clone()]),
        json!({"extra":value.clone()}),
    ] {
        assert_eq!(
            zod_json(Some(&input), &ZodJsonSchema::Unknown),
            Err(ZodJsonUnsupported::NonFiniteNumber)
        );
        assert_eq!(
            zod_json(Some(&input), &ZodJsonSchema::Object(vec![])),
            Err(ZodJsonUnsupported::NonFiniteNumber)
        );
    }
    let schema = ZodJsonSchema::Default {
        schema: Box::new(ZodJsonSchema::Unknown),
        value: &value,
    };
    assert_eq!(
        zod_json(None, &schema),
        Err(ZodJsonUnsupported::NonFiniteNumber)
    );
}
