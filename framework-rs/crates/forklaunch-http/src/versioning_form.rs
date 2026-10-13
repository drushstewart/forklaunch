//! Bounded URL-encoded matching for strings, arrays and nested bracket paths.
use serde_json::Value;
#[path = "versioning_form_tree.rs"]
mod tree;

pub(super) fn parse(bytes: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(bytes).ok()?;
    let parameter_count = text.bytes().filter(|byte| *byte == b'&').count() + 1;
    if parameter_count > 1000 {
        return None;
    }
    let limit = u64::try_from(parameter_count.max(100)).ok()?;
    let mut groups = std::collections::BTreeMap::<String, (usize, Vec<Value>)>::new();
    for (index, part) in text.split('&').enumerate() {
        if index >= 1000 {
            return None;
        }
        let part = brackets(part);
        let separator = part
            .find("]=")
            .map(|index| index + 1)
            .or_else(|| part.find('='));
        let (key, value) = separator.map_or((part.as_str(), ""), |index| {
            (&part[..index], &part[index + 1..])
        });
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
    let mut fields = tree::Node::empty();
    for (key, (_, values)) in groups {
        let node = tree::wrap(&key, tree::Node::values(values), limit)?;
        fields = fields.merge(node, limit);
        if !fields.within_depth(0) {
            return None;
        }
    }
    fields.into_value(0)
}

// qs uses decodeURIComponent: malformed percent escapes or UTF-8 retain the
// entire original component, after '+' replacement, rather than lossy decoding.
fn decode(encoded: &str) -> String {
    let replaced = brackets(encoded).replace('+', " ");
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
fn brackets(encoded: &str) -> String {
    encoded
        .replace("%5B", "[")
        .replace("%5b", "[")
        .replace("%5D", "]")
        .replace("%5d", "]")
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
    fn parameter_depth_limits_and_decoding_are_explicit() {
        assert!(parse("a=x&".repeat(1000).as_bytes()).is_none());
        assert!(parse("a=x&".repeat(999).as_bytes()).is_some());
        assert!(parse(format!("a{}=x", "[b]".repeat(33)).as_bytes()).is_none());
        assert_eq!(
            parse(b"a[b]=x&a[c][]=y&a[c][]=z"),
            Some(json!({"a":{"b":"x","c":["y","z"]}}))
        );
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
