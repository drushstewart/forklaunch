//! Opt-in compatibility with the TypeScript middleware's synchronous Basic login.
use crate::Principal;
use forklaunch_http::{ErrorKind, ErrorResponse};

/// Verify Basic credentials using the repository TypeScript parsing rules and a
/// trusted synchronous login callback. No principal is returned before login
/// succeeds. Passwords and decoded inputs are never included in errors.
///
/// This deliberately accepts Node's permissive base64 decoding and uses only the
/// first two colon-delimited fields. Use [`crate::parse_basic`] for the native
/// strict decoder and passwords containing colons. This function is never enabled
/// by default. Async/custom resource decoders and session validation are separate.
///
/// # Errors
/// Returns the TypeScript 401 format/missing or 403 login error. The caller owns
/// the HTTP representation and any subsequent session authorization.
pub fn verify_basic(
    header: Option<&str>,
    token_prefix: Option<&str>,
    login: impl FnOnce(&str, &str) -> bool,
) -> Result<Principal, ErrorResponse> {
    let header = header.ok_or_else(|| {
        ErrorResponse::new(ErrorKind::Unauthorized, "Authorization token required.")
    })?;
    let mut parts = header.split(' ');
    let prefix = parts.next().unwrap_or_default();
    let token = parts.next().ok_or_else(invalid_format)?;
    if prefix.is_empty() || prefix != token_prefix.unwrap_or("Basic") {
        return Err(invalid_format());
    }
    let bytes = decode_node_base64(token);
    let decoded = String::from_utf8_lossy(&bytes);
    let mut credentials = decoded.split(':');
    let username = credentials
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(invalid_format)?;
    let password = credentials
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(invalid_format)?;
    if !login(username, password) {
        return Err(ErrorResponse::new(
            ErrorKind::Forbidden,
            "Invalid Authorization login.",
        ));
    }
    Ok(Principal::new(username))
}

fn invalid_format() -> ErrorResponse {
    ErrorResponse::new(
        ErrorKind::Unauthorized,
        "Invalid Authorization token format.",
    )
}

// Node consumes each UTF-16 code unit's low byte, accepts both alphabets, skips
// non-alphabet bytes, stops at padding and discards incomplete trailing bits.
fn decode_node_base64(input: &str) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len().saturating_mul(3) / 4);
    let mut bits = 0_u32;
    let mut count = 0_u32;
    for unit in input.encode_utf16() {
        let byte = (unit & 0xff) as u8;
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => continue,
        };
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            output.push(((bits >> count) & 0xff) as u8);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::verify_basic;
    #[test]
    fn only_verified_credentials_create_a_principal() {
        let mut called = false;
        let result = verify_basic(
            Some("Basic dXNlcjpwYXNzOmV4dHJh"),
            None,
            |username, password| {
                called = true;
                assert_eq!((username, password), ("user", "pass"));
                false
            },
        );
        assert!(called);
        assert_eq!(result.unwrap_err().message, "Invalid Authorization login.");
        assert!(
            verify_basic(Some("basic dXNlcjpwYXNz"), None, |_, _| panic!(
                "invalid header must not call login"
            ))
            .is_err()
        );
        assert_eq!(
            crate::parse_basic("Basic dXNlcjpwYXNzOmV4dHJh"),
            Some(("user".into(), "pass:extra".into()))
        );
        assert!(crate::parse_basic("Basic dX!NlcjpwYXNz").is_none());
    }
}
