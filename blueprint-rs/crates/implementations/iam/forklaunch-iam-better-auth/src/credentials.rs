//! Better Auth 1.7.6 credential encoding and Unicode normalization.
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use rand_core::{OsRng, RngCore};
use scrypt::{Params, scrypt};
use subtle::ConstantTimeEq;
use unicode_normalization::UnicodeNormalization;

fn derive(password: &str, salt: &str) -> BlueprintResult<String> {
    // Better Auth passes the hex salt as UTF-8 text, not decoded salt bytes.
    let normalized: String = password.nfkc().collect();
    let params = Params::new(14, 16, 1, 64).map_err(|_| credential_error())?;
    let mut key = [0_u8; 64];
    scrypt(normalized.as_bytes(), salt.as_bytes(), &params, &mut key)
        .map_err(|_| credential_error())?;
    Ok(hex::encode(key))
}

/// Produces the configured `salt:key` scrypt hash with a random 16-byte salt.
/// Run this CPU-bound operation on a blocking executor from async handlers.
///
/// # Errors
/// Returns a safe error if secure randomness or key derivation fails.
pub fn hash_password(password: &str) -> BlueprintResult<String> {
    let mut salt = [0_u8; 16];
    OsRng
        .try_fill_bytes(&mut salt)
        .map_err(|_| credential_error())?;
    let salt = hex::encode(salt);
    Ok(format!("{salt}:{}", derive(password, &salt)?))
}

/// Verifies the actual Better Auth encoding in constant time.
/// Malformed hashes fail closed. As in the upstream verifier, the salt is text
/// and only the first two colon-delimited fields participate in verification.
/// Run this CPU-bound operation on a blocking executor from async handlers.
#[must_use]
pub fn verify_password(password: &str, encoded_hash: &str) -> bool {
    let mut parts = encoded_hash.split(':');
    let (Some(salt), Some(key)) = (parts.next(), parts.next()) else {
        return false;
    };
    if salt.is_empty() || key.len() != 128 {
        return false;
    }
    derive(password, salt).is_ok_and(|derived| bool::from(derived.as_bytes().ct_eq(key.as_bytes())))
}

fn credential_error() -> BlueprintError {
    BlueprintError::new("password_hash_failed", "Unable to securely hash password")
}

mod email_token;
pub use email_token::{EmailTokenError, VerifiedEmailToken, verify_email_token};
