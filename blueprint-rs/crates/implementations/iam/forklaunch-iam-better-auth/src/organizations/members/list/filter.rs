use super::{BlueprintResult, Value, adapter_error, field, values};
use std::cmp::Ordering::{Equal, Greater, Less};
pub(super) struct Filter<'a> {
    field: Option<&'a str>,
    operator: &'a str,
    value: Option<&'a Value>,
}
impl<'a> Filter<'a> {
    pub(super) fn new(query: &'a Value) -> BlueprintResult<Self> {
        let selected = query["filterField"]
            .as_str()
            .filter(|value| !value.is_empty());
        if let Some(selected) = selected {
            field(selected)?;
        }
        let operator = query["filterOperator"].as_str().unwrap_or("eq");
        let value = query.get("filterValue");
        if selected.is_some()
            && matches!(operator, "in" | "not_in")
            && !value.is_some_and(Value::is_array)
        {
            return Err(adapter_error());
        }
        Ok(Self {
            field: selected,
            operator,
            value,
        })
    }
    pub(super) fn matches(&self, row: &Value) -> BlueprintResult<bool> {
        let Some(field) = self.field else {
            return Ok(true);
        };
        let left = row.get(field);
        let equal = left == self.value && (field != "createdAt" || left.is_none());
        Ok(match self.operator {
            "eq" => equal,
            "ne" => !equal,
            "in" | "not_in" => {
                let array = self
                    .value
                    .and_then(Value::as_array)
                    .ok_or_else(adapter_error)?;
                let contained =
                    field != "createdAt" && left.is_some_and(|left| array.contains(left));
                contained == (self.operator == "in")
            }
            "contains" | "starts_with" | "ends_with" => {
                if self.operator == "contains" && left.is_none_or(Value::is_null) {
                    return Ok(false);
                }
                let left = left
                    .and_then(Value::as_str)
                    .filter(|_| field != "createdAt")
                    .ok_or_else(adapter_error)?;
                let right = values::string(self.value);
                match self.operator {
                    "contains" => left.contains(&right),
                    "starts_with" => left.starts_with(&right),
                    _ => left.ends_with(&right),
                }
            }
            "lt" | "lte" | "gt" | "gte" => self.relational(field, left),
            _ => return Err(adapter_error()),
        })
    }
    fn relational(&self, field: &str, left: Option<&Value>) -> bool {
        if self.value.is_none_or(Value::is_null) {
            return false;
        }
        let ordering = if field != "createdAt"
            && left.is_some_and(Value::is_string)
            && self.value.is_some_and(Value::is_string)
        {
            Some(
                values::string(left)
                    .encode_utf16()
                    .cmp(values::string(self.value).encode_utf16()),
            )
        } else {
            let left = if field == "createdAt" {
                left.and_then(Value::as_str)
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                    .map_or(f64::NAN, |date| {
                        date.timestamp_millis()
                            .to_string()
                            .parse()
                            .unwrap_or(f64::NAN)
                    })
            } else {
                values::number(left)
            };
            left.partial_cmp(&values::number(self.value))
        };
        match self.operator {
            "lt" => ordering == Some(Less),
            "lte" => matches!(ordering, Some(Less | Equal)),
            "gt" => ordering == Some(Greater),
            "gte" => matches!(ordering, Some(Greater | Equal)),
            _ => false,
        }
    }
}
