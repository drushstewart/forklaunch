//! Google RS256 ID-token signature, issuer/audience, nonce and one-hour age policy.
use crate::keyring::verify::{compact, decode_jwk, truthy, verify_signature};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Verifies against public keys fetched from the trusted Google certificate endpoint.
/// Token-provided URLs never determine the key source. Caller supplies server time.
/// Missing or invalid claims/keys return None, without provisioning local JWT keys.
#[must_use]
pub fn verify_google_id_token(
    token: &str,
    client_id: &str,
    nonce: Option<&str>,
    jwks: &Value,
    now: DateTime<Utc>,
) -> Option<Value> {
    let (header, payload, signature, content) = compact(token)?;
    if header["alg"] != "RS256" {
        return None;
    }
    let keys = jwks["keys"].as_array()?;
    let selected: Vec<_> = keys
        .iter()
        .filter(|key| {
            !truthy(&header["kid"])
                || !header["kid"].is_object()
                    && !header["kid"].is_array()
                    && key["kid"] == header["kid"]
        })
        .collect();
    // The provider imports every selected JWK before it tries any signature.
    if selected.is_empty() || selected.iter().any(|key| !importable(key)) {
        return None;
    }
    if !selected
        .iter()
        .any(|key| verify_signature(key, "RS256", &header, content.as_bytes(), &signature))
    {
        return None;
    }
    if !matches!(
        payload["iss"].as_str(),
        Some("https://accounts.google.com" | "accounts.google.com")
    ) || payload["aud"] != client_id
        && !payload["aud"]
            .as_array()
            .is_some_and(|audiences| audiences.iter().any(|audience| audience == client_id))
    {
        return None;
    }
    let clock = serde_json::json!(now.timestamp()).as_f64()?;
    let issued = number(payload.get("iat")?)?;
    let age = clock - issued;
    if !(0.0..=3600.0).contains(&age) {
        return None;
    }
    if let Some(not_before) = payload.get("nbf")
        && number(not_before)? > clock
    {
        return None;
    }
    if let Some(expires) = payload.get("exp")
        && number(expires)? <= clock
    {
        return None;
    }
    if let Some(nonce) = nonce.filter(|nonce| !nonce.is_empty())
        && payload["nonce"] != nonce
    {
        return None;
    }
    Some(payload)
}
fn number(value: &Value) -> Option<f64> {
    let number = value.as_number()?;
    number.as_f64().or_else(|| number.to_string().parse().ok())
}
fn importable(key: &Value) -> bool {
    if !key.is_object()
        || key["kty"] != "RSA"
        || key.get("ext").is_some_and(|value| !value.is_boolean())
    {
        return false;
    }
    let private = key.get("d").is_some_and(truthy);
    if key.get("priv").is_some_and(truthy) && !private {
        return false;
    }
    if key.get("d").is_some() && !private {
        return false;
    }
    let fields: &[&str] = if private {
        &["n", "e", "d", "p", "q", "dp", "dq", "qi"]
    } else {
        &["n", "e"]
    };
    if fields.iter().any(|field| {
        key.get(*field).is_none()
            || crate::ecmascript::string(key.get(*field))
                .and_then(|value| decode_jwk(&value))
                .is_none()
    }) {
        return false;
    }
    key.get("key_ops").is_none_or(|ops| {
        !private && ops == &serde_json::json!([])
            || ops == &serde_json::json!([if private { "sign" } else { "verify" }])
    })
}
