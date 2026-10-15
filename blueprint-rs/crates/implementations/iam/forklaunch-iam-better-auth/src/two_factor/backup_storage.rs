//! Backup field codecs are separate from mandatory tenant-record encryption.
use super::{
    BlueprintResult, TwoFactorService, Value, error, json, otp_encryption::OtpEncryptionKeys,
};
use std::sync::Arc;
use zeroize::Zeroizing;

// 100 strings of 128 UTF-8 bytes, each byte escaped as six JSON bytes, plus
// two quotes/string, commas, and brackets. Ciphertext adds a 24-byte nonce/16-byte tag.
pub(super) const MAX_JSON: usize = 77_101;
const MAX_HEX: usize = 154_282;
const MAX_STORED: usize = MAX_HEX + 48;
const FORMAT: &str = "backupCodesFormat";

/// Trusted field-storage policy. Direct services preserve historical arrays;
/// the first HTTP two-factor opt-in selects encrypted JSON. Legacy unmarked arrays
/// remain readable under every policy; only successful consumption/regeneration
/// migrates them. Current policy decodes marked strings, without mode autodetection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackupCodeStorage {
    #[default]
    LegacyArray,
    PlainJson,
    Encrypted,
}

/// Independent backup field keys. No Debug/Serialize or secret accessors. Derived
/// keys are zeroized. These do not rotate OTP, cookie, or tenant-record keys.
pub struct BackupCodeEncryptionKeys(OtpEncryptionKeys);
impl BackupCodeEncryptionKeys {
    /// Uses nonce-prefixed hexadecimal ciphertext without a version envelope.
    /// # Errors
    /// Rejects secrets outside 32..=4096 UTF-8 bytes.
    pub fn legacy(secret: String) -> BlueprintResult<Self> {
        OtpEncryptionKeys::legacy(secret)
            .map(Self)
            .map_err(|_| invalid_config())
    }
    /// Uses the current u32 version and retains at most 16 read versions plus an
    /// optional legacy key. Writes canonical `$ba$version$`; permissive upstream
    /// read prefixes are bounded to 48 bytes. Old versions are never rewritten on read.
    /// # Errors
    /// Rejects invalid key lengths, duplicate versions, missing current version,
    /// empty key sets, or more than 16 keys.
    pub fn versioned(
        current: u32,
        keys: Vec<(u32, String)>,
        legacy: Option<String>,
    ) -> BlueprintResult<Self> {
        OtpEncryptionKeys::versioned(current, keys, legacy)
            .map(Self)
            .map_err(|_| invalid_config())
    }
    fn encrypt(&self, plain: &str) -> BlueprintResult<Zeroizing<String>> {
        self.encrypt_nonce(plain, &super::crypto::random(24)?)
    }
    pub(super) fn encrypt_nonce(
        &self,
        plain: &str,
        nonce: &[u8],
    ) -> BlueprintResult<Zeroizing<String>> {
        self.0
            .encrypt_nonce_bounded(plain, nonce, MAX_JSON)
            .map_err(|_| failure())
    }
    pub(super) fn decrypt(&self, stored: &str) -> BlueprintResult<Zeroizing<String>> {
        self.0
            .decrypt_bounded(stored, MAX_STORED, MAX_HEX, Some(48))
            .map_err(|_| failure())
    }
}
fn invalid_config() -> super::BlueprintError {
    error(
        "better_auth_invalid_config",
        "Invalid backup code encryption keys",
    )
}
fn failure() -> super::BlueprintError {
    error(
        "TWO_FACTOR_BACKUP_STORAGE_FAILED",
        "Unable to decode backup codes",
    )
}
fn codes(value: &Value) -> BlueprintResult<Vec<String>> {
    let array = value.as_array().ok_or_else(failure)?;
    if array.len() > super::backup_options::MAX_CODES {
        return Err(failure());
    }
    array
        .iter()
        .map(|code| {
            let code = code
                .as_str()
                .filter(|code| code.len() <= super::backup_options::MAX_CODE_BYTES)
                .ok_or_else(failure)?;
            Ok(code.to_owned())
        })
        .collect()
}
impl<S> TwoFactorService<S> {
    /// Selects the current codec for marked strings and future writes. Existing
    /// arrays are preserved until an authorized successful mutation. `PlainJson`
    /// still requires an encrypted durable tenant store.
    #[must_use]
    pub fn with_backup_code_storage(mut self, storage: BackupCodeStorage) -> Self {
        self.backup_code_storage = storage;
        self
    }
    /// Installs independent keys and selects encrypted backup storage.
    #[must_use]
    pub fn with_backup_code_encryption_keys(mut self, keys: Arc<BackupCodeEncryptionKeys>) -> Self {
        self.backup_code_encryption_keys = Some(keys);
        self.backup_code_storage = BackupCodeStorage::Encrypted;
        self
    }
    pub(super) fn read_backup_codes(&self, row: &Value) -> BlueprintResult<Option<Vec<String>>> {
        if row.get(FORMAT).is_none() && row["backupCodes"].is_array() {
            return codes(&row["backupCodes"]).map(Some);
        }
        if row[FORMAT] != json!(1) {
            return Err(failure());
        }
        let stored = row["backupCodes"]
            .as_str()
            .filter(|value| value.len() <= MAX_STORED)
            .ok_or_else(failure)?;
        let decoded = if self.backup_code_storage == BackupCodeStorage::Encrypted {
            let fallback = BackupCodeEncryptionKeys(OtpEncryptionKeys::from_bytes(&self.secret));
            self.backup_code_encryption_keys
                .as_deref()
                .unwrap_or(&fallback)
                .decrypt(stored)?
        } else {
            Zeroizing::new(stored.to_owned())
        };
        // Upstream safeJSONParse turns malformed JSON/null into an invalid proof,
        // whereas decrypt failures throw before any proof is consumed.
        if self.backup_code_storage == BackupCodeStorage::Encrypted && decoded.len() > MAX_JSON {
            return Err(failure());
        }
        match serde_json::from_str::<Value>(&decoded) {
            Err(_) | Ok(Value::Null) => Ok(None),
            Ok(value) => codes(&value).map(Some),
        }
    }
    pub(super) fn write_backup_codes(
        &self,
        row: &mut Value,
        codes: &[String],
    ) -> BlueprintResult<()> {
        if codes.len() > super::backup_options::MAX_CODES
            || codes
                .iter()
                .any(|c| c.len() > super::backup_options::MAX_CODE_BYTES)
        {
            return Err(failure());
        }
        if self.backup_code_storage == BackupCodeStorage::LegacyArray {
            row["backupCodes"] = json!(codes);
            row.as_object_mut().ok_or_else(failure)?.remove(FORMAT);
        } else {
            let plain = Zeroizing::new(serde_json::to_string(codes).map_err(|_| failure())?);
            if plain.len() > MAX_JSON {
                return Err(failure());
            }
            let value = if self.backup_code_storage == BackupCodeStorage::Encrypted {
                let fallback =
                    BackupCodeEncryptionKeys(OtpEncryptionKeys::from_bytes(&self.secret));
                self.backup_code_encryption_keys
                    .as_deref()
                    .unwrap_or(&fallback)
                    .encrypt(&plain)?
            } else {
                plain
            };
            row["backupCodes"] = json!(value.as_str());
            row[FORMAT] = json!(1);
        }
        Ok(())
    }
}
