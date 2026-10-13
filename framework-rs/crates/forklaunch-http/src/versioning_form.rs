//! Bounded URL-encoded matching for flat fields and repeated string arrays.
//! Nested/indexed qs paths are rejected rather than treated as literal keys.
use serde_json::{Map, Value};

pub(super) fn parse(bytes: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut groups = std::collections::BTreeMap::<String, (usize, Vec<Value>)>::new();
    for (index, part) in text.split('&').enumerate() {
        if index >= 1000 {
            return None;
        }
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        let key = decode(key);
        groups
            .entry(key)
            .or_insert_with(|| (index, Vec::new()))
            .1
            .push(Value::String(decode(value)));
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    // Object.keys enumerates canonical JavaScript integer keys before strings.
    groups.sort_by_key(|(key, (order, _))| {
        let integer = key
            .parse::<u32>()
            .ok()
            .filter(|n| *n != u32::MAX && n.to_string() == *key);
        (integer.is_none(), integer.unwrap_or(0), *order)
    });
    let mut fields = Map::new();
    for (key, (_, mut values)) in groups {
        let array = key.ends_with("[]");
        let name = if array { &key[..key.len() - 2] } else { &key };
        if name.contains(['[', ']']) || (array && name.is_empty()) {
            return None;
        }
        if name.is_empty() || name == "__proto__" {
            continue;
        }
        match fields.get_mut(name) {
            Some(Value::Array(previous)) => previous.append(&mut values),
            Some(previous) => {
                values.insert(0, previous.take());
                *previous = Value::Array(values);
            }
            None => {
                fields.insert(
                    name.into(),
                    if array || values.len() > 1 {
                        Value::Array(values)
                    } else {
                        values.remove(0)
                    },
                );
            }
        }
    }
    Some(Value::Object(fields))
}

// qs uses decodeURIComponent: malformed percent escapes or UTF-8 retain the
// entire original component, after '+' replacement, rather than lossy decoding.
fn decode(encoded: &str) -> String {
    let replaced = encoded
        .replace("%5B", "[")
        .replace("%5b", "[")
        .replace("%5D", "]")
        .replace("%5d", "]")
        .replace('+', " ");
    let input = replaced.as_bytes();
    let mut decoded = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            let Some(pair) = input.get(index + 1..index + 3) else {
                return replaced;
            };
            let Some(high) = digit(pair[0]) else {
                return replaced;
            };
            let Some(low) = digit(pair[1]) else {
                return replaced;
            };
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(input[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).unwrap_or(replaced)
}
const fn digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn limits_and_unsupported_paths_fail_without_losing_data() {
        assert!(parse("a=x&".repeat(1000).as_bytes()).is_none());
        assert!(parse("a=x&".repeat(999).as_bytes()).is_some());
        for text in ["a[b]=x", "a[0]=x", "a[][]=x", "[]=x"] {
            assert!(parse(text.as_bytes()).is_none(), "{text}");
        }
        assert!(parse(&[255]).is_none());
        assert_eq!(
            parse(b"__proto__=x&constructor=y&=ignored"),
            Some(json!({"constructor":"y"}))
        );
        assert_eq!(
            parse(b"a=%ff+%20&b=%E6%B0%B4&c=%2"),
            Some(json!({"a":"%ff %20","b":"水","c":"%2"}))
        );
        assert_eq!(
            parse(b"a=1&a[]=2&a%5B%5D=3"),
            Some(json!({"a":["1","2","3"]}))
        );
    }
}
