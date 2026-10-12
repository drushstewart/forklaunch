//! Preserve literal object keys that serde's Value visitor reserves internally.
use serde::{
    Deserializer,
    de::{Error, MapAccess, Visitor},
};
use serde_json::{Map, Value, value::RawValue};
use std::fmt;

pub(crate) fn parse(text: &str) -> Result<Value, serde_json::Error> {
    let raw: Box<RawValue> = serde_json::from_str(text)?;
    parse_raw(&raw, 0)
}
fn parse_raw(raw: &RawValue, depth: usize) -> Result<Value, serde_json::Error> {
    if depth >= 128 {
        return Err(serde_json::Error::custom("JSON nesting limit"));
    }
    match raw.get().as_bytes().first() {
        Some(b'{') => {
            let mut decoder = serde_json::Deserializer::from_str(raw.get());
            decoder.deserialize_map(Object(depth))
        }
        Some(b'[') => {
            let values: Vec<Box<RawValue>> = serde_json::from_str(raw.get())?;
            values
                .iter()
                .map(|raw| parse_raw(raw, depth + 1))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        _ => serde_json::from_str(raw.get()),
    }
}
struct Object(usize);
impl<'de> Visitor<'de> for Object {
    type Value = Value;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("JSON object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<Value, A::Error> {
        let mut values = Map::new();
        while let Some((key, raw)) = entries.next_entry::<String, Box<RawValue>>()? {
            values.insert(key, parse_raw(&raw, self.0 + 1).map_err(A::Error::custom)?);
        }
        Ok(Value::Object(values))
    }
}
