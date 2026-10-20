//! Bounded keyring configuration shared by service and worker bootstrappers.
use super::{EnvelopeError, EnvelopeKey, EnvelopeProfile};
use std::{collections::BTreeMap, env::VarError};
use zeroize::{Zeroize, Zeroizing};

impl EnvelopeKey {
    /// Load the standard `ENCRYPTION_*` variables. Missing, malformed or oversized
    /// configuration fails closed without including secret values in errors.
    ///
    /// Writes use v3 with tenant-derived keys. v2 reads use the configured key IDs.
    /// Legacy reads default to the active key for initial adoption;
    /// retain the original key explicitly on rotation, or set the legacy value to
    /// `disabled` after all v1 data has been migrated.
    ///
    /// # Errors
    /// Rejects missing, malformed or excessive key configuration without exposing values.
    pub fn from_env(profile: EnvelopeProfile) -> Result<Self, EnvelopeError> {
        from_values(profile, |name| std::env::var(name))
    }
}

fn from_values(
    profile: EnvelopeProfile,
    mut read: impl FnMut(&str) -> Result<String, VarError>,
) -> Result<EnvelopeKey, EnvelopeError> {
    let active =
        Zeroizing::new(read("ENCRYPTION_KEY_HEX").map_err(|_| EnvelopeError::InvalidInput)?);
    let active_key = decode_key(&active)?;
    let active_id = optional(&mut read, "ENCRYPTION_KEY_ID")?.unwrap_or_else(|| "primary".into());
    let raw =
        Zeroizing::new(optional(&mut read, "ENCRYPTION_PREVIOUS_KEYS_JSON")?.unwrap_or_default());
    if raw.len() > 2048 {
        return Err(EnvelopeError::InvalidInput);
    }
    let mut previous = SecretMap(
        serde_json::from_str(if raw.is_empty() { "{}" } else { &raw })
            .map_err(|_| EnvelopeError::InvalidInput)?,
    );
    if previous.0.len() > 4 {
        return Err(EnvelopeError::InvalidInput);
    }
    let mut keys = Vec::new();
    for (id, value) in &mut previous.0 {
        let value = Zeroizing::new(std::mem::take(value));
        keys.push((id.clone(), decode_key(&value)?));
    }
    let legacy = optional(&mut read, "ENCRYPTION_LEGACY_KEY_HEX")?.map(Zeroizing::new);
    let legacy = match legacy.as_deref().map(String::as_str) {
        None | Some("") => Some(active_key),
        Some("disabled") => None,
        Some(value) => Some(decode_key(value)?),
    };
    EnvelopeKey::with_rotation(profile, active_id, active_key, keys, legacy)
}

fn optional(
    read: &mut impl FnMut(&str) -> Result<String, VarError>,
    name: &str,
) -> Result<Option<String>, EnvelopeError> {
    match read(name) {
        Ok(value) => Ok(Some(value)),
        Err(VarError::NotPresent) => Ok(None),
        Err(_) => Err(EnvelopeError::InvalidInput),
    }
}

struct SecretMap(BTreeMap<String, String>);
impl Drop for SecretMap {
    fn drop(&mut self) {
        for value in self.0.values_mut() {
            value.zeroize();
        }
    }
}
fn decode_key(encoded: &str) -> Result<[u8; 32], EnvelopeError> {
    if encoded.len() != 64 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(EnvelopeError::InvalidInput);
    }
    let mut key = [0u8; 32];
    for (index, pair) in encoded.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        key[index] = u8::from_str_radix(
            std::str::from_utf8(pair).map_err(|_| EnvelopeError::InvalidInput)?,
            16,
        )
        .map_err(|_| EnvelopeError::InvalidInput)?;
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn load(values: &[(&str, String)]) -> Result<EnvelopeKey, EnvelopeError> {
        from_values(EnvelopeProfile::new("test").unwrap(), |name| {
            values
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.clone())
                .ok_or(VarError::NotPresent)
        })
    }
    #[test]
    fn rotated_environment_preserves_old_reads_and_retires_keys() {
        let old = load(&[
            ("ENCRYPTION_KEY_HEX", "11".repeat(32)),
            ("ENCRYPTION_KEY_ID", "old".into()),
        ])
        .unwrap();
        let tenant = uuid::Uuid::new_v4();
        let item = uuid::Uuid::new_v4();
        let sealed = old.seal(tenant, item, "body", b"sensitive").unwrap();
        let values = [
            ("ENCRYPTION_KEY_HEX", "22".repeat(32)),
            ("ENCRYPTION_KEY_ID", "new".into()),
            (
                "ENCRYPTION_PREVIOUS_KEYS_JSON",
                format!("{{\"old\":\"{}\"}}", "11".repeat(32)),
            ),
            ("ENCRYPTION_LEGACY_KEY_HEX", "disabled".into()),
        ];
        let rotated = load(&values).unwrap();
        assert_eq!(
            &*rotated.open(tenant, item, "body", &sealed).unwrap(),
            b"sensitive"
        );
        let retired = load(&values[..2]).unwrap();
        assert!(retired.open(tenant, item, "body", &sealed).is_err());
    }
    #[test]
    fn invalid_configuration_never_falls_back_or_discloses_values() {
        for value in ["secret-invalid".to_string(), "é".repeat(32), "z".repeat(64)] {
            let error = load(&[("ENCRYPTION_KEY_HEX", value.clone())])
                .err()
                .unwrap();
            assert!(!error.to_string().contains(&value));
        }
        assert!(
            load(&[
                ("ENCRYPTION_KEY_HEX", "11".repeat(32)),
                ("ENCRYPTION_PREVIOUS_KEYS_JSON", " ".repeat(2049))
            ])
            .is_err()
        );
        assert!(
            load(&[
                ("ENCRYPTION_KEY_HEX", "11".repeat(32)),
                (
                    "ENCRYPTION_PREVIOUS_KEYS_JSON",
                    format!("{{\"primary\":\"{}\"}}", "22".repeat(32))
                )
            ])
            .is_err()
        );
        assert!(
            from_values(EnvelopeProfile::new("test").unwrap(), |name| {
                if name == "ENCRYPTION_KEY_HEX" {
                    Ok("11".repeat(32))
                } else {
                    Err(VarError::NotUnicode(std::ffi::OsString::new()))
                }
            })
            .is_err()
        );
    }
}
