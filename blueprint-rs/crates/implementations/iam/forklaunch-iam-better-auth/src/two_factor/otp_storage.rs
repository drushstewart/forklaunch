//! OTP field representation, independent of encryption of the entire stored row.
use super::TwoFactorService;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use zeroize::Zeroizing;

/// Trusted OTP comparison policy. Default plain codes still reside inside the
/// encrypted tenant record. Hashed uses SHA-256 of UTF-8 and unpadded base64url.
/// Changing policy affects verification of existing proofs immediately; restoring
/// the original policy permits remaining attempts without rewriting the proof.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OtpStorage {
    #[default]
    Plain,
    Hashed,
    /// Better Auth XChaCha20-Poly1305 field encoding using trusted encryption keys.
    Encrypted,
}
impl OtpStorage {
    pub(super) fn comparison(self, code: &str) -> Zeroizing<String> {
        Zeroizing::new(match self {
            Self::Plain | Self::Encrypted => code.to_owned(),
            Self::Hashed => URL_SAFE_NO_PAD
                .encode(ring::digest::digest(&ring::digest::SHA256, code.as_bytes()).as_ref()),
        })
    }
}
/// Validate either supported persisted representation, independently of current
/// policy. A policy switch is an ordinary failed proof, not corrupt storage.
pub(super) fn valid_stored(value: &str) -> bool {
    super::otp_encryption::recognizable(value)
        || ((1..=32).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_digit()))
        || (value.len() == 43
            && URL_SAFE_NO_PAD
                .decode(value)
                .is_ok_and(|bytes| bytes.len() == 32 && URL_SAFE_NO_PAD.encode(bytes) == value))
}
impl<S> TwoFactorService<S> {
    /// Selects field encoding for new OTPs and comparison of existing OTPs.
    /// Does not change delivery, digit/attempt limits, expiry, or row encryption.
    #[must_use]
    pub fn with_otp_storage(mut self, storage: OtpStorage) -> Self {
        self.otp_codec = None;
        self.otp_storage = storage;
        self
    }
}
