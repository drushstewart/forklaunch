use std::fmt::Write;

/// Public validator error, matching TypeScript's string-segment `ParseError` DTO.
/// Messages are public-facing; callers must redact private provider details.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ParseError {
    pub path: Vec<String>,
    pub message: String,
}

/// Formats errors exactly as TypeScript `prettyPrintParseErrors` for UTF-8 strings.
/// Missing and empty error lists produce the same summary. An empty prefix is
/// distinct from no prefix. Paths and messages retain whitespace and punctuation.
/// This formats supplied errors; it does not select a schema validator or alter
/// the native HTTP error envelope.
#[must_use]
pub fn pretty_print_parse_errors(errors: Option<&[ParseError]>, prefix: Option<&str>) -> String {
    let mut output = prefix.map_or_else(
        || "Validation failed".to_owned(),
        |prefix| format!("{prefix} validation failed"),
    );
    let errors = errors.unwrap_or_default();
    if errors.is_empty() {
        return output;
    }
    output.push_str(" with the following errors:\n");
    for (index, error) in errors.iter().enumerate() {
        if index > 0 {
            output.push_str("\n\n");
        }
        let path = if error.path.is_empty() {
            "root".to_owned()
        } else {
            error.path.join(" > ")
        };
        // Formatting into a String is infallible.
        write!(
            output,
            "{}. Path: {path}\n   Message: {}",
            index + 1,
            error.message
        )
        .expect("formatting into String");
    }
    output
}

/// Render supplied schema errors using TypeScript request middleware's wire format.
///
/// The body is plain text, matching the final Express response after the
/// enriched send middleware applies the default 400 string contract. Explicit
/// response media declarations can be applied with `with_header`. This opt-in
/// helper does not translate native validator messages into Zod messages.
/// Only pass public validation diagnostics. Missing correlation IDs use the
/// upstream fallback, while an explicitly empty ID remains empty.
#[must_use]
pub fn typescript_request_validation_error_response(
    errors: Option<&[ParseError]>,
    correlation_id: Option<&str>,
) -> forklaunch_http::FrameworkResponse {
    forklaunch_http::FrameworkResponse::text(
        400,
        format!(
            "{}\n\nCorrelation id: {}",
            pretty_print_parse_errors(errors, Some("Request")),
            correlation_id.unwrap_or("No correlation ID")
        ),
    )
    .with_header("content-type", "text/plain; charset=utf-8")
}

/// Render trusted response-validator diagnostics using TypeScript's error envelope.
/// `None` means that validation succeeded for that section; `Some(&[])` means
/// failure without individual diagnostics. Returns `None` when both sections
/// succeeded. Header diagnostics precede body diagnostics, as in TypeScript.
/// This opt-in formatter does not expose handler/provider errors or perform
/// schema validation. Callers must supply only public validator diagnostics.
#[must_use]
pub fn typescript_response_validation_error_response(
    header_errors: Option<&[ParseError]>,
    body_errors: Option<&[ParseError]>,
    correlation_id: Option<&str>,
) -> Option<forklaunch_http::FrameworkResponse> {
    let errors: Vec<_> = [(header_errors, "Header"), (body_errors, "Response")]
        .into_iter()
        .filter_map(|(errors, prefix)| {
            errors.map(|errors| pretty_print_parse_errors(Some(errors), Some(prefix)))
        })
        .collect();
    if errors.is_empty() {
        return None;
    }
    Some(
        forklaunch_http::FrameworkResponse::text(
            500,
            format!(
                "Invalid response:\n{}\n\nCorrelation id: {}",
                errors.join("\n\n"),
                correlation_id.unwrap_or("No correlation ID")
            ),
        )
        .with_header("content-type", "text/plain; charset=utf-8"),
    )
}
