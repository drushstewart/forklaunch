#[test]
fn forklaunch_literal_internal_marker_keys_are_object_data() {
    use serde::Deserialize as _;
    use serde_json::{json, Value};
    #[derive(serde::Deserialize)]
    struct Record {
        nested: Value,
    }
    for key in [
        "$serde_json::private::Number",
        "$serde_json::private::RawValue",
    ] {
        for value in [json!("123"), json!({"id":true}), json!(null), json!([1, 2])] {
            let expected = Value::Object([(key.into(), value)].into_iter().collect());
            let text = expected.to_string();
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), expected);
            assert_eq!(
                serde_json::from_slice::<Value>(text.as_bytes()).unwrap(),
                expected
            );
            #[cfg(feature = "std")]
            assert_eq!(
                serde_json::from_reader::<_, Value>(text.as_bytes()).unwrap(),
                expected
            );
            assert_eq!(
                serde_json::from_value::<Value>(expected.clone()).unwrap(),
                expected
            );
            assert_eq!(Value::deserialize(&expected).unwrap(), expected);
            assert_eq!(
                serde_json::from_str::<Record>(&format!("{{\"nested\":{text}}}"))
                    .unwrap()
                    .nested,
                expected
            );
            #[cfg(feature = "raw_value")]
            {
                let raw = serde_json::value::RawValue::from_string(text).unwrap();
                assert_eq!(serde_json::to_value(raw).unwrap(), expected);
            }
        }
    }
    let escaped: Value =
        serde_json::from_str(r#"{"\u0024serde_json::private::Number":"1","extra":false}"#).unwrap();
    assert_eq!(escaped["$serde_json::private::Number"], "1");
    assert_eq!(escaped["extra"], false);
    let duplicate: Value = serde_json::from_str(
        r#"{"$serde_json::private::RawValue":"1","$serde_json::private::RawValue":"2"}"#,
    )
    .unwrap();
    assert_eq!(duplicate["$serde_json::private::RawValue"], "2");
}

#[cfg(feature = "arbitrary_precision")]
#[test]
fn forklaunch_real_private_number_protocol_retains_precision() {
    for text in [
        "12345678901234567890123456789012345678901234567890",
        "1.00000000000000000000000000001",
        "1e+999",
    ] {
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        assert!(value.is_number());
        assert_eq!(value.to_string(), text);
        assert_eq!(
            serde_json::from_value::<serde_json::Value>(value.clone()).unwrap(),
            value
        );
    }
}
