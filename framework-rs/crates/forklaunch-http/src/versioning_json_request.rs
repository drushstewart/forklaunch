//! Opt-in, bounded unversioned JSON parsing, independent of schema validation.
use super::{
    BodyParserHooks, charset, invalid_body, parse_json_body, validate_parser_options, wire,
};
use crate::{Body, ContractValidationError, FrameworkRequest};
use forklaunch_contract::{BodyKind, BodyParserOptions, RequestContract, RouteContract};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

/// Parsed JSON and decompressed bytes; the original wire body remains unchanged.
/// Both fields are absent when framing or the declared media type skips parsing.
/// A reviver may remove the root value while retaining the decompressed bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedJsonBody {
    pub value: Option<Arc<Value>>,
    pub raw_body: Option<Arc<Vec<u8>>>,
}

#[derive(Debug, Clone)]
struct Registration {
    request: RequestContract,
    options: BodyParserOptions,
    hooks: BodyParserHooks,
}

/// Explicit unversioned JSON parsers, bound to request declarations and parser options.
/// Parsing runs independently of request-schema validation mode. Native defaults
/// are unchanged unless registered. Only exact declared media types are supported.
/// Finite fractional and exponent values use correctly rounded IEEE-754 parsing.
/// Integral values outside the JavaScript safe-integer range, nonfinite values
/// and lone UTF-16 surrogates are rejected rather than silently changing values. This profile
/// does not implement arbitrary JavaScript values or JSON-schema validation.
/// Local callers must supply HTTP framing headers (`Content-Length` or
/// `Transfer-Encoding`); an unframed body is skipped, as on the HTTP transport.
/// MCP invocation is not affected by this HTTP parser registration.
#[derive(Debug, Clone, Default)]
pub struct JsonRequestParsers {
    entries: BTreeMap<String, Registration>,
}
impl JsonRequestParsers {
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Register one finite JSON parser, optionally with verifier/reviver callbacks.
    /// # Errors
    /// Rejects versioned/non-JSON routes, non-exact media types and duplicate entries.
    pub fn insert(&mut self, route: &RouteContract, hooks: BodyParserHooks) -> Result<(), String> {
        let body = route
            .request
            .body
            .as_ref()
            .ok_or("JSON parser requires a body declaration")?;
        if !route.versions.is_empty() || !matches!(body.kind, BodyKind::Json | BodyKind::Unknown) {
            return Err("JSON parser requires an unversioned JSON body".into());
        }
        let media = body.content_type.as_deref().unwrap_or("application/json");
        if media.parse::<mime::Mime>().is_err() || media.contains(['*', ';']) || !media.is_ascii() {
            return Err("JSON parser requires an exact media type without parameters".into());
        }
        validate_parser_options(&route.options.body_parser)?;
        let key = key(route);
        if self.entries.contains_key(&key) {
            return Err("Duplicate JSON parser operation".into());
        }
        self.entries.insert(
            key,
            Registration {
                request: route.request.clone(),
                options: route.options.body_parser.clone(),
                hooks,
            },
        );
        Ok(())
    }

    /// Validate registrations against original declarations before exposing a router.
    /// # Errors
    /// Rejects missing, ambiguous or changed registrations.
    pub fn validate_routes(&self, routes: &[RouteContract]) -> Result<(), String> {
        for (key, entry) in &self.entries {
            let mut matches = routes.iter().filter(|route| self::key(route) == *key);
            let route = matches.next().ok_or("Missing JSON parser route")?;
            if matches.next().is_some() || !matches_registration(route, entry) {
                return Err("JSON parser registration no longer matches its declaration".into());
            }
        }
        Ok(())
    }

    /// Merge registrations when composing routers.
    /// # Errors
    /// Rejects duplicate method/operation registrations without changing either entry.
    pub fn extend(&mut self, other: Self) -> Result<(), String> {
        if other
            .entries
            .keys()
            .any(|key| self.entries.contains_key(key))
        {
            return Err("Duplicate JSON parser operation".into());
        }
        self.entries.extend(other.entries);
        Ok(())
    }

    /// Parse a registered request. `None` means no parser is registered.
    /// Always parses the original body; previously supplied parsed values are not trusted.
    /// # Errors
    /// Returns a redacted parser error for malformed/unsupported input or stale metadata.
    pub fn parse(
        &self,
        route: &RouteContract,
        request: &FrameworkRequest,
    ) -> Result<Option<ParsedJsonBody>, ContractValidationError> {
        let Some(entry) = self.entries.get(&key(route)) else {
            return Ok(None);
        };
        if !matches_registration(route, entry) {
            return Err(invalid_body("stale JSON parser registration"));
        }
        let framed = crate::header_value(&request.headers, "transfer-encoding").is_some()
            || crate::header_value(&request.headers, "content-length")
                .is_some_and(|value| value.parse::<u64>().is_ok());
        let expected = entry
            .request
            .body
            .as_ref()
            .and_then(|body| body.content_type.as_deref())
            .unwrap_or("application/json");
        let media_matches =
            crate::header_value(&request.headers, "content-type").is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .eq_ignore_ascii_case(expected)
            });
        if !framed || !media_matches {
            return Ok(Some(ParsedJsonBody {
                value: None,
                raw_body: None,
            }));
        }
        let charset = charset::charset(
            crate::header_value(&request.headers, "content-type"),
            &entry.options.json_default_charset,
        );
        if !charset.starts_with("utf-") {
            return Err(charset::unsupported());
        }
        charset::decode(&[], &charset)?;
        let bytes = match &request.body {
            Body::Empty => &[][..],
            Body::Text(value) => value.as_bytes(),
            Body::Binary(value) => value,
        };
        let decoded = wire::decode(
            bytes,
            crate::header_value(&request.headers, "content-encoding"),
            &entry.options.json_wire,
        )?;
        entry.hooks.verify(request, &decoded, Some(&charset))?;
        // Check the admitted input domain before a reviver can hide unsupported values.
        let original = parse_json_body(
            &decoded,
            &charset,
            entry.options.json_strict,
            &BodyParserHooks::default(),
        )?;
        if original
            .as_ref()
            .is_some_and(|value| !supported_numbers(value))
        {
            return Err(invalid_body(
                "JSON number is outside the supported safe-integer domain",
            ));
        }
        let value = if entry.hooks.has_json_reviver() {
            parse_json_body(&decoded, &charset, entry.options.json_strict, &entry.hooks)?
        } else {
            original
        };
        Ok(Some(ParsedJsonBody {
            value: value.map(Arc::new),
            raw_body: Some(Arc::new(decoded.into_owned())),
        }))
    }
}
fn supported_numbers(value: &Value) -> bool {
    match value {
        Value::Number(number) => number.as_f64().is_some_and(|value| {
            value.is_finite() && (value.fract() != 0.0 || value.abs() <= 9_007_199_254_740_991.0)
        }),
        Value::Array(values) => values.iter().all(supported_numbers),
        Value::Object(values) => values.values().all(supported_numbers),
        _ => true,
    }
}
fn matches_registration(route: &RouteContract, entry: &Registration) -> bool {
    route.versions.is_empty()
        && route.request == entry.request
        && route.options.body_parser == entry.options
}
fn key(route: &RouteContract) -> String {
    format!("{} {}", route.method.as_str(), route.operation_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HttpMethod, RequestContext};
    use forklaunch_contract::BodyContract;
    fn route() -> RouteContract {
        RouteContract::post("Parse", "Parse", "/parse")
            .with_request_body(BodyContract::json("Input"))
            .with_response(200, None)
    }
    fn request(body: &str) -> FrameworkRequest {
        FrameworkRequest::new(HttpMethod::Post, "/parse", RequestContext::new("test"))
            .with_header("content-type", "application/json")
            .with_header("content-length", body.len().to_string())
            .with_body(Body::Text(body.into()))
    }
    #[test]
    fn framed_empty_and_media_skips_are_distinct_and_keep_original_bytes() {
        let route = route();
        let mut parsers = JsonRequestParsers::default();
        parsers.insert(&route, BodyParserHooks::default()).unwrap();
        let empty = parsers.parse(&route, &request("")).unwrap().unwrap();
        assert_eq!(empty.value.as_deref(), Some(&serde_json::json!({})));
        assert_eq!(empty.raw_body.as_deref(), Some(&Vec::new()));
        let mut absent = request("");
        absent.headers.remove("content-length");
        assert_eq!(
            parsers.parse(&route, &absent).unwrap().unwrap().raw_body,
            None
        );
        let wrong = request("{").with_header("content-type", "application/problem+json");
        assert_eq!(parsers.parse(&route, &wrong).unwrap().unwrap().value, None);
        let original = request("{\"amount\":12.99}");
        let before = original.clone();
        assert_eq!(
            parsers
                .parse(&route, &original)
                .unwrap()
                .unwrap()
                .value
                .unwrap()["amount"],
            12.99
        );
        assert_eq!(original, before);
    }
    #[test]
    fn bounded_numeric_unicode_and_reviver_admission() {
        let route = route();
        let mut parsers = JsonRequestParsers::default();
        parsers.insert(&route, BodyParserHooks::default()).unwrap();
        for body in [
            "{\"n\":9007199254740993}",
            "{\"n\":1e400}",
            "{\"n\":1e20}",
            "{\"s\":\"\\ud800\"}",
        ] {
            assert!(parsers.parse(&route, &request(body)).is_err(), "{body}");
        }
        for body in [
            "{\"n\":12.99}",
            "{\"n\":5e-324}",
            "{\"n\":-0}",
            "{\"n\":1e2}",
            "{\"s\":\"\\ud83d\\ude00\"}",
        ] {
            assert!(parsers.parse(&route, &request(body)).is_ok(), "{body}");
        }
        let mut reviver = JsonRequestParsers::default();
        reviver
            .insert(
                &route,
                BodyParserHooks::default().with_json_reviver(|_, _| Ok(None)),
            )
            .unwrap();
        assert!(
            reviver
                .parse(&route, &request("{\"n\":9007199254740993}"))
                .is_err()
        );
        let removed = reviver.parse(&route, &request("{}")).unwrap().unwrap();
        assert!(removed.value.is_none());
        assert!(removed.raw_body.is_some());
    }
    #[test]
    fn direct_local_registration_parses_before_validation_and_handler() {
        let route = route();
        let mut parsers = JsonRequestParsers::default();
        parsers.insert(&route, BodyParserHooks::default()).unwrap();
        let mut router = crate::LocalRouter::new();
        router.register(route, |request: FrameworkRequest| {
            assert_eq!(
                request.parsed_json.unwrap().value.as_deref(),
                Some(&serde_json::json!({}))
            );
            Ok(crate::FrameworkResponse::text(200, "ok"))
        });
        let execute = |body| {
            router
                .execute_with_json_parsers(
                    request(body),
                    crate::ParserErrorFormat::TypeScriptText,
                    crate::ResponseStatusErrorFormat::Json,
                    &crate::DefaultErrorResponses::default(),
                    &parsers,
                )
                .unwrap()
        };
        assert_eq!(execute("").status_code, 200);
        let invalid = execute("{");
        assert_eq!(invalid.status_code, 400);
        assert!(invalid.body.as_text().unwrap().contains("test"));
    }
    #[test]
    fn registration_validation_is_bound_to_request_and_parser_options() {
        let route = route();
        let mut parsers = JsonRequestParsers::default();
        parsers.insert(&route, BodyParserHooks::default()).unwrap();
        assert!(parsers.insert(&route, BodyParserHooks::default()).is_err());
        assert!(parsers.validate_routes(&[]).is_err());
        let mut changed = route.clone();
        changed.options.body_parser.json_strict = false;
        assert!(parsers.validate_routes(&[changed]).is_err());
        assert!(
            parsers
                .validate_routes(&[route.clone(), route.clone()])
                .is_err()
        );
        assert!(parsers.validate_routes(&[route]).is_ok());
    }
}
