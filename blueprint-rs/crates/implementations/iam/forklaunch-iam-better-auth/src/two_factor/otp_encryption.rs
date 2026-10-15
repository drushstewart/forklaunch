//! Better Auth field encryption; independent of generic tenant-row encryption.
use super::{BlueprintResult, TwoFactorService, error};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use std::{collections::BTreeMap, sync::Arc};
use zeroize::Zeroizing;

/// Trusted field-encryption keys. Only zeroized derived keys are retained; no
/// Debug or Serialize implementation exposes secrets. Versions are bounded u32.
pub struct OtpEncryptionKeys {
    legacy: Option<Zeroizing<[u8; 32]>>,
    versions: BTreeMap<u32, Zeroizing<[u8; 32]>>,
    current: Option<u32>,
}
fn derived(secret: &[u8]) -> Zeroizing<[u8; 32]> {
    let mut result = Zeroizing::new([0; 32]);
    result.copy_from_slice(ring::digest::digest(&ring::digest::SHA256, secret).as_ref());
    result
}
impl OtpEncryptionKeys {
    /// Uses bare nonce-prefixed hex without a version envelope.
    /// # Errors
    /// Rejects secrets outside 32..=4096 UTF-8 bytes.
    pub fn legacy(secret: String) -> BlueprintResult<Self> {
        let secret = Zeroizing::new(secret);
        validate_secret(&secret)?;
        Ok(Self::from_bytes(secret.as_bytes()))
    }
    /// Encrypts with current version; decrypts retained versions and optionally
    /// legacy bare hex. Does not rotate cookie signing keys or generic store keys.
    /// # Errors
    /// Rejects empty sets or more than 16 keys, duplicate or absent current versions, and
    /// secrets outside 32..=4096 UTF-8 bytes. Input strings are zeroized on drop.
    pub fn versioned(
        current: u32,
        keys: Vec<(u32, String)>,
        legacy: Option<String>,
    ) -> BlueprintResult<Self> {
        let keys: Vec<_> = keys
            .into_iter()
            .map(|(v, k)| (v, Zeroizing::new(k)))
            .collect();
        let legacy = legacy.map(Zeroizing::new);
        if keys.is_empty() || keys.len() > 16 {
            return Err(invalid_config());
        }
        let mut versions = BTreeMap::new();
        for (version, secret) in &keys {
            validate_secret(secret)?;
            if versions
                .insert(*version, derived(secret.as_bytes()))
                .is_some()
            {
                return Err(invalid_config());
            }
        }
        if !versions.contains_key(&current) {
            return Err(invalid_config());
        }
        let legacy = legacy
            .map(|secret| {
                validate_secret(&secret)?;
                Ok(derived(secret.as_bytes()))
            })
            .transpose()?;
        Ok(Self {
            legacy,
            versions,
            current: Some(current),
        })
    }
    pub(super) fn from_bytes(secret: &[u8]) -> Self {
        Self {
            legacy: Some(derived(secret)),
            versions: BTreeMap::new(),
            current: None,
        }
    }
    pub(super) fn encrypt(&self, plaintext: &str) -> BlueprintResult<Zeroizing<String>> {
        let nonce = super::crypto::random(24)?;
        self.encrypt_nonce(plaintext, &nonce)
    }
    fn encrypt_nonce(&self, plaintext: &str, nonce: &[u8]) -> BlueprintResult<Zeroizing<String>> {
        self.encrypt_nonce_bounded(plaintext, nonce, 32)
    }
    pub(super) fn encrypt_nonce_bounded(
        &self,
        plaintext: &str,
        nonce: &[u8],
        max_plaintext: usize,
    ) -> BlueprintResult<Zeroizing<String>> {
        if plaintext.len() > max_plaintext || nonce.len() != 24 {
            return Err(codec_failure());
        }
        let key = match self.current {
            Some(version) => self.versions.get(&version),
            None => self.legacy.as_ref(),
        }
        .ok_or_else(codec_failure)?;
        let cipher =
            XChaCha20Poly1305::new_from_slice(key.as_ref()).map_err(|_| codec_failure())?;
        let encrypted = cipher
            .encrypt(
                <&XNonce>::try_from(nonce).map_err(|_| codec_failure())?,
                plaintext.as_bytes(),
            )
            .map_err(|_| codec_failure())?;
        let bytes: Vec<_> = nonce.iter().chain(encrypted.iter()).copied().collect();
        let hex = hex_encode(&bytes);
        Ok(Zeroizing::new(
            self.current
                .map_or_else(|| hex.clone(), |v| format!("$ba${v}${hex}")),
        ))
    }
    pub(super) fn decrypt(&self, stored: &str) -> BlueprintResult<Zeroizing<String>> {
        self.decrypt_bounded(stored, 192, 144, None)
    }
    pub(super) fn decrypt_bounded(
        &self,
        stored: &str,
        max_stored: usize,
        max_hex: usize,
        max_prefix: Option<usize>,
    ) -> BlueprintResult<Zeroizing<String>> {
        if stored.len() > max_stored {
            return Err(codec_failure());
        }
        let (key, hex) = if self.current.is_none() {
            (self.legacy.as_ref().ok_or_else(codec_failure)?, stored)
        } else if let Some((version, hex)) = envelope(stored) {
            (self.versions.get(&version).ok_or_else(codec_failure)?, hex)
        } else {
            (self.legacy.as_ref().ok_or_else(codec_failure)?, stored)
        };
        if max_prefix.is_some_and(|max| stored.len() - hex.len() > max) {
            return Err(codec_failure());
        }
        let bytes = hex_decode_bounded(hex, max_hex)?;
        let cipher =
            XChaCha20Poly1305::new_from_slice(key.as_ref()).map_err(|_| codec_failure())?;
        let plain = Zeroizing::new(
            cipher
                .decrypt(
                    <&XNonce>::try_from(&bytes[..24]).map_err(|_| codec_failure())?,
                    &bytes[24..],
                )
                .map_err(|_| codec_failure())?,
        );
        Ok(Zeroizing::new(String::from_utf8_lossy(&plain).into_owned()))
    }
}
fn validate_secret(secret: &str) -> BlueprintResult<()> {
    if !(32..=4096).contains(&secret.len()) {
        return Err(invalid_config());
    }
    Ok(())
}
fn invalid_config() -> super::BlueprintError {
    error("better_auth_invalid_config", "Invalid OTP encryption keys")
}
fn codec_failure() -> super::BlueprintError {
    error("TWO_FACTOR_STORAGE_DECODE_FAILED", "Unable to decode OTP")
}
fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|b| {
            [
                char::from(HEX[usize::from(b >> 4)]),
                char::from(HEX[usize::from(b & 15)]),
            ]
        })
        .collect()
}
fn hex_decode(value: &str) -> BlueprintResult<Vec<u8>> {
    hex_decode_bounded(value, 144)
}
fn hex_decode_bounded(value: &str, maximum: usize) -> BlueprintResult<Vec<u8>> {
    if !(80..=maximum).contains(&value.len()) || !value.len().is_multiple_of(2) {
        return Err(codec_failure());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |v: u8| {
                char::from(v)
                    .to_digit(16)
                    .and_then(|v| u8::try_from(v).ok())
                    .ok_or_else(codec_failure)
            };
            Ok((digit(pair[0])? << 4) | digit(pair[1])?)
        })
        .collect()
}
fn whitespace(c: char) -> bool {
    matches!(c,'\u{0009}'..='\u{000d}'|'\u{0020}'|'\u{00a0}'|'\u{1680}'|'\u{2000}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{202f}'|'\u{205f}'|'\u{3000}'|'\u{feff}')
}
fn envelope(value: &str) -> Option<(u32, &str)> {
    let (version, hex) = value.strip_prefix("$ba$")?.split_once('$')?;
    let version = version.trim_start_matches(whitespace);
    let (negative, version) = if let Some(v) = version.strip_prefix('-') {
        (true, v)
    } else {
        (false, version.strip_prefix('+').unwrap_or(version))
    };
    let digits: String = version.chars().take_while(char::is_ascii_digit).collect();
    let version = digits.parse::<u32>().ok()?;
    if negative && version != 0 {
        return None;
    }
    Some((version, hex))
}
pub(super) fn recognizable(value: &str) -> bool {
    let hex = if value.starts_with("$ba$") {
        envelope(value).map(|(_, h)| h)
    } else {
        Some(value)
    };
    value.len() <= 192 && hex.is_some_and(|h| hex_decode(h).is_ok())
}
impl<S> TwoFactorService<S> {
    /// Installs independent field keys and enables encrypted OTP storage. Existing
    /// cookie signing and tenant encryption keys are unchanged.
    #[must_use]
    pub fn with_otp_encryption_keys(mut self, keys: Arc<OtpEncryptionKeys>) -> Self {
        self.otp_codec = None;
        self.otp_encryption_keys = Some(keys);
        self.otp_storage = super::OtpStorage::Encrypted;
        self
    }
    pub(super) fn encrypt_otp(&self, value: &str) -> BlueprintResult<Zeroizing<String>> {
        let fallback = OtpEncryptionKeys::from_bytes(&self.secret);
        self.otp_encryption_keys
            .as_deref()
            .unwrap_or(&fallback)
            .encrypt(value)
    }
    pub(super) fn decrypt_otp(&self, value: &str) -> BlueprintResult<Zeroizing<String>> {
        let fallback = OtpEncryptionKeys::from_bytes(&self.secret);
        self.otp_encryption_keys
            .as_deref()
            .unwrap_or(&fallback)
            .decrypt(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_otp_actual_node_vectors_and_config_bounds() {
        let vectors: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("otp_encryption_vectors.json")).unwrap();
        let nonce: Vec<u8> = (0..24).collect();
        for vector in vectors {
            let keys =
                OtpEncryptionKeys::legacy(vector["secret"].as_str().unwrap().into()).unwrap();
            let encrypted = keys
                .encrypt_nonce(vector["plaintext"].as_str().unwrap(), &nonce)
                .unwrap();
            assert_eq!(encrypted.as_str(), vector["ciphertext"].as_str().unwrap());
            assert_eq!(
                keys.decrypt(&encrypted).unwrap().as_str(),
                vector["plaintext"].as_str().unwrap()
            );
            assert_eq!(
                keys.decrypt(&encrypted.to_uppercase()).unwrap().as_str(),
                vector["plaintext"].as_str().unwrap()
            );
        }
        for secret in [String::new(), "x".repeat(31), "x".repeat(4097)] {
            assert!(OtpEncryptionKeys::legacy(secret).is_err());
        }
        for (current, keys) in [
            (1, vec![]),
            (2, vec![(1, "x".repeat(32))]),
            (1, vec![(1, "x".repeat(32)), (1, "y".repeat(32))]),
            (0, (0..17).map(|v| (v, "x".repeat(32))).collect()),
        ] {
            assert!(OtpEncryptionKeys::versioned(current, keys, None).is_err());
        }
    }
    #[test]
    fn encrypted_otp_versions_legacy_unknown_and_tampered() {
        let legacy = OtpEncryptionKeys::legacy("x".repeat(32)).unwrap();
        let raw = legacy.encrypt("123456").unwrap();
        let old = OtpEncryptionKeys::versioned(7, vec![(7, "y".repeat(32))], Some("x".repeat(32)))
            .unwrap();
        assert_eq!(old.decrypt(&raw).unwrap().as_str(), "123456");
        let blob = old.encrypt("123456").unwrap();
        assert!(blob.starts_with("$ba$7$"));
        let rotated =
            OtpEncryptionKeys::versioned(8, vec![(8, "z".repeat(32)), (7, "y".repeat(32))], None)
                .unwrap();
        assert_eq!(rotated.decrypt(&blob).unwrap().as_str(), "123456");
        assert_eq!(
            rotated
                .decrypt(&blob.replace("$7$", "$ +7junk$"))
                .unwrap()
                .as_str(),
            "123456"
        );
        assert!(rotated.decrypt(&raw).is_err());
        let retired = OtpEncryptionKeys::versioned(8, vec![(8, "z".repeat(32))], None).unwrap();
        assert!(retired.decrypt(&blob).is_err());
        for bad in [
            String::new(),
            "xyz".into(),
            blob.replace("$7$", "$-1$"),
            blob.replace("$7$", "$4294967296$"),
            format!(
                "{}{}",
                &blob[..blob.len() - 1],
                if blob.ends_with('0') { '1' } else { '0' }
            ),
        ] {
            assert!(rotated.decrypt(&bad).is_err());
        }
        let a = old.encrypt("123456").unwrap();
        let b = old.encrypt("123456").unwrap();
        assert_ne!(*a, *b);
    }
}
