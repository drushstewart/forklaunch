//! Configured HS256 email-verification tokens and JOSE registered-claim checks.
use base64::{
    Engine, alphabet,
    engine::{
        DecodePaddingMode,
        general_purpose::{GeneralPurpose, GeneralPurposeConfig},
    },
};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::Serialize;
use serde_json::value::RawValue;
use sha2::Sha256;
use std::collections::HashMap;

type Object = HashMap<String, Box<RawValue>>;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EmailTokenError {
    InvalidToken,
    TokenExpired,
    InvalidClaims,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedEmailToken {
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_type: Option<String>,
}
/// Verifies the configured secret/algorithm before inspecting email claims.
/// `NumericDate` validation follows JOSE's optional iat/nbf/exp ordering. Raw JSON
/// numbers preserve JavaScript overflow semantics without changing stored JSON.
/// # Errors
/// Distinguishes malformed/forged JWTs, expired JWTs and the handler's claim-schema errors.
pub fn verify_email_token(
    token: &str,
    secret: &[u8],
    now: DateTime<Utc>,
) -> Result<VerifiedEmailToken, EmailTokenError> {
    let invalid = EmailTokenError::InvalidToken;
    let mut parts = token.split('.');
    let header = parts.next().ok_or(invalid)?;
    let payload = parts.next().ok_or(invalid)?;
    let signature = parts.next().ok_or(invalid)?;
    if parts.next().is_some() {
        return Err(invalid);
    }
    let protected = object(&decode(header)?)?;
    if string(&protected, "alg").as_deref() != Some("HS256") {
        return Err(invalid);
    }
    if let Some(crit) = protected.get("crit") {
        let crit: Vec<String> = serde_json::from_str(crit.get()).map_err(|_| invalid)?;
        if crit.is_empty() || crit.iter().any(|value| value != "b64") {
            return Err(invalid);
        }
        if protected
            .get("b64")
            .is_none_or(|value| value.get() != "true")
        {
            return Err(invalid);
        }
    }
    let signature = decode(signature)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).map_err(|_| invalid)?;
    mac.update(header.as_bytes());
    mac.update(b".");
    mac.update(payload.as_bytes());
    mac.verify_slice(&signature).map_err(|_| invalid)?;
    let claims = object(&decode(payload)?)?;
    numeric(&claims, "iat")?;
    // Chrono supports fewer than 2^53 whole seconds, so this cast is exact.
    #[allow(clippy::cast_precision_loss)]
    let now = now.timestamp() as f64;
    if numeric(&claims, "nbf")?.is_some_and(|nbf| nbf > now) {
        return Err(invalid);
    }
    if numeric(&claims, "exp")?.is_some_and(|exp| exp <= now) {
        return Err(EmailTokenError::TokenExpired);
    }
    let malformed = EmailTokenError::InvalidClaims;
    let email = string(&claims, "email").ok_or(malformed)?;
    if crate::accounts::validate_email(&email).is_err() {
        return Err(malformed);
    }
    let optional = |field| match claims.get(field) {
        None => Ok(None),
        Some(value) => serde_json::from_str::<String>(value.get())
            .map(Some)
            .map_err(|_| malformed),
    };
    Ok(VerifiedEmailToken {
        email,
        update_to: optional("updateTo")?,
        request_type: optional("requestType")?,
    })
}
fn object(bytes: &[u8]) -> Result<Object, EmailTokenError> {
    let value = std::str::from_utf8(bytes).map_err(|_| EmailTokenError::InvalidToken)?;
    serde_json::from_str(value.strip_prefix('\u{feff}').unwrap_or(value))
        .map_err(|_| EmailTokenError::InvalidToken)
}
fn string(object: &Object, field: &str) -> Option<String> {
    serde_json::from_str(object.get(field)?.get()).ok()
}
fn numeric(object: &Object, field: &str) -> Result<Option<f64>, EmailTokenError> {
    object
        .get(field)
        .map(|value| {
            value
                .get()
                .parse::<f64>()
                .map_err(|_| EmailTokenError::InvalidToken)
        })
        .transpose()
}
fn decode(value: &str) -> Result<Vec<u8>, EmailTokenError> {
    let clean: String = value
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\u{c}' | '\r' | ' '))
        .collect();
    let data = clean.trim_end_matches('=');
    let padding = clean.len() - data.len();
    if padding > 2
        || padding > 0 && (!clean.len().is_multiple_of(4) || padding != (4 - data.len() % 4) % 4)
    {
        return Err(EmailTokenError::InvalidToken);
    }
    GeneralPurpose::new(
        &alphabet::URL_SAFE,
        GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::RequireNone)
            .with_decode_allow_trailing_bits(true),
    )
    .decode(data)
    .map_err(|_| EmailTokenError::InvalidToken)
}
