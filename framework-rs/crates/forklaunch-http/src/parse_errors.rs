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
