//! Bounded offline JSON Schema validation shared by HTTP and socket contracts.
use serde_json::Value;
use std::sync::Arc;

#[cfg(feature = "zod-regex")]
#[path = "schema_validation_regex.rs"]
mod bounded_regex;
#[cfg(feature = "zod-regex")]
pub use bounded_regex::RegexProfile;

/// Schema rejection is distinct from an operational resource failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaValidationFailure {
    Invalid(String),
    Operational,
}
impl std::fmt::Display for SchemaValidationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => f.write_str(message),
            Self::Operational => f.write_str("schema validation resource limit"),
        }
    }
}
impl std::error::Error for SchemaValidationFailure {}
pub type CheckedJsonValidator = dyn Fn(&Value) -> Result<(), SchemaValidationFailure> + Send + Sync;

/// Compile an operational-error-aware validator with ordinary offline defaults.
/// # Errors
/// Rejects malformed or unsupported schemas and compile budget exhaustion.
pub fn compile_checked(schema: &Value) -> Result<Arc<CheckedJsonValidator>, String> {
    compile_checked_profile(schema, false)
}

/// Checked multipart validation preserves the existing file assertion profile.
/// # Errors
/// Rejects malformed or unsupported schemas and compile budget exhaustion.
pub fn compile_multipart_checked(schema: &Value) -> Result<Arc<CheckedJsonValidator>, String> {
    compile_checked_profile(schema, true)
}

fn compile_checked_profile(
    schema: &Value,
    multipart: bool,
) -> Result<Arc<CheckedJsonValidator>, String> {
    #[cfg(feature = "zod-regex")]
    {
        compile_with_regex_profile(schema, multipart, RegexProfile::default())
    }
    #[cfg(not(feature = "zod-regex"))]
    {
        let validator = build_profile(schema, multipart)?;
        Ok(Arc::new(move |value| {
            validator
                .validate(value)
                .map_err(|e| SchemaValidationFailure::Invalid(e.to_string()))
        }))
    }
}

/// Configure bounded regex admission without exposing a boolean-only validator.
/// # Errors
/// Rejects malformed or unsupported schemas and compile budget exhaustion.
#[cfg(feature = "zod-regex")]
pub fn compile_with_regex_profile(
    schema: &Value,
    multipart: bool,
    profile: RegexProfile,
) -> Result<Arc<CheckedJsonValidator>, String> {
    let validator = build_profile(schema, multipart, Some(profile))?;
    Ok(Arc::new(move |value| {
        let _scope = bounded_regex::Scope::enter(profile.evaluation_work);
        let result = validator.validate(value);
        if bounded_regex::Scope::failed() {
            Err(SchemaValidationFailure::Operational)
        } else {
            result.map_err(|e| SchemaValidationFailure::Invalid(e.to_string()))
        }
    }))
}

#[cfg(feature = "zod-checks")]
#[path = "schema_validation_zod.rs"]
mod zod;

#[path = "schema_validation_file.rs"]
mod file;
pub use file::opaque_binary_schema;

pub type JsonValidator = dyn Fn(&Value) -> Result<(), String> + Send + Sync;

/// Compile the shared AJV-compatible validation profile once.
/// Missing dialect declarations use Draft 7; remote retrieval is disabled.
/// # Errors
/// Rejects malformed schemas, unresolved references and resource limits.
pub fn compile(schema: &Value) -> Result<Arc<JsonValidator>, String> {
    compile_profile(schema, false)
}

/// Compile multipart file assertions without rewriting resources or literal data.
/// All other JSON Schema behavior follows the shared offline profile.
/// # Errors
/// Rejects malformed schemas, unsupported file assertions and resource limits.
pub fn compile_multipart(schema: &Value) -> Result<Arc<JsonValidator>, String> {
    compile_profile(schema, true)
}

fn compile_profile(schema: &Value, multipart: bool) -> Result<Arc<JsonValidator>, String> {
    let validator = build_profile(
        schema,
        multipart,
        #[cfg(feature = "zod-regex")]
        None,
    )?;
    Ok(Arc::new(move |value| {
        validator.validate(value).map_err(|error| error.to_string())
    }))
}

fn build_profile(
    schema: &Value,
    multipart: bool,
    #[cfg(feature = "zod-regex")] regex: Option<RegexProfile>,
) -> Result<jsonschema::Validator, String> {
    schema_budget(schema)?;
    // Match the TypeScript AJV full-format profile: email domains
    // require a dot, rather than accepting local-only mailboxes.
    let email = regex::Regex::new(r"(?i)^[a-z0-9!#$%&'*+/=?^_`{|}~-]+(?:\.[a-z0-9!#$%&'*+/=?^_`{|}~-]+)*@(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$")
        .map_err(|error| format!("invalid email format pattern: {error}"))?;
    let uuid =
        regex::Regex::new(r"(?i)^(?:urn:uuid:)?[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$")
            .map_err(|error| format!("invalid UUID format pattern: {error}"))?;
    let mut options = jsonschema::options()
        .with_format("email", move |value| email.is_match(value))
        .with_format("uuid", move |value| uuid.is_match(value))
        .with_retriever(NoResources)
        .should_validate_formats(true)
        .with_pattern_options(
            jsonschema::PatternOptions::fancy_regex()
                .backtrack_limit(100_000)
                .size_limit(1024 * 1024)
                .dfa_size_limit(1024 * 1024),
        );
    #[cfg(feature = "zod-uri")]
    {
        options = options.with_format("forklaunch-whatwg-url", |value| {
            ada_url::Url::can_parse(value, None)
        });
    }
    #[cfg(feature = "zod-checks")]
    {
        options = options.with_keyword(zod::KEYWORD, zod::compile);
    }
    #[cfg(feature = "zod-regex")]
    if let Some(profile) = regex {
        let compiler = bounded_regex::Compiler::new(profile);
        options = options.with_keyword(bounded_regex::KEYWORD, move |parent, value, location| {
            compiler.compile(parent, value, location)
        });
    }
    options = options.with_schema_rewriter(move |schema| {
        if schema.get("x-forklaunch-zod-regex").is_some() {
            #[cfg(not(feature = "zod-regex"))]
            return Err("x-forklaunch-zod-regex requires the forklaunch-contract zod-regex feature and checked compiler".to_owned());
            #[cfg(feature = "zod-regex")]
            if regex.is_none() {
                return Err("x-forklaunch-zod-regex requires the checked schema compiler".to_owned());
            }
        }
        #[cfg(not(feature = "zod-checks"))]
        if schema.get("x-forklaunch-zod-checks").is_some() {
            return Err(
                "x-forklaunch-zod-checks requires the forklaunch-contract zod-checks feature"
                    .to_owned(),
            );
        }
        #[cfg(not(feature = "zod-uri"))]
        if schema.get("format").and_then(Value::as_str) == Some("forklaunch-whatwg-url") {
            return Err(
                "forklaunch-whatwg-url requires the forklaunch-contract zod-uri feature".to_owned(),
            );
        }
        if multipart {
            file::rewrite(schema)
        } else {
            Ok(None)
        }
    });
    if schema.get("$schema").is_none() {
        options = options.with_draft(jsonschema::Draft::Draft7);
    }
    // The isolated validator dependency opts into AJV fragment-token decoding
    // and older-draft $ref sibling constraints without rewriting schema data.
    let validator = options
        .build(schema)
        .map_err(|error| format!("invalid JSON schema: {error}"))?;
    Ok(validator)
}
struct NoResources;
impl jsonschema::Retrieve for NoResources {
    fn retrieve(
        &self,
        _uri: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err("JSON schema resource retrieval is disabled".into())
    }
}
fn schema_budget(schema: &Value) -> Result<(), String> {
    let mut pending = vec![(schema, 0)];
    let mut remaining = 100_000_usize;
    while let Some((value, depth)) = pending.pop() {
        if depth > 64 || remaining == 0 {
            return Err("JSON schema exceeds depth/node limits".to_owned());
        }
        remaining -= 1;
        let children = match value {
            Value::Array(values) => values.len(),
            Value::Object(values) => values.len(),
            _ => 0,
        };
        if children > remaining.saturating_sub(pending.len()) {
            return Err("JSON schema exceeds node limit".to_owned());
        }
        match value {
            Value::Array(values) => pending.extend(values.iter().map(|value| (value, depth + 1))),
            Value::Object(values) => {
                pending.extend(values.values().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
    }
    serde_json::to_writer(&mut SchemaBytes(32 * 1024 * 1024), schema)
        .map_err(|_| "JSON schema exceeds 32 MiB".to_owned())
}
struct SchemaBytes(usize);
impl std::io::Write for SchemaBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other("schema byte limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "schema_validation_uri_tests.rs"]
mod uri_tests;

#[cfg(test)]
#[path = "schema_validation_zod_tests.rs"]
mod zod_tests;

#[cfg(test)]
#[path = "schema_validation_regex_tests.rs"]
mod regex_tests;
