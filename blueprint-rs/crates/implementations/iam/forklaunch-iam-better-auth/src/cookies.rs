//! Better Call signed session-cookie wire protocol.
use base64::{Engine, alphabet, engine::general_purpose};
use hmac::{Hmac, Mac};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use sha2::Sha256;
use std::{borrow::Cow, fmt::Write};

// JavaScript encodeURIComponent, including its additional unescaped punctuation.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// Signs and URI-encodes a session token, matching Better Call's cookie value.
#[must_use]
pub fn sign_cookie_value(value: &str, secret: &[u8]) -> String {
    let mut mac = signing_mac(secret);
    mac.update(value.as_bytes());
    let signature = general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    utf8_percent_encode(&format!("{value}.{signature}"), COMPONENT).to_string()
}

/// Verifies a previously URI-decoded signed cookie and returns its token.
/// Use `cookie_value` to extract/decode a Cookie header first.
#[must_use]
pub fn verify_cookie_value<'a>(value: &'a str, secret: &[u8]) -> Option<&'a str> {
    let (token, signature) = value.rsplit_once('.')?;
    if token.is_empty() || signature.len() != 44 || !signature.ends_with('=') {
        return None;
    }
    // WebCrypto's atob accepts noncanonical unused padding bits.
    let decoder = general_purpose::GeneralPurpose::new(
        &alphabet::STANDARD,
        general_purpose::GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
    );
    let signature = decoder.decode(signature).ok()?;
    let mut mac = signing_mac(secret);
    mac.update(token.as_bytes());
    mac.verify_slice(&signature).ok()?;
    Some(token)
}

/// Extracts the first named cookie, including quoted and URI-encoded values.
/// Invalid percent escapes/UTF-8 preserve the original value, like Better Call.
#[must_use]
pub fn cookie_value(header: &str, name: &str) -> Option<String> {
    let value = header.split(';').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        (key.trim() == name).then(|| value.trim())
    })?;
    let value = if value.starts_with('"') {
        // Better Call removes the first and last characters of a quoted value.
        value.get(1..value.len().saturating_sub(1)).unwrap_or("")
    } else {
        value
    };
    let bytes = value.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'%'
            && !bytes
                .get(index + 1..index + 3)
                .is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit))
        {
            return Some(value.into());
        }
    }
    Some(
        percent_decode_str(value)
            .decode_utf8()
            .map_or_else(|_| value.into(), Cow::into_owned),
    )
}

/// Cookie options for the configured 24-hour persistent session profile.
#[derive(Clone, Copy, Debug)]
pub struct SessionCookie {
    pub secure: bool,
}

impl SessionCookie {
    #[must_use]
    pub const fn dont_remember_name(self) -> &'static str {
        if self.secure {
            "__Secure-better-auth.dont_remember"
        } else {
            "better-auth.dont_remember"
        }
    }

    #[must_use]
    pub fn transient_flag(self, secret: &[u8]) -> String {
        self.named_header(
            self.dont_remember_name(),
            &sign_cookie_value("true", secret),
            None,
        )
    }

    /// Clears all configured browser auth cookies. Each returned value must be
    /// appended as a separate Set-Cookie header, never joined with commas.
    #[must_use]
    pub fn clear_all(self) -> Vec<String> {
        let prefix = if self.secure {
            "__Secure-better-auth."
        } else {
            "better-auth."
        };
        [
            "session_token",
            "session_data",
            "oauth_state",
            "dont_remember",
        ]
        .iter()
        .map(|suffix| self.named_header(&format!("{prefix}{suffix}"), "", Some(0)))
        .collect()
    }
    /// Clears the three session cookies used by Better Auth's sign-out endpoint.
    /// An in-progress OAuth state cookie is independent of signing out.
    #[cfg(feature = "http")]
    pub(crate) fn clear_session(self) -> Vec<String> {
        let prefix = if self.secure {
            "__Secure-better-auth."
        } else {
            "better-auth."
        };
        ["session_token", "session_data", "dont_remember"]
            .iter()
            .map(|suffix| self.named_header(&format!("{prefix}{suffix}"), "", Some(0)))
            .collect()
    }
    #[must_use]
    pub const fn name(self) -> &'static str {
        if self.secure {
            "__Secure-better-auth.session_token"
        } else {
            "better-auth.session_token"
        }
    }

    /// Creates the configured session Set-Cookie header. Transient sessions
    /// omit Max-Age; persistent sessions use exactly 24 hours.
    #[must_use]
    pub fn serialize(self, token: &str, secret: &[u8], persistent: bool) -> String {
        self.header(
            &sign_cookie_value(token, secret),
            persistent.then_some(86400),
        )
    }

    #[must_use]
    pub fn clear(self) -> String {
        self.header("", Some(0))
    }

    fn header(self, value: &str, max_age: Option<u32>) -> String {
        self.named_header(self.name(), value, max_age)
    }

    pub(crate) fn named_header(self, name: &str, value: &str, max_age: Option<u32>) -> String {
        let mut result = format!("{name}={value}");
        if let Some(max_age) = max_age {
            write!(result, "; Max-Age={max_age}").expect("writing to a String cannot fail");
        }
        result.push_str("; Path=/; HttpOnly");
        if self.secure {
            result.push_str("; Secure");
        }
        result.push_str("; SameSite=Lax");
        result
    }
}

fn signing_mac(secret: &[u8]) -> Hmac<Sha256> {
    Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key size")
}
