use fieldnotes_security::SecurityError;
pub(crate) fn validate(title: &str, body: &str) -> Result<(), SecurityError> {
    if title.trim().is_empty() || title.chars().count() > 512 || body.chars().count() > 65536 {
        Err(SecurityError::InvalidInput)
    } else {
        Ok(())
    }
}
