//! Shared password storage primitives for IAM implementations.
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use forklaunch_core::domain::BlueprintError;
use forklaunch_core::domain::BlueprintResult;
use rand_core::{OsRng, RngCore};

/// Produces a PHC-encoded Argon2id hash using independent random salts.
///
/// # Errors
/// Returns a safe error on random source or hash failure.
pub fn hash_password(password: &str) -> BlueprintResult<String> {
    let mut salt = [0_u8; 16];
    OsRng
        .try_fill_bytes(&mut salt)
        .map_err(|_| credential_error())?;
    let salt = SaltString::encode_b64(&salt).map_err(|_| credential_error())?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| credential_error())
}

/// Verifies a stored PHC hash using its encoded parameters. Malformed hashes fail closed.
#[must_use]
pub fn verify_password(password: &str, encoded_hash: &str) -> bool {
    PasswordHash::new(encoded_hash).is_ok_and(|hash| {
        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok()
    })
}

fn credential_error() -> BlueprintError {
    BlueprintError::new("password_hash_failed", "Unable to securely hash password")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn salted_hashes_are_distinct_and_reject_wrong_passwords_and_bad_encodings() {
        let first = hash_password("correct-password").unwrap();
        let second = hash_password("correct-password").unwrap();
        assert!(first.starts_with("$argon2id$"));
        assert_ne!(first, second);
        assert!(!first.contains("correct-password"));
        assert!(verify_password("correct-password", &first));
        assert!(!verify_password("wrong", &first));
        assert!(!verify_password("correct-password", "correct-password"));
        assert!(!verify_password("correct-password", "$argon2id$malformed"));
    }
}
