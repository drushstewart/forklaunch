//! Portable bounds that Schemars otherwise publishes only as format annotations.
use schemars::{
    JsonSchema,
    schema::{InstanceType, RootSchema, SchemaObject},
    visit::{self, Visitor},
};

pub(super) fn root<T: JsonSchema>() -> RootSchema {
    let mut root = schemars::schema_for!(T);
    SmallIntegerBounds.visit_root_schema(&mut root);
    if root.schema.object.is_some() {
        root.schema.extensions.insert(
            "x-forklaunch-model".into(),
            serde_json::json!({
                "version": 1, "id": T::schema_id(), "name": T::schema_name(), "projection": "value"
            }),
        );
    }
    root
}

/// Attach identity only to the named DTO's own object definition.
#[doc(hidden)]
#[must_use]
pub fn named_schema<T: JsonSchema>(
    mut schema: schemars::schema::Schema,
) -> schemars::schema::Schema {
    if let schemars::schema::Schema::Object(object) = &mut schema
        && object.object.is_some()
    {
        object.extensions.insert(
            "x-forklaunch-model".into(),
            serde_json::json!({
                "version":1,"id":T::schema_id(),"name":T::schema_name(),"projection":"value"
            }),
        );
    }
    schema
}

struct SmallIntegerBounds;
impl Visitor for SmallIntegerBounds {
    fn visit_schema_object(&mut self, schema: &mut SchemaObject) {
        // Serde defaults loosen raw input only; canonical encoded values always
        // include these supported scalar members.
        if let Some(object) = schema.object.as_mut() {
            for (name, property) in &object.properties {
                if matches!(property,schemars::schema::Schema::Object(property) if property.extensions.contains_key(super::codec::DEFAULT))
                {
                    object.required.insert(name.clone());
                }
            }
        }

        // Only integer assertions carry native numeric bounds. A format-looking
        // string annotation, enum literal, example or default is never rewritten.
        if schema
            .instance_type
            .as_ref()
            .is_some_and(|ty| ty.contains(&InstanceType::Integer))
        {
            // Platform-sized and wide unsigned formats have the same lower
            // bound. Intersect rather than replace authored tighter limits;
            // aliases and optional members retain these formats from Schemars.
            if matches!(
                schema.format.as_deref(),
                Some("uint" | "uint64" | "uint128")
            ) {
                let number = schema.number();
                number.minimum = Some(number.minimum.unwrap_or(0.0).max(0.0));
            }
            let bounds = match schema.format.as_deref() {
                Some("uint8") => Some((0.0, f64::from(u8::MAX))),
                Some("uint16") => Some((0.0, f64::from(u16::MAX))),
                Some("uint32") => Some((0.0, f64::from(u32::MAX))),
                Some("int8") => Some((f64::from(i8::MIN), f64::from(i8::MAX))),
                Some("int16") => Some((f64::from(i16::MIN), f64::from(i16::MAX))),
                Some("int32") => Some((f64::from(i32::MIN), f64::from(i32::MAX))),
                _ => None,
            };
            if let Some((minimum, maximum)) = bounds {
                let number = schema.number();
                number.minimum = Some(number.minimum.map_or(minimum, |value| value.max(minimum)));
                number.maximum = Some(number.maximum.map_or(maximum, |value| value.min(maximum)));
            }
        }
        visit::visit_schema_object(self, schema);
    }
}

/// Schema-only provenance adapter; it never replaces the application's field type.
#[doc(hidden)]
pub struct NativeMember<T: ?Sized>(std::marker::PhantomData<T>);
impl<T: JsonSchema + ?Sized> JsonSchema for NativeMember<T> {
    fn is_referenceable() -> bool {
        !unbound_date::<T>() && T::is_referenceable()
    }
    fn schema_name() -> String {
        T::schema_name()
    }
    fn schema_id() -> std::borrow::Cow<'static, str> {
        T::schema_id()
    }
    fn json_schema(generator: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
        if unbound_date::<T>() {
            return rejected_date_alias();
        }
        T::json_schema(generator)
    }
    fn _schemars_private_non_optional_json_schema(
        generator: &mut schemars::r#gen::SchemaGenerator,
    ) -> schemars::schema::Schema {
        if unbound_date::<T>() {
            return rejected_date_alias();
        }
        T::_schemars_private_non_optional_json_schema(generator)
    }
    fn _schemars_private_is_option() -> bool {
        T::_schemars_private_is_option()
    }
}
fn unbound_date<T: ?Sized>() -> bool {
    std::any::type_name::<T>().contains(std::any::type_name::<
        super::datetime::DateTime<super::datetime::Utc>,
    >())
}
fn rejected_date_alias() -> schemars::schema::Schema {
    SchemaObject {
        subschemas: Some(Box::new(schemars::schema::SubschemaValidation {
            not: Some(Box::new(true.into())),
            ..Default::default()
        })),
        extensions: std::collections::BTreeMap::from([(
            "x-forklaunch-unbound-native-date".into(),
            serde_json::Value::Bool(true),
        )]),
        ..Default::default()
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bounds_preserve_narrower_constraints_and_literal_annotations() {
        let mut root: RootSchema = serde_json::from_value(json!({
            "type":"integer","format":"int16","minimum":-5,"maximum":7,
            "examples":[{"type":"integer","format":"uint8"}],
            "definitions":{"text":{"type":"string","format":"uint8"}}
        }))
        .unwrap();
        SmallIntegerBounds.visit_root_schema(&mut root);
        let value = serde_json::to_value(root).unwrap();
        assert_eq!(value["minimum"].as_f64(), Some(-5.0));
        assert_eq!(value["maximum"].as_f64(), Some(7.0));
        assert!(value["definitions"]["text"].get("maximum").is_none());
        assert!(value["examples"][0].get("maximum").is_none());
    }
}
