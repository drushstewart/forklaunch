//! Trusted server-only verification using the persisted configured key set.
mod rsa;
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, KeyRing, Model, Utc,
    Value, json,
};
use base64::{
    Engine, alphabet,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
};
impl<S: AuthStore> KeyRing<S> {
    /// Verifies a token with the configured issuer/audience and persisted public keys.
    /// Invalid tokens and unavailable keys return a null payload without minting keys.
    /// # Errors
    /// Returns the configured server API schema errors for invalid input types.
    pub async fn verify_jwt(
        &self,
        body: Option<&Value>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        validate_body(body)?;
        let body = body.ok_or_else(|| validation("[body] Invalid input".into()))?;
        let token = body["token"]
            .as_str()
            .ok_or_else(|| validation("[body.token] Invalid input".into()))?;
        let issuer = body["issuer"]
            .as_str()
            .filter(|issuer| !issuer.is_empty())
            .unwrap_or(&self.base_url);
        let payload = self.verify_token(token, issuer, now).await.unwrap_or(None);
        Ok(json!({"payload":payload}))
    }
    async fn verify_token(
        &self,
        token: &str,
        issuer: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Option<Value>> {
        let Some((header, payload, signature, content)) = compact(token) else {
            return Ok(None);
        };
        let Some(kid) = header["kid"].as_str().filter(|kid| !kid.is_empty()) else {
            return Ok(None);
        };
        let mut tx = self.store.begin(false).await?;
        let row = tx
            .list(Model::Jwks)
            .await?
            .into_iter()
            .take(100)
            .find(|row| row["id"] == kid);
        tx.commit().await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let algorithm = if row["alg"].is_null() {
            "EdDSA"
        } else if let Some(algorithm) = row["alg"].as_str() {
            algorithm
        } else {
            return Ok(None);
        };
        let Some(jwk) = row["publicKey"]
            .as_str()
            .and_then(|text| crate::ecmascript::parse(text).ok())
        else {
            return Ok(None);
        };
        if !verify_signature(&jwk, algorithm, &header, content.as_bytes(), &signature) {
            return Ok(None);
        }
        if !claims(
            &payload,
            issuer,
            &self.base_url,
            json!(now.timestamp()).as_f64().expect("numeric date"),
        ) {
            return Ok(None);
        }
        Ok(Some(payload))
    }
}
pub(crate) fn verify_signature(
    jwk: &Value,
    algorithm: &str,
    header: &Value,
    content: &[u8],
    signature: &[u8],
) -> bool {
    if jwk.get("ext").is_some_and(|value| !value.is_boolean())
        || jwk
            .get("key_ops")
            .is_some_and(|value| value != &json!(["verify"]))
        || jwk.get("d").is_some()
        || jwk.get("priv").is_some_and(truthy)
    {
        return false;
    }
    if matches!(
        algorithm,
        "RS256" | "RS384" | "RS512" | "PS256" | "PS384" | "PS512"
    ) {
        return header["alg"] == algorithm && rsa::verify(jwk, algorithm, content, signature);
    }
    let Some(x) = jwk["x"].as_str().and_then(decode_jwk) else {
        return false;
    };
    let (verification, key): (&dyn ring::signature::VerificationAlgorithm, Vec<u8>) =
        match algorithm {
            "EdDSA" | "Ed25519"
                if matches!(header["alg"].as_str(), Some("EdDSA" | "Ed25519"))
                    && jwk["kty"] == "OKP"
                    && jwk["crv"] == "Ed25519" =>
            {
                (&ring::signature::ED25519, x)
            }
            "ES256" | "ES384" if header["alg"] == algorithm && jwk["kty"] == "EC" => {
                let (curve, width, verification): (
                    _,
                    _,
                    &dyn ring::signature::VerificationAlgorithm,
                ) = if algorithm == "ES256" {
                    ("P-256", 32, &ring::signature::ECDSA_P256_SHA256_FIXED)
                } else {
                    ("P-384", 48, &ring::signature::ECDSA_P384_SHA384_FIXED)
                };
                let Some(y) = jwk["y"].as_str().and_then(decode_jwk) else {
                    return false;
                };
                if jwk["crv"] != curve || x.len() != width || y.len() != width {
                    return false;
                }
                (verification, [vec![4], x, y].concat())
            }
            "ES512"
                if header["alg"] == algorithm && jwk["kty"] == "EC" && jwk["crv"] == "P-521" =>
            {
                use p521::ecdsa::signature::Verifier;
                let Some(y) = jwk["y"].as_str().and_then(decode_jwk) else {
                    return false;
                };
                if x.len() != 66 || y.len() != 66 {
                    return false;
                }
                let Ok(key) = p521::ecdsa::VerifyingKey::from_sec1_bytes(&[vec![4], x, y].concat())
                else {
                    return false;
                };
                let Ok(signature) = p521::ecdsa::Signature::from_slice(signature) else {
                    return false;
                };
                return key.verify(content, &signature).is_ok();
            }
            _ => return false,
        };
    ring::signature::UnparsedPublicKey::new(verification, key)
        .verify(content, signature)
        .is_ok()
}

pub(crate) fn compact(token: &str) -> Option<(Value, Value, Vec<u8>, &str)> {
    let parts: Vec<_> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let parse = |part| crate::ecmascript::parse(std::str::from_utf8(&decode(part)?).ok()?).ok();
    let header = parse(parts[0])?;
    if !header.is_object()
        || !matches!(
            header["alg"].as_str(),
            Some(
                "EdDSA"
                    | "Ed25519"
                    | "ES256"
                    | "ES384"
                    | "ES512"
                    | "RS256"
                    | "RS384"
                    | "RS512"
                    | "PS256"
                    | "PS384"
                    | "PS512"
            )
        )
    {
        return None;
    }
    if let Some(crit) = header.get("crit")
        && (crit
            .as_array()
            .is_none_or(|items| items.is_empty() || items.iter().any(|item| item != "b64"))
            || header["b64"] != true)
    {
        return None;
    }
    let payload = parse(parts[1])?;
    if !payload.is_object() {
        return None;
    }
    Some((
        header,
        payload,
        decode(parts[2])?,
        token.rsplit_once('.')?.0,
    ))
}
fn decode(value: &str) -> Option<Vec<u8>> {
    let value: String = value
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r' | '\u{c}' | ' '))
        .collect();
    // atob accepts omitted padding, but any explicit padding must be complete.
    if value.contains('=') && !value.len().is_multiple_of(4) {
        return None;
    }
    GeneralPurpose::new(
        &alphabet::URL_SAFE,
        GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    )
    .decode(value)
    .ok()
}
// Node's WebCrypto JWK import uses its forgiving base64 decoder, independently
// of JOSE's compact-token decoder. Keep the two boundaries distinct.
pub(crate) fn decode_jwk(value: &str) -> Option<Vec<u8>> {
    let mut encoded: String = value
        .split('=')
        .next()?
        .chars()
        .filter_map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '+' | '/' => Some(c),
            '-' => Some('+'),
            '_' => Some('/'),
            _ => None,
        })
        .collect();
    if encoded.len() % 4 == 1 {
        encoded.pop();
    }
    GeneralPurpose::new(
        &alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    )
    .decode(encoded)
    .ok()
}

fn claims(payload: &Value, issuer: &str, audience: &str, now: f64) -> bool {
    if payload["iss"] != issuer
        || payload["aud"].as_str().is_none_or(|aud| aud != audience)
            && !payload["aud"]
                .as_array()
                .is_some_and(|auds| auds.iter().any(|aud| aud == audience))
        || !truthy(&payload["sub"])
    {
        return false;
    }
    for claim in ["iat", "nbf", "exp"] {
        if let Some(value) = payload.get(claim) {
            let Some(number) = value
                .as_number()
                .and_then(|n| n.as_f64().or_else(|| n.to_string().parse().ok()))
            else {
                return false;
            };
            if claim == "nbf" && number > now || claim == "exp" && number <= now {
                return false;
            }
        }
    }
    true
}
pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::String(value) => !value.is_empty(),
        Value::Number(value) => value
            .as_f64()
            .or_else(|| value.to_string().parse().ok())
            .is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::Array(_) | Value::Object(_) => true,
    }
}
fn validate_body(body: Option<&Value>) -> BlueprintResult<()> {
    if !body.is_some_and(Value::is_object) {
        return Err(validation(format!(
            "[body] Invalid input: expected object, received {}",
            received(body)
        )));
    }
    let body = body.expect("validated object");
    let mut errors = Vec::new();
    for (field, required) in [("token", true), ("issuer", false)] {
        let value = body.get(field);
        if value.is_none() && !required {
            continue;
        }
        if !value.is_some_and(Value::is_string) {
            errors.push(format!(
                "[body.{field}] Invalid input: expected string, received {}",
                received(value)
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(validation(errors.join("; ")))
    }
}
fn validation(message: String) -> BlueprintError {
    BlueprintError::new("VALIDATION_ERROR", message)
}
fn received(value: Option<&Value>) -> &'static str {
    match value {
        None => "undefined",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
    }
}
