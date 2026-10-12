//! String coercion at configured Zod server/HTTP boundaries.
use serde_json::Value;
pub(crate) fn string(value: Option<&Value>) -> Option<String> {
    Some(match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::String(value)) => value.clone(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Number(value)) => ryu_js::Buffer::new().format(value.as_f64()?).to_owned(),
        Some(Value::Object(value)) => {
            // A JSON own toString property shadows the inherited callable. The
            // default valueOf still returns an object, so ToPrimitive throws.
            if value.contains_key("toString") {
                return None;
            }
            "[object Object]".into()
        }
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                if value.is_null() {
                    Some(String::new())
                } else {
                    string(Some(value))
                }
            })
            .collect::<Option<Vec<_>>>()?
            .join(","),
    })
}
