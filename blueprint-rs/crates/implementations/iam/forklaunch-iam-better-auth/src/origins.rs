//! Better Auth trusted-origin and callback matching. Patterns are trusted config.
use percent_encoding::percent_decode_str;
use regex::Regex;
use std::{borrow::Cow, fmt::Write};
use url::Url;
mod policy;
pub use policy::{OriginPolicy, RequestOrigins};

/// Matches configured trusted origins, including glob hosts/custom-scheme paths.
/// Relative callbacks must be explicitly allowed; they never authenticate Origin.
#[must_use]
pub fn matches_origin(value: &str, pattern: &str, allow_relative: bool) -> bool {
    if value.starts_with('/') {
        return allow_relative && safe_relative(value);
    }
    let parsed = Url::parse(value).ok();
    let origin = parsed.as_ref().and_then(origin);
    if pattern.contains(['*', '?']) {
        let sample = if pattern.contains("://") {
            Some(origin.as_deref().unwrap_or(value).to_owned())
        } else {
            parsed.as_ref().and_then(host)
        };
        return sample.is_some_and(|sample| glob(pattern, &sample));
    }
    let Some(parsed) = parsed else { return false };
    if matches!(parsed.scheme(), "http" | "https") {
        return origin.as_deref() == Some(pattern);
    }
    let (Some(candidate), Some(trusted)) = (custom(value), custom(pattern)) else {
        return false;
    };
    candidate.scheme == trusted.scheme
        && (trusted.authority.is_empty() || candidate.authority == trusted.authority)
        && (trusted.path.is_empty()
            || candidate.path == trusted.path
            || candidate.path.starts_with(&format!("{}/", trusted.path)))
}

fn origin(url: &Url) -> Option<String> {
    let value = url.origin().ascii_serialization();
    (value != "null").then_some(value)
}
fn host(url: &Url) -> Option<String> {
    let name = url.host_str()?;
    Some(
        url.port()
            .map_or_else(|| name.to_owned(), |port| format!("{name}:{port}")),
    )
}
fn controls(value: &str) -> bool {
    value
        .chars()
        .any(|c| c <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&c))
}
fn safe_relative(value: &str) -> bool {
    if value.starts_with("//") || value.contains('\\') || controls(value) {
        return false;
    }
    let path = value
        .split(['?', '#'])
        .next()
        .unwrap_or(value)
        .to_ascii_lowercase();
    if path.contains("%2f") || path.contains("%5c") {
        return false;
    }
    Url::parse("https://better-auth.invalid")
        .ok()
        .and_then(|base| base.join(value).ok())
        .is_some_and(|url| url.origin().ascii_serialization() == "https://better-auth.invalid")
}
struct Custom {
    scheme: String,
    authority: String,
    path: String,
}
fn custom(value: &str) -> Option<Custom> {
    if controls(value) {
        return None;
    }
    let (scheme, mut rest) = value.split_once(':')?;
    if scheme.is_empty() {
        return None;
    }
    let authority = if let Some(stripped) = rest.strip_prefix("//") {
        let end = stripped.find(['/', '?', '#']).unwrap_or(stripped.len());
        rest = &stripped[end..];
        stripped[..end].to_lowercase()
    } else {
        String::new()
    };
    let path = rest.split(['?', '#']).next().unwrap_or(rest);
    let decoded = decode_component(path);
    let mut segments = Vec::new();
    for segment in decoded.split('/') {
        match segment {
            ".." => {
                segments.pop();
            }
            "" | "." => {}
            _ => segments.push(segment),
        }
    }
    Some(Custom {
        scheme: scheme.to_lowercase(),
        authority,
        path: if segments.is_empty() {
            String::new()
        } else {
            format!("/{}", segments.join("/"))
        },
    })
}
fn decode_component(value: &str) -> String {
    let bytes = value.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'%'
            && !bytes
                .get(index + 1..index + 3)
                .is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit))
        {
            return value.to_owned();
        }
    }
    percent_decode_str(value)
        .decode_utf8()
        .map_or_else(|_| value.to_owned(), Cow::into_owned)
}

// Port of Better Auth's default wildcard matcher: slash/backslash separators,
// segment-local */?, whole-segment **, escapes and optional trailing separators.
fn glob(pattern: &str, sample: &str) -> bool {
    let pattern = utf16_symbols(pattern);
    let sample = utf16_symbols(sample);
    let separator = r"[/\\]+?";
    let wildcard = r"[^/\\]";
    let segments: Vec<_> = pattern.split('/').collect();
    let mut expression = String::from("^");
    for (index, segment) in segments.iter().enumerate() {
        if segment.is_empty() && index > 0 {
            continue;
        }
        let current_separator = if index == segments.len() - 1 {
            r"[/\\]*?"
        } else if segments.get(index + 1) == Some(&"**") {
            ""
        } else {
            separator
        };
        if *segment == "**" {
            if !current_separator.is_empty() {
                if index != 0 {
                    expression.push_str(current_separator);
                }
                write!(expression, "(?:{wildcard}*?{current_separator})*?")
                    .expect("writing a String succeeds");
            }
            continue;
        }
        let mut chars = segment.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(c) = chars.next() {
                        expression.push_str(&regex::escape(&c.to_string()));
                    }
                }
                '?' => expression.push_str(wildcard),
                '*' => write!(expression, "{wildcard}*?").expect("writing a String succeeds"),
                _ => expression.push_str(&regex::escape(&c.to_string())),
            }
        }
        expression.push_str(current_separator);
    }
    expression.push_str(r"\z");
    Regex::new(&expression).is_ok_and(|regex| regex.is_match(&sample))
}

// JavaScript regular expressions without the Unicode flag consume UTF-16 units.
// Encode surrogate units as private supplementary scalars for Rust's regex engine.
fn utf16_symbols(value: &str) -> String {
    value
        .encode_utf16()
        .map(|unit| {
            char::from_u32(if (0xd800..=0xdfff).contains(&unit) {
                0xf0000 + u32::from(unit)
            } else {
                u32::from(unit)
            })
            .expect("mapped UTF-16 unit is scalar")
        })
        .collect()
}
