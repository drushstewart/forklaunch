//! Trusted explicit verification delivery; no session or email-change authority.
use crate::background::DeliveryFuture;
#[cfg(any(feature = "http", test))]
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
#[cfg(any(feature = "http", test))]
use chrono::{DateTime, Utc};
#[cfg(any(feature = "http", test))]
use forklaunch_blueprint_core::BlueprintError;
use forklaunch_blueprint_core::BlueprintResult;
#[cfg(any(feature = "http", test))]
use hmac::{Hmac, Mac};
use serde_json::Value;
#[cfg(any(feature = "http", test))]
use serde_json::json;
#[cfg(any(feature = "http", test))]
use sha2::Sha256;
use zeroize::Zeroizing;

/// Sensitive owned callback input, deliberately without Debug/Serde.
pub struct EmailVerificationDeliveryContext {
    user: Value,
    token: Zeroizing<String>,
    url: Zeroizing<String>,
}
impl EmailVerificationDeliveryContext {
    #[must_use]
    pub fn user(&self) -> &Value {
        &self.user
    }
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}
/// Trusted explicit-send callback. Its future is always awaited, independently
/// of application background scheduling. Blocking code/panics are not intercepted.
pub trait EmailVerificationDelivery: Send + Sync {
    /// # Errors
    /// Preparation and future errors become sanitized empty HTTP500 responses.
    fn prepare(&self, context: EmailVerificationDeliveryContext)
    -> BlueprintResult<DeliveryFuture>;
}
#[cfg(any(feature = "http", test))]
pub(crate) fn failure() -> BlueprintError {
    BlueprintError::new(
        "EMAIL_VERIFICATION_DELIVERY_FAILED",
        "Unable to deliver verification email",
    )
}
#[cfg(any(feature = "http", test))]
pub(crate) fn token(
    email: &str,
    secret: &[u8],
    now: DateTime<Utc>,
) -> BlueprintResult<Zeroizing<String>> {
    let issued = now.timestamp();
    let expires = issued.checked_add(3600).ok_or_else(failure)?;
    let payload = Zeroizing::new(
        serde_json::to_vec(&json!({"email":email.to_lowercase(),"iat":issued,"exp":expires}))
            .map_err(|_| failure())?,
    );
    let mut output = Zeroizing::new(format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256"}"#),
        URL_SAFE_NO_PAD.encode(payload.as_slice())
    ));
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).map_err(|_| failure())?;
    mac.update(output.as_bytes());
    output.push('.');
    output.push_str(&URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()));
    Ok(output)
}
#[cfg(feature = "http")]
pub(crate) async fn deliver(
    hook: &dyn EmailVerificationDelivery,
    user: &Value,
    base: &str,
    callback: &str,
    secret: &[u8],
    now: DateTime<Utc>,
) -> BlueprintResult<()> {
    let public = super::reset_request::projection(user)?;
    let token = token(super::text(user, "email")?, secret, now)?;
    let url = Zeroizing::new(format!(
        "{base}/verify-email?token={}&callbackURL={}",
        token.as_str(),
        super::reset_request::component(if callback.is_empty() { "/" } else { callback })
    ));
    hook.prepare(EmailVerificationDeliveryContext {
        user: public,
        token,
        url,
    })
    .map_err(|_| failure())?
    .await
    .map_err(|_| failure())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn email_delivery_token_exact_expiry_and_no_change_authority() {
        let now = DateTime::from_timestamp(1_700_000_000, 999_000_000).unwrap();
        let proof = token("USER@EXAMPLE.TEST", b"owned-secret", now).unwrap();
        let claims = crate::credentials::verify_email_token(&proof, b"owned-secret", now).unwrap();
        assert_eq!(claims.email, "user@example.test");
        assert!(claims.update_to.is_none() && claims.request_type.is_none());
        assert_eq!(
            crate::credentials::verify_email_token(
                &proof,
                b"owned-secret",
                DateTime::from_timestamp(now.timestamp() + 3600, 0).unwrap()
            )
            .unwrap_err(),
            crate::credentials::EmailTokenError::TokenExpired
        );
        assert!(crate::credentials::verify_email_token(&proof, b"wrong-secret", now).is_err());
    }
}
