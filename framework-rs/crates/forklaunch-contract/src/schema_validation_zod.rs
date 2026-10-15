//! Reserved exact Zod check assertions; never reinterpret ordinary schema keywords.
//! Input strings are Rust UTF-8 strings, so lone JavaScript UTF-16 surrogates are
//! outside this representation domain. Numbers use their finite IEEE-754 value.
use forklaunch_common::zod_multiple_of::ZodMultipleOf;
use jsonschema::{Keyword, ValidationError, paths::Location};
use serde_json::{Map, Value};

pub(super) const KEYWORD: &str = "x-forklaunch-zod-checks";
struct Checks {
    minimum: Option<u64>,
    maximum: Option<u64>,
    divisors: Vec<ZodMultipleOf>,
    formats: Vec<Format>,
    time_precisions: Vec<Option<u16>>,
}
enum Format {
    Email,
    Uuid,
    Cuid,
    Cuid2,
    Ulid,
    Nanoid,
    Base64,
    Base64Url,
    Date,
    Duration,
    #[cfg(feature = "zod-uri")]
    Uri,
}
impl Format {
    fn accepts(&self, value: &str) -> bool {
        match self {
            Self::Email => forklaunch_common::zod_string_format::email(value),
            Self::Uuid => forklaunch_common::zod_string_format::uuid(value),
            Self::Cuid => forklaunch_common::zod_string_format::cuid(value),
            Self::Cuid2 => forklaunch_common::zod_string_format::cuid2(value),
            Self::Ulid => forklaunch_common::zod_string_format::ulid(value),
            Self::Nanoid => forklaunch_common::zod_string_format::nanoid(value),
            Self::Base64 => forklaunch_common::zod_string_format::base64(value),
            Self::Base64Url => forklaunch_common::zod_string_format::base64url(value),
            Self::Duration => forklaunch_common::zod_string_format::duration(value),
            Self::Date => forklaunch_common::zod_string_format::date(value),
            #[cfg(feature = "zod-uri")]
            Self::Uri => ada_url::Url::can_parse(value, None),
        }
    }
}
fn invalid<'a>() -> ValidationError<'a> {
    ValidationError::custom("invalid x-forklaunch-zod-checks version, fields or limits")
}
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn bound(value: Option<&Value>) -> Result<Option<u64>, ValidationError<'static>> {
    value
        .map(|value| {
            value
                .as_f64()
                .filter(|value| {
                    value.is_finite()
                        && *value >= 0.0
                        && *value <= 9_007_199_254_740_991.0
                        && value.fract() == 0.0
                })
                .map(|value| value as u64)
                .ok_or_else(invalid)
        })
        .transpose()
}
fn time_precisions(value: Option<&Value>) -> Result<Vec<Option<u16>>, ValidationError<'static>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .filter(|v| v.len() <= 10_000)
        .ok_or_else(invalid)?;
    values
        .iter()
        .map(|value| {
            if value.is_null() {
                return Ok(None);
            }
            bound(Some(value))?
                .filter(|v| *v <= 1000)
                .and_then(|v| u16::try_from(v).ok())
                .map(Some)
                .ok_or_else(invalid)
        })
        .collect()
}
pub(super) fn compile<'a>(
    _: &'a Map<String, Value>,
    value: &'a Value,
    _: Location,
) -> Result<Box<dyn for<'i> Keyword<'i>>, ValidationError<'a>> {
    let fields = value.as_object().ok_or_else(invalid)?;
    if fields.get("version").and_then(Value::as_f64) != Some(1.0)
        || fields.keys().any(|key| {
            !matches!(
                key.as_str(),
                "version"
                    | "utf16MinLength"
                    | "utf16MaxLength"
                    | "multipleOf"
                    | "formats"
                    | "timePrecisions"
            )
        })
    {
        return Err(invalid());
    }
    let time_precisions = time_precisions(fields.get("timePrecisions"))?;
    let minimum = bound(fields.get("utf16MinLength"))?;
    let maximum = bound(fields.get("utf16MaxLength"))?;
    let raw_formats = fields
        .get("formats")
        .map(|value| {
            value
                .as_array()
                .filter(|values| values.len() <= 10_000)
                .ok_or_else(invalid)
        })
        .transpose()?;
    let raw_divisors = fields
        .get("multipleOf")
        .map(|value| {
            value
                .as_array()
                .filter(|values| values.len() <= 10_000)
                .ok_or_else(invalid)
        })
        .transpose()?;
    if raw_formats.map_or(0, Vec::len) + raw_divisors.map_or(0, Vec::len) + time_precisions.len()
        > 10_000
    {
        return Err(invalid());
    }
    let mut formats = Vec::new();
    for value in raw_formats.into_iter().flatten() {
        formats.push(match value.as_str() {
            Some("email") => Format::Email,
            Some("uuid") => Format::Uuid,
            Some("cuid") => Format::Cuid,
            Some("cuid2") => Format::Cuid2,
            Some("ulid") => Format::Ulid,
            Some("nanoid") => Format::Nanoid,
            Some("base64") => Format::Base64,
            Some("base64url") => Format::Base64Url,
            Some("duration") => Format::Duration,
            Some("date") => Format::Date,
            #[cfg(feature = "zod-uri")]
            Some("uri") => Format::Uri,
            #[cfg(not(feature = "zod-uri"))]
            Some("uri") => {
                return Err(ValidationError::custom(
                    "URI checks require the forklaunch-contract zod-uri feature",
                ));
            }
            _ => return Err(invalid()),
        });
    }
    let mut divisors = Vec::new();
    if let Some(values) = fields.get("multipleOf") {
        let values = values
            .as_array()
            .filter(|values| values.len() <= 10_000)
            .ok_or_else(invalid)?;
        for value in values {
            let divisor = match value {
                Value::Number(value) => value
                    .as_f64()
                    .filter(|value| value.is_finite())
                    .ok_or_else(invalid)?,
                Value::String(value) if value == "NaN" => f64::NAN,
                Value::String(value) if value == "Infinity" => f64::INFINITY,
                Value::String(value) if value == "-Infinity" => f64::NEG_INFINITY,
                _ => return Err(invalid()),
            };
            divisors.push(ZodMultipleOf::new(divisor));
        }
    }
    Ok(Box::new(Checks {
        minimum,
        maximum,
        divisors,
        formats,
        time_precisions,
    }))
}
impl<'i> Keyword<'i> for Checks {
    fn is_valid(&self, instance: &'i Value) -> bool {
        if let Some(value) = instance.as_str() {
            if !self
                .time_precisions
                .iter()
                .all(|p| forklaunch_common::zod_string_format::time(value, *p))
            {
                return false;
            }
            if !self.formats.iter().all(|format| format.accepts(value)) {
                return false;
            }
            let length = value.encode_utf16().count() as u64;
            if self.minimum.is_some_and(|minimum| length < minimum)
                || self.maximum.is_some_and(|maximum| length > maximum)
            {
                return false;
            }
        }
        if !self.divisors.is_empty()
            && let Some(value) = instance.as_number()
        {
            return value
                .as_f64()
                .filter(|value| value.is_finite())
                .is_some_and(|value| self.divisors.iter().all(|divisor| divisor.accepts(value)));
        }
        true
    }
    fn validate(&self, instance: &'i Value) -> Result<(), ValidationError<'i>> {
        if self.is_valid(instance) {
            Ok(())
        } else {
            Err(ValidationError::custom(
                "value violates exact Zod check assertions",
            ))
        }
    }
}
