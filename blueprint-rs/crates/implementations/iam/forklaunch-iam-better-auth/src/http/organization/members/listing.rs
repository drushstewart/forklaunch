use super::{BlueprintResult, HashMap, Value, received, validation};
const OPERATORS: &[&str] = &[
    "eq",
    "ne",
    "lt",
    "lte",
    "gt",
    "gte",
    "in",
    "not_in",
    "contains",
    "starts_with",
    "ends_with",
];
pub(super) fn validate(query: &HashMap<String, Value>) -> BlueprintResult<()> {
    let mut errors = Vec::new();
    for field in [
        "limit",
        "offset",
        "sortBy",
        "sortDirection",
        "filterField",
        "filterValue",
        "filterOperator",
        "organizationId",
        "organizationSlug",
    ] {
        let Some(value) = query.get(field) else {
            continue;
        };
        match field {
            "limit" | "offset" => {
                if !value.is_string() && !value.is_number() {
                    errors.push(format!("[query.{field}] Invalid input"));
                }
            }
            "filterValue" => {
                if !(value.is_string()
                    || value.is_number()
                    || value.is_boolean()
                    || value.as_array().is_some_and(|values| {
                        values.iter().all(Value::is_string) || values.iter().all(Value::is_number)
                    }))
                {
                    errors.push(format!("[query.{field}] Invalid input"));
                }
            }
            "sortDirection" | "filterOperator" => {
                let options = if field == "sortDirection" {
                    &["asc", "desc"][..]
                } else {
                    OPERATORS
                };
                if !value.as_str().is_some_and(|value| options.contains(&value)) {
                    errors.push(format!(
                        "[query.{field}] Invalid option: expected one of {}",
                        options
                            .iter()
                            .map(|value| format!("\"{value}\""))
                            .collect::<Vec<_>>()
                            .join("|")
                    ));
                }
            }
            _ => {
                if !value.is_string() {
                    errors.push(format!(
                        "[query.{field}] Invalid input: expected string, received {}",
                        received(Some(value))
                    ));
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(validation(errors.join("; ")))
    }
}
