//! Bounded generation policy; existing proofs are independent of current policy.
use super::{BlueprintResult, TwoFactorService, crypto, error};
use std::sync::Arc;

pub(super) const MAX_CODES: usize = 100;
pub(super) const MAX_CODE_BYTES: usize = 128;

/// Number and random-character length of newly generated backup codes. A dash is
/// added after at most five characters; defaults are ten codes of ten characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackupCodeOptions {
    amount: u8,
    length: u8,
}
impl Default for BackupCodeOptions {
    fn default() -> Self {
        Self {
            amount: 10,
            length: 10,
        }
    }
}
impl BackupCodeOptions {
    /// # Errors
    /// Rejects amounts over 100 and lengths outside 1..=127. Zero amount is
    /// supported. JavaScript fractional/negative numbers are outside this subset.
    pub fn new(amount: u8, length: u8) -> BlueprintResult<Self> {
        if usize::from(amount) > MAX_CODES || !(1..=127).contains(&length) {
            return Err(error(
                "better_auth_invalid_config",
                "Invalid backup code options",
            ));
        }
        Ok(Self { amount, length })
    }
    #[must_use]
    pub fn amount(self) -> u8 {
        self.amount
    }
    #[must_use]
    pub fn length(self) -> u8 {
        self.length
    }
}

/// Trusted synchronous generation, matching the upstream `() => string[]` hook.
/// No identity/session/secret context is supplied. The returned strings are
/// authentication proofs: duplicates and empty strings intentionally remain valid.
/// Output is bounded to 100 strings, each at most 128 UTF-8 bytes. The callback is
/// invoked inside the authorized mutation transaction and must not reenter its
/// store. Return `Err` for fallible work; Rust panics are not JavaScript exceptions.
/// The service neither logs callback errors nor exposes their messages in responses.
pub trait BackupCodeGenerator: Send + Sync {
    /// Produces replacement authentication proofs.
    /// # Errors
    /// Returns a provider failure; the service discards its private details.
    fn generate(&self) -> BlueprintResult<Vec<String>>;
}
impl<F> BackupCodeGenerator for F
where
    F: Fn() -> BlueprintResult<Vec<String>> + Send + Sync,
{
    fn generate(&self) -> BlueprintResult<Vec<String>> {
        self()
    }
}
impl<S> TwoFactorService<S> {
    /// Changes future generation only; existing stored codes retain their authority.
    #[must_use]
    pub fn with_backup_code_options(mut self, options: BackupCodeOptions) -> Self {
        self.backup_code_options = options;
        self
    }
    /// Installs a trusted synchronous generator overriding amount/length options.
    #[must_use]
    pub fn with_backup_code_generator(mut self, generator: Arc<dyn BackupCodeGenerator>) -> Self {
        self.backup_code_generator = Some(generator);
        self
    }
    pub(super) fn generate_backup_codes(&self) -> BlueprintResult<Vec<String>> {
        let codes = if let Some(generator) = &self.backup_code_generator {
            generator.generate().map_err(|_| generation_failure())?
        } else {
            (0..self.backup_code_options.amount)
                .map(|_| {
                    let code = crypto::opaque(usize::from(self.backup_code_options.length))?;
                    let split = 5.min(code.len());
                    Ok(format!("{}-{}", &code[..split], &code[split..]))
                })
                .collect::<BlueprintResult<Vec<_>>>()?
        };
        if codes.len() > MAX_CODES || codes.iter().any(|code| code.len() > MAX_CODE_BYTES) {
            return Err(generation_failure());
        }
        Ok(codes)
    }
}
fn generation_failure() -> super::BlueprintError {
    error(
        "TWO_FACTOR_GENERATION_FAILED",
        "Unable to generate backup codes",
    )
}
