//! Organization updates and the configured exclusive permission aliases.
use super::validation::{received, validation};
use super::{BlueprintResult, Value};
pub(super) fn validate_body(path: &str, body: Option<&Value>) -> BlueprintResult<()> {
    if !body.is_some_and(Value::is_object) {
        let mut message = format!(
            "[body] Invalid input: expected object, received {}",
            received(body)
        );
        if path == "/organization/has-permission" {
            message.push_str("; [body] Invalid input");
        }
        return Err(validation(message));
    }
    let body = body.expect("object checked above");
    let mut errors = Vec::new();
    if path == "/organization/update" {
        update(body, &mut errors);
    }
    if let Some(value) = body.get("organizationId")
        && !value.is_string()
    {
        errors.push(format!(
            "[body.organizationId] Invalid input: expected string, received {}",
            received(Some(value))
        ));
    }
    if path == "/organization/has-permission" {
        let matches = ["permission", "permissions"]
            .iter()
            .filter(|field| body.get(**field).is_some_and(permissions))
            .count();
        if matches == 0 {
            errors.push("[body] Invalid input".into());
        }
        if matches == 2 {
            errors.push("[body] Invalid input: more than one option matched".into());
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(validation(errors.join("; ")))
    }
}
fn permissions(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.values().all(|value| {
            value
                .as_array()
                .is_some_and(|array| array.iter().all(Value::is_string))
        })
    })
}
fn update(body: &Value, errors: &mut Vec<String>) {
    let Some(data) = body.get("data").filter(|data| data.is_object()) else {
        errors.push(format!(
            "[body.data] Invalid input: expected object, received {}",
            received(body.get("data"))
        ));
        return;
    };
    for field in [
        "domain",
        "subscription",
        "status",
        "name",
        "slug",
        "logo",
        "metadata",
    ] {
        let Some(value) = data.get(field) else {
            continue;
        };
        if value.is_null() && matches!(field, "domain" | "subscription" | "status" | "logo") {
            continue;
        }
        let kind = if field == "metadata" {
            "record"
        } else {
            "string"
        };
        if !(if field == "metadata" {
            value.is_object()
        } else {
            value.is_string()
        }) {
            errors.push(format!(
                "[body.data.{field}] Invalid input: expected {kind}, received {}",
                received(Some(value))
            ));
        } else if matches!(field, "name" | "slug") && value.as_str() == Some("") {
            errors.push(format!(
                "[body.data.{field}] Too small: expected string to have >=1 characters"
            ));
        }
    }
}
