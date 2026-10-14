//! Trusted local key configuration and bounded legacy metadata normalization.
use super::{BlueprintResult, Value, json, key_error};
/// Supported local signing profile. Defaults to `EdDSA` and 2048-bit RSA generation.
#[derive(Clone, Debug)]
pub struct KeyPairConfig {
    algorithm: String,
    bits: usize,
}
impl Default for KeyPairConfig {
    fn default() -> Self {
        Self {
            algorithm: "EdDSA".into(),
            bits: 2048,
        }
    }
}
impl KeyPairConfig {
    /// # Errors
    /// Rejects unsupported algorithms and RSA sizes outside 2048..=4096 bits.
    pub fn new(algorithm: &str, rsa_bits: usize) -> BlueprintResult<Self> {
        if !matches!(
            algorithm,
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
        ) || !(2048..=4096).contains(&rsa_bits)
        {
            return Err(key_error());
        }
        Ok(Self {
            algorithm: algorithm.into(),
            bits: rsa_bits,
        })
    }
    #[must_use]
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }
    #[must_use]
    pub const fn rsa_bits(&self) -> usize {
        self.bits
    }
}
pub(super) fn algorithm<'a>(row: &'a Value, fallback: &'a str) -> BlueprintResult<&'a str> {
    match row.get("alg") {
        None | Some(Value::Null) => Ok(fallback),
        Some(Value::String(value)) => Ok(value),
        _ => Err(key_error()),
    }
}
pub(super) fn public(row: &Value, fallback: &str) -> BlueprintResult<Value> {
    if row.get("alg").is_some_and(|value| !value.is_null()) {
        return super::public_key(row);
    }
    let algorithm = algorithm(row, fallback)?;
    let encoded = row["publicKey"]
        .as_str()
        .filter(|value| value.len() <= 16384)
        .ok_or_else(key_error)?;
    let mut public: Value = serde_json::from_str(encoded).map_err(|_| key_error())?;
    let object = public.as_object_mut().ok_or_else(key_error)?;
    if !object.contains_key("alg") {
        object.insert("alg".into(), json!(algorithm));
    }
    if !object.contains_key("kid") {
        object.insert("kid".into(), row["id"].clone());
    }
    let mut normalized = json!({"id":row["id"], "alg":algorithm, "publicKey":serde_json::to_string(&public).map_err(|_| key_error())?});
    // Missing row curve inherits the actual public curve. Never invent missing
    // cryptographic coordinates or override a conflicting persisted curve.
    normalized["crv"] = row
        .get("crv")
        .filter(|value| !value.is_null())
        .unwrap_or(&public["crv"])
        .clone();
    super::public_key(&normalized)
}

#[cfg(test)]
mod tests;
