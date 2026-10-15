//! Trusted asynchronous OTP field codecs. This is independent of store encryption.
use super::{BlueprintResult, error};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use zeroize::Zeroizing;

/// An owned, bounded secret for a trusted codec. Never log its contents.
/// Deliberately implements neither Debug nor Serialize.
pub struct OtpCodecText(Zeroizing<String>);
impl OtpCodecText {
    /// # Errors
    /// Rejects values exceeding 1024 UTF-8 bytes. Empty and Unicode values are valid.
    pub fn new(value: String) -> BlueprintResult<Self> {
        let value = Zeroizing::new(value);
        if value.len() > 1024 {
            return Err(failure());
        }
        Ok(Self(value))
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// No database lock remains held while this owned future is polled.
pub type OtpCodecFuture =
    Pin<Box<dyn Future<Output = BlueprintResult<OtpCodecText>> + Send + 'static>>;
/// Trusted custom hashing. The same operation encodes generated and supplied codes.
pub trait OtpHashCodec: Send + Sync {
    fn hash(&self, value: OtpCodecText) -> OtpCodecFuture;
}
/// Trusted custom encryption. No session, token or user context is provided.
pub trait OtpEncryptionCodec: Send + Sync {
    fn encrypt(&self, value: OtpCodecText) -> OtpCodecFuture;
    fn decrypt(&self, value: OtpCodecText) -> OtpCodecFuture;
}
/// Bounded cooperative callback wait and result-acceptance lease.
/// Does not interrupt blocking/non-yielding callbacks or synchronous construction.
/// A durable lease rejects stale completion after restart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OtpCodecOptions {
    timeout_ms: u32,
}
impl OtpCodecOptions {
    /// # Errors
    /// Requires a whole-millisecond timeout in 1ms..=30 seconds.
    pub fn new(timeout: Duration) -> BlueprintResult<Self> {
        if !(1..=30_000).contains(&timeout.as_millis())
            || !timeout.subsec_nanos().is_multiple_of(1_000_000)
        {
            return Err(error(
                "better_auth_invalid_config",
                "Invalid OTP codec timeout",
            ));
        }
        Ok(Self {
            timeout_ms: u32::try_from(timeout.as_millis()).map_err(|_| failure())?,
        })
    }
    #[must_use]
    pub fn timeout(self) -> Duration {
        Duration::from_millis(u64::from(self.timeout_ms))
    }
}
impl Default for OtpCodecOptions {
    fn default() -> Self {
        Self { timeout_ms: 30_000 }
    }
}
pub(super) enum CodecKind {
    Hash(Arc<dyn OtpHashCodec>),
    Encryption(Arc<dyn OtpEncryptionCodec>),
}
pub(super) struct Codec {
    pub kind: CodecKind,
    pub options: OtpCodecOptions,
}
impl Codec {
    pub async fn encode(&self, value: &str) -> BlueprintResult<OtpCodecText> {
        let input = OtpCodecText::new(value.to_owned())?;
        let future = match &self.kind {
            CodecKind::Hash(codec) => codec.hash(input),
            CodecKind::Encryption(codec) => codec.encrypt(input),
        };
        let result = tokio::time::timeout(self.options.timeout(), future)
            .await
            .map_err(|_| failure())?
            .map_err(|_| failure())?;
        // Upstream stores `${encoded}:${attempts}`; this typed subset excludes delimiter ambiguity.
        if result.as_str().contains(':') {
            return Err(failure());
        }
        Ok(result)
    }
    pub async fn compare(&self, stored: &str, supplied: &str) -> BlueprintResult<bool> {
        let (future, expected, encoded) = match &self.kind {
            CodecKind::Hash(codec) => (
                codec.hash(OtpCodecText::new(supplied.to_owned())?),
                stored,
                true,
            ),
            CodecKind::Encryption(codec) => (
                codec.decrypt(OtpCodecText::new(stored.to_owned())?),
                supplied,
                false,
            ),
        };
        let result = tokio::time::timeout(self.options.timeout(), future)
            .await
            .map_err(|_| failure())?
            .map_err(|_| failure())?;
        if encoded && result.as_str().contains(':') {
            return Err(failure());
        }
        let difference = result
            .as_str()
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |value, (a, b)| value | (a ^ b));
        Ok(result.as_str().len() == expected.len() && difference == 0)
    }
}
pub(super) fn failure() -> super::BlueprintError {
    error("TWO_FACTOR_CUSTOM_CODEC_FAILED", "Unable to process OTP")
}
