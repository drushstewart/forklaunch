//! Trusted asynchronous backup-field callbacks, distinct from OTP consume semantics.
use super::{BlueprintResult, TwoFactorService, error};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use zeroize::Zeroizing;

/// Owned, zeroized callback data. No Debug/Serialize. Ciphertext is bounded to
/// 154330 UTF-8 bytes; decoded JSON is checked separately at 77101 bytes.
pub struct BackupCodeCodecText(Zeroizing<String>);
impl BackupCodeCodecText {
    /// # Errors
    /// Rejects strings above 154330 UTF-8 bytes. Empty/Unicode/colon strings are admitted.
    pub fn new(value: String) -> BlueprintResult<Self> {
        let value = Zeroizing::new(value);
        if value.len() > 154_330 {
            return Err(failure());
        }
        Ok(Self(value))
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// A trusted owned future. No transaction is held while this future is polled.
pub type BackupCodeCodecFuture =
    Pin<Box<dyn Future<Output = BlueprintResult<BackupCodeCodecText>> + Send + 'static>>;
/// Paired asynchronous field encryption. Inputs contain only backup-code data,
/// never user/session/token context. Return errors for fallible work; panics and
/// blocking callback construction are not interrupted or converted to JS errors.
pub trait BackupCodeEncryptionCodec: Send + Sync {
    fn encrypt(&self, json: BackupCodeCodecText) -> BackupCodeCodecFuture;
    fn decrypt(&self, encoded: BackupCodeCodecText) -> BackupCodeCodecFuture;
}
/// Total operation lease across all callbacks and persistence phases. Timeout is
/// cooperative; it cannot interrupt blocking/non-yielding trusted code or commit.
#[derive(Clone, Copy)]
pub struct BackupCodeCodecOptions {
    timeout: Duration,
}
impl Default for BackupCodeCodecOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
        }
    }
}
impl BackupCodeCodecOptions {
    /// # Errors
    /// Rejects non-whole-millisecond durations or values outside 1ms..=30s.
    pub fn new(timeout: Duration) -> BlueprintResult<Self> {
        if timeout.is_zero()
            || timeout > Duration::from_secs(30)
            || !timeout.as_nanos().is_multiple_of(1_000_000)
        {
            return Err(error(
                "better_auth_invalid_config",
                "Invalid backup codec lease",
            ));
        }
        Ok(Self { timeout })
    }
    #[must_use]
    pub fn timeout(self) -> Duration {
        self.timeout
    }
}
pub(super) struct Codec {
    pub hook: Arc<dyn BackupCodeEncryptionCodec>,
    pub options: BackupCodeCodecOptions,
}
pub(super) fn failure() -> super::BlueprintError {
    error(
        "TWO_FACTOR_BACKUP_CODEC_FAILED",
        "Unable to process backup codes",
    )
}
impl<S> TwoFactorService<S> {
    /// Installs trusted asynchronous backup encryption. Subsequent built-in storage
    /// selection removes this callback. Existing format2 records then fail closed.
    #[must_use]
    pub fn with_backup_code_encryption_codec(
        mut self,
        hook: Arc<dyn BackupCodeEncryptionCodec>,
        options: BackupCodeCodecOptions,
    ) -> Self {
        self.backup_codec = Some(Arc::new(Codec { hook, options }));
        self
    }
    #[cfg(feature = "http")]
    pub(crate) fn has_custom_backup_codec(&self) -> bool {
        self.backup_codec.is_some()
    }
}
