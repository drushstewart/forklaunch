use super::compile;
use serde_json::json;

#[test]
fn reserved_check_capability_follows_schema_positions_and_explicit_refs() {
    let assertion =
        json!({"type":"string","x-forklaunch-zod-checks":{"version":1,"utf16MaxLength":1}});
    for schema in [
        assertion.clone(),
        json!({"type":"object","properties":{"value":assertion}}),
        json!({"$defs":{"Check":assertion},"$ref":"#/$defs/Check"}),
        json!({"default":assertion,"$ref":"#/default"}),
    ] {
        assert_eq!(compile(&schema).is_ok(), cfg!(feature = "zod-checks"));
    }
    // Literal instance data is never interpreted as a schema unless referenced.
    let schema = json!({"const":{"x-forklaunch-zod-checks":{"version":999}},"default":{"x-forklaunch-zod-checks":null}});
    assert!(compile(&schema).unwrap()(&schema["const"]).is_ok());
}
#[cfg(feature = "zod-checks")]
#[test]
fn exact_assertions_reject_malformed_configs_and_preserve_input() {
    for invalid in [
        json!(null),
        json!([]),
        json!({}),
        json!({"version":2}),
        json!({"version":1,"extra":0}),
        json!({"version":1,"utf16MinLength":-1}),
        json!({"version":1,"utf16MaxLength":0.5}),
        json!({"version":1,"multipleOf":null}),
        json!({"version":1,"multipleOf":["nan"]}),
        json!({"version":1,"multipleOf":vec![1;10001]}),
    ] {
        assert!(compile(&json!({"x-forklaunch-zod-checks":invalid})).is_err());
    }
    let schema = json!({"type":"string","x-forklaunch-zod-checks":{"version":1.0,"utf16MinLength":1.0,"utf16MaxLength":1}});
    let validate = compile(&schema).unwrap();
    for (text, allowed) in [("a", true), ("水", true), ("", false), ("😀", false)] {
        let input = json!(text);
        let before = input.clone();
        assert_eq!(validate(&input).is_ok(), allowed);
        assert_eq!(input, before);
    }
    for (divisors, value, allowed) in [
        (json!([0.1]), json!(0.3), true),
        (json!([2, 3]), json!(3), false),
        (json!([-0.1]), json!(0.3), true),
        (json!([0]), json!(0), false),
        (json!(["NaN"]), json!(1), false),
        (json!(["Infinity"]), json!(1), false),
        (json!(["-Infinity"]), json!(1), false),
    ] {
        assert_eq!(compile(&json!({"type":"number","x-forklaunch-zod-checks":{"version":1,"multipleOf":divisors}})).unwrap()(&value).is_ok(),allowed);
    }
}

#[cfg(feature = "zod-checks")]
#[test]
fn format_checks_are_ordered_bounded_and_uri_requires_its_capability() {
    for invalid in [
        json!(null),
        json!({}),
        json!(["url"]),
        json!(["EMAIL"]),
        json!([null]),
        json!(vec!["email"; 10001]),
    ] {
        assert!(
            compile(&json!({"x-forklaunch-zod-checks":{"version":1,"formats":invalid}})).is_err()
        );
    }
    assert!(compile(&json!({"x-forklaunch-zod-checks":{"version":1,"formats":vec!["email";5001],"multipleOf":vec![1;5000]}})).is_err());
    let email =
        compile(&json!({"x-forklaunch-zod-checks":{"version":1,"formats":["email","email"]}}))
            .unwrap();
    assert!(email(&json!("a'b@b-.co")).is_ok());
    assert!(email(&json!("a%b@b.co")).is_err());
    let mixed =
        compile(&json!({"x-forklaunch-zod-checks":{"version":1,"formats":["email","uuid"]}}))
            .unwrap();
    assert!(mixed(&json!("a@b.co")).is_err());
    assert!(mixed(&json!(true)).is_ok());
    let uri = json!({"x-forklaunch-zod-checks":{"version":1,"formats":["uri"]}});
    assert_eq!(compile(&uri).is_ok(), cfg!(feature = "zod-uri"));
    for schema in [json!({"const":uri}), json!({"default":uri})] {
        assert!(compile(&schema).is_ok());
    }
    assert_eq!(
        compile(&json!({"default":uri,"$ref":"#/default"})).is_ok(),
        cfg!(feature = "zod-uri")
    );
}
