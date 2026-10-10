//! Form webhook HMAC-SHA1 verification following Twilio's SDK algorithm.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::collections::BTreeMap;

pub(crate) fn validate_callback_url(url: &str) -> BlueprintResult<()> {
    let parsed = reqwest::Url::parse(url).map_err(|_| invalid())?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
        || url.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    Ok(())
}
fn invalid() -> BlueprintError {
    BlueprintError::new(
        "twilio_invalid_signature",
        "Twilio callback could not be authenticated",
    )
}
fn canonical(url: &str, params: &[(String, String)]) -> BlueprintResult<String> {
    if params.len() > 1024
        || params
            .iter()
            .map(|(k, v)| k.len().saturating_add(v.len()))
            .sum::<usize>()
            > 1024 * 1024
    {
        return Err(invalid());
    }
    let mut fields: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (name, value) in params {
        fields.entry(name).or_default().push(value);
    }
    // JavaScript Array.sort compares UTF-16 code units, including astral keys.
    let mut fields: Vec<_> = fields.into_iter().collect();
    fields.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
    let mut data = url.to_owned();
    for (name, mut values) in fields {
        values.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        values.dedup();
        for value in values {
            data.push_str(name);
            data.push_str(value);
        }
    }
    Ok(data)
}
/// Computes the signature for fixtures or a trusted provider relay. Incoming
/// webhooks must use `verify`, which compares the decoded MAC in constant time.
/// # Errors
/// Rejects empty tokens and oversized forms.
pub fn expected_signature(
    auth_token: &str,
    url: &str,
    params: &[(String, String)],
) -> BlueprintResult<String> {
    if auth_token.is_empty() {
        return Err(invalid());
    }
    let mut mac = Hmac::<Sha1>::new_from_slice(auth_token.as_bytes()).map_err(|_| invalid())?;
    mac.update(canonical(url, params)?.as_bytes());
    Ok(STANDARD.encode(mac.finalize().into_bytes()))
}
/// Verifies the exact configured external URL and every supplied form field.
/// Replays are possible in Twilio's protocol; callbacks must be safe to repeat.
/// # Errors
/// Returns a sanitized authentication error for a malformed or mismatched MAC.
pub fn verify(
    auth_token: &str,
    external_url: &str,
    params: &[(String, String)],
    signature: &str,
) -> BlueprintResult<()> {
    validate_callback_url(external_url)?;
    if auth_token.is_empty() || signature.len() != 28 {
        return Err(invalid());
    }
    let decoded = STANDARD.decode(signature).map_err(|_| invalid())?;
    let mut mac = Hmac::<Sha1>::new_from_slice(auth_token.as_bytes()).map_err(|_| invalid())?;
    mac.update(canonical(external_url, params)?.as_bytes());
    mac.verify_slice(&decoded).map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_golden_signature_and_tamper_cases() {
        let url = "https://example.com/myapp.php?foo=1&bar=2";
        let params: Vec<_> = [
            ("CallSid", "CA1234567890ABCDE"),
            ("Caller", "+14158675310"),
            ("Digits", "1234"),
            ("From", "+14158675310"),
            ("To", "+18005551212"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
        let signature = "L/OH5YylLD5NRKLltdqwSvS0BnU=";
        assert_eq!(
            expected_signature("12345", url, &params).unwrap(),
            signature
        );
        verify("12345", url, &params, signature).unwrap();
        assert!(verify("wrong", url, &params, signature).is_err());
        assert!(verify("12345", "https://example.com/other", &params, signature).is_err());
        let mut extra = params.clone();
        extra.push(("Extra".into(), "injected".into()));
        assert!(verify("12345", url, &extra, signature).is_err());
        assert!(verify("12345", url, &params, "invalid").is_err());
        assert!(verify("12345", "http://example.com/", &params, signature).is_err());
    }
    #[test]
    fn duplicate_values_and_unicode_use_javascript_sort_order() {
        let params = vec![
            ("x".into(), "b".into()),
            ("x".into(), "a".into()),
            ("x".into(), "b".into()),
            ("\u{e000}".into(), "z".into()),
            ("💬".into(), "v".into()),
        ];
        assert_eq!(canonical("url", &params).unwrap(), "urlxaxb💬v\u{e000}z");
    }
}
