//! Trusted local key configuration and bounded legacy metadata normalization.
use super::{BlueprintResult, Value, json, key_error};
/// Supported local signing profile. Defaults to `EdDSA` and 2048-bit RSA generation.
#[derive(Clone, Debug)]
pub struct KeyPairConfig {
    algorithm: String,
    bits: usize,
    rotation_millis: Option<i64>,
    additional: Vec<Self>,
}
impl Default for KeyPairConfig {
    fn default() -> Self {
        Self {
            algorithm: "EdDSA".into(),
            bits: 2048,
            rotation_millis: None,
            additional: Vec::new(),
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
            rotation_millis: None,
            additional: Vec::new(),
        })
    }
    /// Advertises additional profiles eligible for lazy algorithm-constrained minting.
    /// # Errors
    /// Rejects more than 16 profiles, duplicate/primary algorithms, nested lists,
    /// and per-extra rotation intervals; the shared parent interval applies.
    pub fn with_additional_profiles(mut self, profiles: Vec<Self>) -> BlueprintResult<Self> {
        let mut seen = std::collections::BTreeSet::new();
        seen.insert(self.algorithm.clone());
        if profiles.len() > 16
            || profiles.iter().any(|profile| {
                !profile.additional.is_empty()
                    || profile.rotation_millis.is_some()
                    || !seen.insert(profile.algorithm.clone())
            })
        {
            return Err(key_error());
        }
        self.additional = profiles;
        Ok(self)
    }
    pub(super) fn additional_profile(&self, algorithm: &str) -> Option<&Self> {
        self.additional
            .iter()
            .find(|profile| profile.algorithm == algorithm)
    }
    /// Sets automatic expiry for newly created local keys. Zero disables expiry.
    /// Existing records retain their persisted expiry; no timer or rewrite is added.
    /// # Errors
    /// Rejects submillisecond durations and values beyond JavaScript's Date range.
    pub fn with_rotation_interval(
        mut self,
        interval: std::time::Duration,
    ) -> BlueprintResult<Self> {
        if !interval.subsec_nanos().is_multiple_of(1_000_000)
            || interval.as_millis() > 8_640_000_000_000_000
        {
            return Err(key_error());
        }
        let millis = i64::try_from(interval.as_millis()).map_err(|_| key_error())?;
        self.rotation_millis = (millis != 0).then_some(millis);
        Ok(self)
    }
    pub(super) fn expiration(
        &self,
        now: super::DateTime<super::Utc>,
    ) -> BlueprintResult<Option<super::DateTime<super::Utc>>> {
        self.rotation_millis
            .map(|interval| {
                now.timestamp_millis()
                    .checked_add(interval)
                    .and_then(super::DateTime::from_timestamp_millis)
                    .ok_or_else(key_error)
            })
            .transpose()
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
mod algorithm_tests;
#[cfg(test)]
mod rotation_tests;
#[cfg(test)]
mod tests;
