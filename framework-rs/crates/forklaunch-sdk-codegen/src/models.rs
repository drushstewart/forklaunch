//! Shared schema-to-model compiler used by native generators and the packaged CLI.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use convert_case::{Case, Casing};
use ramhorns::Content;
use serde_json::Value as JsonValue;

#[derive(Content, Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct RustModelTemplate {
    pub name: String,
    pub fields: Vec<RustModelFieldTemplate>,
    pub union: bool,
    pub deny_unknown_fields: bool,
    pub transparent: bool,
    pub has_default: bool,
    pub variants: Vec<RustUnionVariantTemplate>,
}

#[derive(Content, Debug, Clone, PartialEq, Eq)]
pub struct RustUnionVariantTemplate {
    pub name: String,
    pub value_type: String,
    pub first: bool,
}

#[derive(Content, Debug, Clone, PartialEq, Eq)]
pub struct RustModelFieldTemplate {
    pub optional: bool,
    pub required: bool,
    pub flatten: bool,
    pub wire_name_literal: String,
    pub field_name: String,
    pub field_type: String,
}

#[derive(Content, Debug, Clone, PartialEq, Eq)]
pub struct RustTypeAliasTemplate {
    pub name: String,
    pub target_type: String,
}

pub(crate) fn ts_type_from_schema_name(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "string" | "str" | "uuid" | "datetime" | "date" => "string".to_string(),
        "bool" | "boolean" => "boolean".to_string(),
        "usize" | "u64" | "u32" | "i64" | "i32" | "integer" | "int" | "number" | "float"
        | "f64" | "f32" => "number".to_string(),
        _ => "unknown".to_string(),
    }
}

pub(crate) fn ts_type_from_schema(schema: &JsonValue) -> String {
    if let Some(allowed) = schema.as_bool() {
        return if allowed { "unknown" } else { "never" }.into();
    }
    if let Some(parts) = schema.get("allOf").and_then(JsonValue::as_array) {
        let mut base = schema.clone();
        base.as_object_mut().unwrap().remove("allOf");
        return std::iter::once(&base)
            .chain(parts)
            .map(|part| format!("({})", ts_type_from_schema(part)))
            .collect::<Vec<_>>()
            .join(" & ");
    }
    if let Some(name) = schema
        .get(super::schema::REFERENCE)
        .and_then(JsonValue::as_str)
    {
        return name.to_owned();
    }
    if schema.get("nullable") == Some(&JsonValue::Bool(true)) {
        let mut value = schema.clone();
        value.as_object_mut().unwrap().remove("nullable");
        return format!("{} | null", ts_type_from_schema(&value));
    }
    if let Some(const_value) = schema.get("const") {
        return ts_literal_type(const_value);
    }

    if let Some(enum_values) = schema.get("enum").and_then(JsonValue::as_array)
        && !enum_values.is_empty()
    {
        return enum_values
            .iter()
            .map(ts_literal_type)
            .collect::<Vec<_>>()
            .join(" | ");
    }

    if let Some(one_of) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(JsonValue::as_array)
        && !one_of.is_empty()
    {
        return one_of
            .iter()
            .map(ts_type_from_schema)
            .collect::<Vec<_>>()
            .join(" | ");
    }

    if let Some(types) = schema.get("type").and_then(JsonValue::as_array) {
        return types
            .iter()
            .filter_map(JsonValue::as_str)
            .map(|kind| {
                let mut variant = schema.clone();
                variant["type"] = JsonValue::String(kind.to_owned());
                ts_type_from_schema(&variant)
            })
            .collect::<Vec<_>>()
            .join(" | ");
    }

    match schema_type(schema) {
        Some("object") => ts_object_type(schema),
        Some("array") => ts_array_type(schema),
        Some("integer" | "number") => "number".to_string(),
        Some("string") => "string".to_string(),
        Some("boolean") => "boolean".to_string(),
        Some("null") => "null".to_string(),
        _ if schema.get("properties").is_some() => ts_object_type(schema),
        _ => "unknown".to_string(),
    }
}

fn ts_array_type(schema: &JsonValue) -> String {
    if let Some(prefix) = schema
        .get("prefixItems")
        .or_else(|| schema.get("items").filter(|items| items.is_array()))
        .and_then(JsonValue::as_array)
    {
        let tail = if schema.get("prefixItems").is_some() {
            schema.get("items")
        } else {
            schema.get("additionalItems")
        };
        let minimum = schema
            .get("minItems")
            .and_then(JsonValue::as_u64)
            .unwrap_or(0);
        let maximum = schema
            .get("maxItems")
            .and_then(JsonValue::as_u64)
            .unwrap_or(u64::MAX);
        let mut fields = prefix
            .iter()
            .enumerate()
            .take_while(|(index, _)| (*index as u64) < maximum)
            .map(|(index, item)| {
                format!(
                    "item{index}{}: {}",
                    if (index as u64) < minimum { "" } else { "?" },
                    ts_type_from_schema(item)
                )
            })
            .collect::<Vec<_>>();
        if maximum > prefix.len() as u64 && tail != Some(&JsonValue::Bool(false)) {
            fields.push(format!(
                "...rest: Array<{}>",
                tail.map_or_else(|| "unknown".into(), ts_type_from_schema)
            ));
        }
        return format!("[{}]", fields.join(", "));
    }
    let item_type = schema
        .get("items")
        .map_or_else(|| "unknown".to_string(), ts_type_from_schema);
    format!("Array<{item_type}>")
}

pub(crate) fn ts_object_type(schema: &JsonValue) -> String {
    let properties = schema.get("properties").and_then(JsonValue::as_object);
    if properties.is_none_or(serde_json::Map::is_empty) {
        let value_type = match schema.get("additionalProperties") {
            Some(JsonValue::Bool(false)) => "never".into(),
            Some(value) if value.is_object() => ts_type_from_schema(value),
            _ => "unknown".into(),
        };
        if schema
            .get("additionalProperties")
            .and_then(|value| value.get(super::schema::REFERENCE))
            .is_some()
        {
            return format!("{{ [key: string]: {value_type} }}");
        }
        return format!("Record<string, {value_type}>");
    }
    let properties = properties.unwrap();

    let required = schema
        .get("required")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .collect::<BTreeSet<_>>();

    let mut parts = Vec::new();
    for (name, property_schema) in properties {
        let optional = if required.contains(name.as_str()) {
            ""
        } else {
            "?"
        };
        parts.push(format!(
            "{}{}: {}",
            ts_property_key(name),
            optional,
            ts_type_from_schema(property_schema)
        ));
    }

    let object_type = format!("{{ {} }}", parts.join("; "));
    if schema.get("additionalProperties") == Some(&JsonValue::Bool(false)) {
        object_type
    } else {
        format!("{object_type} & Record<string, unknown>")
    }
}

pub(crate) fn schema_type(schema: &JsonValue) -> Option<&str> {
    schema.get("type").and_then(|schema_type| {
        schema_type.as_str().or_else(|| {
            schema_type.as_array().and_then(|types| {
                types
                    .iter()
                    .filter_map(JsonValue::as_str)
                    .find(|ty| *ty != "null")
            })
        })
    })
}

pub(crate) fn ts_literal_type(value: &JsonValue) -> String {
    if let Some(value) = value.as_str() {
        return format!("'{}'", escape_ts_string(value));
    }
    if value.is_number() || value.is_boolean() {
        return value.to_string();
    }
    if value.is_null() {
        return "null".to_string();
    }
    match value {
        JsonValue::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(ts_literal_type)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        JsonValue::Object(values) if values.is_empty() => "Record<string, never>".into(),
        JsonValue::Object(values) => format!(
            "{{ {} }}",
            values
                .iter()
                .map(|(key, value)| format!("{}: {}", ts_property_key(key), ts_literal_type(value)))
                .collect::<Vec<_>>()
                .join("; ")
        ),
        _ => "unknown".into(),
    }
}

pub(crate) fn ts_property_key(name: &str) -> String {
    if is_ts_identifier(name) {
        name.to_string()
    } else {
        format!("'{}'", escape_ts_string(name))
    }
}

pub(crate) fn is_ts_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first == '$' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric())
}

pub(crate) fn rust_types_from_schema(
    root_name: &str,
    schema: &JsonValue,
) -> (Vec<RustModelTemplate>, Vec<RustTypeAliasTemplate>, String) {
    let mut models = BTreeMap::new();
    if let Some(definitions) = schema
        .get(super::schema::DEFINITIONS)
        .and_then(JsonValue::as_object)
    {
        for (name, definition) in definitions {
            let field_type = build_rust_type(name, definition, &mut models);
            if field_type != *name {
                models.insert(
                    name.clone(),
                    RustModelTemplate {
                        name: name.clone(),
                        fields: vec![RustModelFieldTemplate {
                            optional: false,
                            required: false,
                            flatten: false,
                            wire_name_literal: "\"value\"".into(),
                            field_name: "value".into(),
                            field_type,
                        }],
                        union: false,
                        deny_unknown_fields: false,
                        transparent: true,
                        has_default: true,
                        variants: vec![],
                    },
                );
            }
        }
    }
    let response_type = build_rust_type(root_name, schema, &mut models);
    // Deriving Default across required recursive edges would recurse forever.

    let mut defaults = BTreeMap::new();
    for name in models.keys() {
        can_default(name, &models, &mut BTreeSet::new(), &mut defaults);
    }
    for (name, model) in &mut models {
        model.has_default = defaults[name];
    }
    let mut aliases = Vec::new();

    if !models.contains_key(&response_type) {
        aliases.push(RustTypeAliasTemplate {
            name: rust_type_name(root_name),
            target_type: response_type.clone(),
        });
    }

    (
        models.into_values().collect(),
        aliases,
        rust_type_name(root_name),
    )
}

// Strip only a representable nullable alternative; other unions retain JSON values.
pub(crate) fn non_null_schema(schema: &JsonValue) -> Option<JsonValue> {
    let mut variant = schema.clone();
    let mut nullable = variant.get("nullable") == Some(&JsonValue::Bool(true));
    variant.as_object_mut()?.remove("nullable");
    if let Some(types) = variant.get("type").and_then(JsonValue::as_array) {
        let non_null = types
            .iter()
            .filter(|value| value.as_str() != Some("null"))
            .cloned()
            .collect::<Vec<_>>();
        if non_null.len() != types.len() && !non_null.is_empty() {
            nullable = true;
            variant["type"] = if non_null.len() == 1 {
                non_null[0].clone()
            } else {
                JsonValue::Array(non_null)
            };
        }
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(alternatives) = variant.get(key).and_then(JsonValue::as_array)
            && alternatives.len() == 2
            && alternatives
                .iter()
                .any(|value| value.get("type").and_then(JsonValue::as_str) == Some("null"))
        {
            return alternatives
                .iter()
                .find(|value| value.get("type").and_then(JsonValue::as_str) != Some("null"))
                .cloned();
        }
    }
    nullable.then_some(variant)
}

pub(crate) fn build_rust_type(
    type_name: &str,
    schema: &JsonValue,
    models: &mut BTreeMap<String, RustModelTemplate>,
) -> String {
    if let Some(name) = schema
        .get(super::schema::REFERENCE)
        .and_then(JsonValue::as_str)
    {
        return format!("Box<{name}>");
    }
    // An intersection can narrow its constituent type but cannot admit a
    // different value shape. Retain a concrete referenced/typed constituent
    // when the containing schema contributes validation-only constraints.
    // The wire codec still evaluates every intersection constraint.
    if !["type", "properties", "const", "enum", "oneOf", "anyOf"]
        .iter()
        .any(|key| schema.get(*key).is_some())
        && let Some(parts) = schema.get("allOf").and_then(JsonValue::as_array)
        && let Some(part) = parts
            .iter()
            .find(|part| part.get(super::schema::REFERENCE).is_some() || part.get("type").is_some())
    {
        return build_rust_type(type_name, part, models);
    }
    if let Some(variant) = non_null_schema(schema) {
        return format!(
            "Option<{}>",
            build_rust_type(&format!("{type_name}Value"), &variant, models)
        );
    }
    if let Some(alternatives) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(JsonValue::as_array)
    {
        return build_rust_union(type_name, alternatives, models);
    }
    if let Some(types) = schema.get("type").and_then(JsonValue::as_array)
        && types.len() > 1
    {
        let alternatives = types
            .iter()
            .map(|kind| {
                let mut variant = schema.clone();
                variant["type"] = kind.clone();
                variant
            })
            .collect::<Vec<_>>();
        return build_rust_union(type_name, &alternatives, models);
    }
    if let Some(value) = schema.get("const") {
        return match value {
            JsonValue::Null => "()",
            JsonValue::Bool(_) => "bool",
            JsonValue::Number(number) if number.is_i64() => "i64",
            JsonValue::Number(_) => "f64",
            JsonValue::String(_) => "String",
            _ => "serde_json::Value",
        }
        .into();
    }

    match schema_type(schema) {
        Some("object")
            if schema
                .get("properties")
                .and_then(JsonValue::as_object)
                .is_none_or(serde_json::Map::is_empty) =>
        {
            let value_type = schema
                .get("additionalProperties")
                .filter(|value| value.is_object())
                .map_or_else(
                    || "serde_json::Value".into(),
                    |value| build_rust_type(&format!("{type_name}Value"), value, models),
                );
            format!("BTreeMap<String, {value_type}>")
        }
        Some("object") => rust_model_from_schema(type_name, schema, models),
        Some("array") => {
            let item_type_name = format!("{}Item", rust_type_name(type_name));
            let item_type = schema.get("items").map_or_else(
                || "serde_json::Value".to_string(),
                |items| build_rust_type(&item_type_name, items, models),
            );
            let item_type = item_type
                .strip_prefix("Box<")
                .and_then(|value| value.strip_suffix('>'))
                .unwrap_or(&item_type);
            format!("Vec<{item_type}>")
        }
        Some("integer") => "i64".to_string(),
        Some("number") => "f64".to_string(),
        Some("string") => "String".to_string(),
        Some("boolean") => "bool".to_string(),
        Some("null") => "()".to_string(),
        _ if schema.get("properties").is_some() => {
            rust_model_from_schema(type_name, schema, models)
        }
        _ => "serde_json::Value".to_string(),
    }
}

pub(crate) fn build_rust_union(
    type_name: &str,
    alternatives: &[JsonValue],
    models: &mut BTreeMap<String, RustModelTemplate>,
) -> String {
    if alternatives.is_empty() {
        return "serde_json::Value".into();
    }
    let base = rust_type_name(type_name);
    let mut name = base.clone();
    let mut suffix = 2;
    while models.contains_key(&name) {
        name = format!("{base}{suffix}");
        suffix += 1;
    }
    models.insert(
        name.clone(),
        RustModelTemplate {
            name: name.clone(),
            fields: vec![],
            union: true,
            deny_unknown_fields: false,
            transparent: false,
            has_default: true,
            variants: vec![],
        },
    );
    let variants = alternatives
        .iter()
        .enumerate()
        .map(|(index, schema)| RustUnionVariantTemplate {
            name: format!("Variant{}", index + 1),
            value_type: build_rust_type(&format!("{name}Variant{}", index + 1), schema, models),
            first: index == 0,
        })
        .collect();
    models.get_mut(&name).unwrap().variants = variants;
    name
}

pub(crate) fn rust_model_from_schema(
    type_name: &str,
    schema: &JsonValue,
    models: &mut BTreeMap<String, RustModelTemplate>,
) -> String {
    let base_name = rust_type_name(type_name);
    let mut model_name = base_name.clone();
    let mut suffix = 2;
    while models.contains_key(&model_name) {
        model_name = format!("{base_name}{suffix}");
        suffix += 1;
    }
    // Reserve the name before nested fields allocate their own models.
    models.insert(
        model_name.clone(),
        RustModelTemplate {
            name: model_name.clone(),
            fields: vec![],
            union: false,
            deny_unknown_fields: false,
            transparent: false,
            has_default: true,
            variants: vec![],
        },
    );

    let required = schema
        .get("required")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .collect::<BTreeSet<_>>();
    let mut fields = Vec::new();
    let mut field_names = BTreeSet::new();

    if let Some(properties) = schema.get("properties").and_then(JsonValue::as_object) {
        for (name, property_schema) in properties {
            let base_field_name = rust_field_ident(name);
            let mut field_name = base_field_name.clone();
            let mut suffix = 2;
            while !field_names.insert(field_name.clone()) {
                field_name = format!("{}_{suffix}", base_field_name.trim_end_matches('_'));
                suffix += 1;
            }
            let nested_type_name = format!("{}{}", model_name, field_name.to_case(Case::Pascal));
            let mut field_type = build_rust_type(&nested_type_name, property_schema, models);
            if !required.contains(name.as_str()) {
                field_type = format!("Option<{field_type}>");
            }
            fields.push(RustModelFieldTemplate {
                optional: !required.contains(name.as_str()),
                required: required.contains(name.as_str()),
                flatten: false,
                wire_name_literal: format!("{name:?}"),
                field_name,
                field_type,
            });
        }
    }

    if schema.get("additionalProperties") != Some(&JsonValue::Bool(false)) {
        let mut field_name = "additional_properties".to_string();
        let mut suffix = 2;
        while !field_names.insert(field_name.clone()) {
            field_name = format!("additional_properties_{suffix}");
            suffix += 1;
        }
        let value_type = schema
            .get("additionalProperties")
            .filter(|value| value.is_object())
            .map_or_else(
                || "serde_json::Value".into(),
                |value| build_rust_type(&format!("{model_name}AdditionalValue"), value, models),
            );
        fields.push(RustModelFieldTemplate {
            optional: false,
            required: false,
            flatten: true,
            wire_name_literal: "\"\"".into(),
            field_name,
            field_type: format!("BTreeMap<String, {value_type}>"),
        });
    }

    models.insert(
        model_name.clone(),
        RustModelTemplate {
            name: model_name.clone(),
            fields,
            union: false,
            deny_unknown_fields: schema.get("additionalProperties")
                == Some(&JsonValue::Bool(false)),
            transparent: false,
            has_default: true,
            variants: vec![],
        },
    );
    model_name
}

pub(crate) fn rust_type_name(input: &str) -> String {
    let mut name = input
        .to_case(Case::Pascal)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>();
    if name.is_empty() {
        name = "GeneratedModel".to_string();
    }
    if name.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        name.insert_str(0, "Model");
    }
    name
}

pub(crate) fn rust_field_ident(input: &str) -> String {
    let mut name = input
        .to_case(Case::Snake)
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if name.is_empty() || name.chars().all(|ch| ch == '_') {
        name = "field".to_string();
    }
    if name.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        name.insert(0, '_');
    }
    if is_rust_keyword(&name) {
        name.push('_');
    }
    name
}

pub(crate) fn is_rust_keyword(input: &str) -> bool {
    matches!(
        input,
        "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "gen"
            | "macro"
            | "override"
            | "priv"
            | "try"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "union"
            | "as"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
    )
}

pub(crate) fn escape_ts_string(input: &str) -> String {
    let mut escaped = String::new();
    for ch in input.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\'' => escaped.push_str("\\'"),
            '\u{0}'..='\u{1f}' | '\u{2028}' | '\u{2029}' => {
                let _ = write!(escaped, "\\u{:04x}", u32::from(ch));
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn can_default(
    name: &str,
    models: &BTreeMap<String, RustModelTemplate>,
    active: &mut BTreeSet<String>,
    memo: &mut BTreeMap<String, bool>,
) -> bool {
    if let Some(result) = memo.get(name) {
        return *result;
    }
    if active.len() >= 64 || !active.insert(name.into()) {
        return false;
    }
    let model = &models[name];
    let fields = if model.union {
        model
            .variants
            .first()
            .map(|variant| variant.value_type.as_str())
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        model
            .fields
            .iter()
            .map(|field| field.field_type.as_str())
            .collect()
    };
    let result = fields.into_iter().all(|ty| {
        let ty = ty
            .strip_prefix("Box<")
            .and_then(|value| value.strip_suffix('>'))
            .unwrap_or(ty);
        !models.contains_key(ty) || can_default(ty, models, active, memo)
    });
    active.remove(name);
    memo.insert(name.into(), result);
    result
}

#[derive(Content)]
struct ModelSourceTemplate<'a> {
    has_optional_fields: bool,
    has_required_fields: bool,
    models: &'a [RustModelTemplate],
    aliases: &'a [RustTypeAliasTemplate],
}

/// Render Serde models, preserving wire field names and omission/null presence.
/// The caller supplies `use serde::{Deserialize, Serialize};` when models exist.
///
/// # Errors
/// Returns an error if the bundled model template is invalid.
pub fn render_rust_models(
    models: &[RustModelTemplate],
    aliases: &[RustTypeAliasTemplate],
) -> anyhow::Result<String> {
    let data = ModelSourceTemplate {
        has_optional_fields: models
            .iter()
            .flat_map(|model| &model.fields)
            .any(|field| field.optional),
        has_required_fields: models
            .iter()
            .flat_map(|model| &model.fields)
            .any(|field| field.required),
        models,
        aliases,
    };
    Ok(ramhorns::Template::new(include_str!("models.rs.template"))?.render(&data))
}
