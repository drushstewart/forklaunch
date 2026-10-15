//! Bounded delivered OTP options with trusted default-await delivery.
use super::{
    AuthStore, AuthTransaction, BlueprintResult, DateTime, FactorResponse, Model, TwoFactorService,
    Utc, Value, authenticated, corrupt, crypto, empty_response, enabled, enrollment, error,
    invalid_challenge, json, output_user, text, time, verification_row,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use zeroize::Zeroizing;

/// Trusted delivered-code policy. Defaults are six digits, 180 seconds and five
/// failed attempts. These options are never accepted from an HTTP request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OtpOptions {
    digits: u8,
    validity_ms: i64,
    allowed_attempts: u8,
}
impl Default for OtpOptions {
    fn default() -> Self {
        Self {
            digits: 6,
            validity_ms: 180_000,
            allowed_attempts: 5,
        }
    }
}
impl OtpOptions {
    /// Validity is a duration, unlike upstream's numeric minutes. Expiry is
    /// persisted at send time; the current attempt policy is read at verification.
    /// # Errors
    /// Rejects digits outside 1..=32, attempts outside 1..=100, zero validity,
    /// sub-millisecond precision, or durations exceeding signed milliseconds.
    pub fn new(
        digits: u8,
        validity: std::time::Duration,
        allowed_attempts: u8,
    ) -> BlueprintResult<Self> {
        let invalid = || error("better_auth_invalid_config", "Invalid OTP options");
        let validity_ms = i64::try_from(validity.as_millis()).map_err(|_| invalid())?;
        if !(1..=32).contains(&digits)
            || !(1..=100).contains(&allowed_attempts)
            || validity_ms == 0
            || !validity.subsec_nanos().is_multiple_of(1_000_000)
        {
            return Err(invalid());
        }
        Ok(Self {
            digits,
            validity_ms,
            allowed_attempts,
        })
    }
    #[must_use]
    pub fn digits(self) -> u8 {
        self.digits
    }
    #[must_use]
    pub fn validity(self) -> std::time::Duration {
        std::time::Duration::from_millis(self.validity_ms.unsigned_abs())
    }
    #[must_use]
    pub fn allowed_attempts(self) -> u8 {
        self.allowed_attempts
    }
}

/// An owned delivery operation. Default dispatch awaits this future after the
/// encrypted OTP transaction has committed; no storage lock survives dispatch.
pub type OtpDeliveryFuture = crate::background::DeliveryFuture;

/// Trusted delivery input. The code is an authentication secret; neither this
/// context nor its secret implements Debug or Serialize. No session is included.
pub struct OtpDeliveryContext {
    user: Value,
    code: Zeroizing<String>,
}
impl OtpDeliveryContext {
    /// Public user projection for choosing an administrator-configured channel.
    #[must_use]
    pub fn user(&self) -> &Value {
        &self.user
    }
    /// Short-lived authentication secret. Never log or expose it to the caller.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }
}

/// Trusted server delivery implementation, never selected by HTTP input.
/// Preparation errors model synchronous callback failure. A future that returns
/// an error models asynchronous delivery failure: the send response stays successful
/// and the already committed OTP remains valid until its original expiry.
pub trait OtpDelivery: Send + Sync {
    /// # Errors
    /// Return an error if the delivery operation cannot be constructed. Error
    /// contents are discarded at the public boundary to avoid credential leaks.
    fn prepare(&self, context: OtpDeliveryContext) -> BlueprintResult<OtpDeliveryFuture>;
}

pub(super) struct Authority {
    pub user: Value,
    pub session: Option<Value>,
    pub challenge: Option<Value>,
    pub identifier: String,
}
pub(super) async fn authority<T: AuthTransaction>(
    tx: &mut T,
    token: &str,
    challenge: &str,
    now: DateTime<Utc>,
) -> BlueprintResult<Authority> {
    if !token.is_empty() {
        match authenticated(tx, token, now).await {
            Ok((user, session)) => {
                let identifier = session_identifier(text(&user, "id")?, text(&session, "id")?);
                return Ok(Authority {
                    user,
                    session: Some(session),
                    challenge: None,
                    identifier,
                });
            }
            Err(error) if error.code == "UNAUTHORIZED" => {}
            Err(error) => return Err(error),
        }
    }
    if !challenge.starts_with("2fa-") {
        return Err(invalid_challenge());
    }
    let row = verification_row(tx, challenge)
        .await?
        .ok_or_else(invalid_challenge)?;
    if time(&row, "expiresAt")? < now {
        return Err(invalid_challenge());
    }
    let user = tx
        .get(Model::User, text(&row, "value")?)
        .await?
        .ok_or_else(invalid_challenge)?;
    if !enabled(&user)? {
        return Err(invalid_challenge());
    }
    Ok(Authority {
        user,
        session: None,
        challenge: Some(row),
        identifier: identifier(&format!("challenge:{challenge}")),
    })
}
fn session_identifier(user: &str, session: &str) -> String {
    identifier(&format!("session:{}", json!([user, session])))
}
fn identifier(key: &str) -> String {
    // Fixed-length internal purpose separation prevents opaque application IDs
    // from overflowing the existing verification identifier lookup budget.
    let hash = ring::digest::digest(&ring::digest::SHA256, key.as_bytes());
    format!("2fa-otp-{}", URL_SAFE_NO_PAD.encode(hash.as_ref()))
}
pub(super) fn code(digits: u8) -> BlueprintResult<Zeroizing<String>> {
    let digits = usize::from(digits);
    let mut value = Zeroizing::new(String::with_capacity(digits));
    while value.len() < digits {
        for byte in crypto::random(16)?
            .iter()
            .copied()
            .filter(|byte| *byte < 250)
        {
            value.push(char::from(b'0' + byte % 10));
            if value.len() == digits {
                break;
            }
        }
    }
    Ok(value)
}
impl<S: AuthStore> TwoFactorService<S> {
    /// Enables delivered OTP using a trusted server-owned hook.
    /// Explicit Result failures are supported; Rust panics are not callback errors.
    #[must_use]
    pub fn with_otp_delivery(mut self, delivery: std::sync::Arc<dyn OtpDelivery>) -> Self {
        self.otp_delivery = Some(delivery);
        self
    }
    /// Sets trusted OTP policy without changing delivery. Existing codes retain
    /// their stored expiry; verification uses the newly selected attempt limit.
    #[must_use]
    pub fn with_otp_options(mut self, options: OtpOptions) -> Self {
        self.otp_options = options;
        self
    }
    #[must_use]
    pub fn has_otp_delivery(&self) -> bool {
        self.otp_delivery.is_some()
    }

    /// Persists a new configured OTP, replacing an older code for the same
    /// authenticated session or signed challenge, then awaits trusted delivery.
    /// Canceling the caller after commit does not roll back the expiring code;
    /// delivery completion never issues an authenticated session.
    /// # Errors
    /// Rejects absent authority, storage failures and synchronous preparation
    /// failures. Asynchronous delivery errors are deliberately handled.
    pub async fn send_otp(
        &self,
        token: &str,
        challenge: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<FactorResponse> {
        if self.otp_codec.is_some() {
            let started = std::time::Instant::now();
            let clock = move || {
                now.checked_add_signed(
                    chrono::Duration::from_std(started.elapsed()).unwrap_or(chrono::Duration::MAX),
                )
                .unwrap_or(DateTime::<Utc>::MAX_UTC)
            };
            return self.send_custom_otp(token, challenge, &clock).await;
        }
        self.otp_delivery
            .as_ref()
            .ok_or_else(|| error("OTP_NOT_CONFIGURED", "otp isn't configured"))?;
        let expires = now
            .checked_add_signed(chrono::Duration::milliseconds(self.otp_options.validity_ms))
            .map(super::timestamp)
            .ok_or_else(|| error("better_auth_invalid_config", "OTP expiry is out of range"))?;
        let mut tx = self.store.begin(true).await?;
        let bound = authority(&mut tx, token, challenge, now).await?;
        let code = code(self.otp_options.digits)?;
        let previous = verification_row(&mut tx, &bound.identifier).await?;
        let id = previous
            .as_ref()
            .map(|row| text(row, "id").map(str::to_owned))
            .transpose()?
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let stored = if self.otp_storage == super::OtpStorage::Encrypted {
            self.encrypt_otp(&code)?
        } else {
            self.otp_storage.comparison(&code)
        };
        let row = json!({"id":id,"identifier":bound.identifier,"value":stored.as_str(),"attempts":0,"expiresAt":expires});
        if previous.is_some() {
            tx.replace(Model::Verification, row).await?;
        } else {
            tx.insert(Model::Verification, row).await?;
        }
        tx.commit().await?;
        self.deliver_otp(output_user(&bound.user), code).await
    }
    pub(super) async fn deliver_otp(
        &self,
        user: Value,
        code: Zeroizing<String>,
    ) -> BlueprintResult<FactorResponse> {
        let delivery = self
            .otp_delivery
            .as_ref()
            .ok_or_else(|| error("OTP_NOT_CONFIGURED", "otp isn't configured"))?;
        let context = OtpDeliveryContext { user, code };
        let future = delivery.prepare(context).map_err(|_| {
            error(
                "TWO_FACTOR_DELIVERY_PREPARATION_FAILED",
                "Unable to prepare OTP delivery",
            )
        })?;
        crate::background::run(
            future,
            self.otp_background_dispatcher.as_deref(),
            "/two-factor/send-otp",
            "TWO_FACTOR_DELIVERY_FAILED",
            "TWO_FACTOR_DISPATCH_FAILED",
            "OTP delivery failed",
        )
        .await;
        Ok(empty_response(json!({"status":true})))
    }

    /// Enables delivered OTP after validating the current credential password,
    /// rotates the authenticated session, and leaves TOTP/recovery records alone.
    /// # Errors
    /// Invalid passwords or concurrent credential changes reject the transition;
    /// persistence failures roll back user enablement and session rotation.
    pub async fn enable_otp(
        &self,
        token: &str,
        password: String,
        now: DateTime<Utc>,
    ) -> BlueprintResult<FactorResponse> {
        let snapshot = self
            .password_snapshot(token, password, now, super::PasswordAction::General)
            .await?;
        if self.otp_delivery.is_none() {
            return Err(error("OTP_NOT_CONFIGURED", "OTP is not available"));
        }
        let mut tx = self.store.begin(true).await?;
        let (mut user, session) = authenticated(&mut tx, token, now).await?;
        snapshot.check(&mut tx, &user).await?;
        user["twoFactorEnabled"] = json!(true);
        user["updatedAt"] = json!(super::timestamp(now));
        tx.replace(Model::User, user.clone()).await?;
        let replacement = enrollment::rotate(&mut tx, &user, &session, now).await?;
        tx.commit().await?;
        let mut response = empty_response(json!({"method":"otp"}));
        response.session = Some(replacement);
        Ok(response)
    }
}

impl<S: AuthStore> TwoFactorService<S> {
    /// Consumes a delivered OTP under the tenant transaction lock. Invalid codes
    /// rearm the original expiry with an incremented counter. Verification uses
    /// the current configured attempt limit (default: five failed attempts). Success and session issuance are
    /// atomic. OTP-only users have no TOTP row or account-wide lock counter, as in
    /// the upstream default; existing TOTP rows share their account-wide counter.
    /// # Errors
    /// Invalid codes commit counters before returning. Other persistence failures
    /// roll back code consumption and session/user/trust changes together.
    #[allow(clippy::too_many_lines)]
    pub async fn verify_otp(
        &self,
        token: &str,
        challenge: &str,
        supplied: &str,
        trust_device: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<FactorResponse> {
        if self.otp_codec.is_some() {
            let started = std::time::Instant::now();
            let clock = move || {
                now.checked_add_signed(
                    chrono::Duration::from_std(started.elapsed()).unwrap_or(chrono::Duration::MAX),
                )
                .unwrap_or(DateTime::<Utc>::MAX_UTC)
            };
            return self
                .verify_custom_otp(token, challenge, supplied, trust_device, &clock)
                .await;
        }
        if supplied.len() > 128 {
            return Err(error("INVALID_CODE", "Invalid code"));
        }
        let mut tx = self.store.begin(true).await?;
        let bound = authority(&mut tx, token, challenge, now).await?;
        let mut factor = if bound.challenge.is_some() {
            tx.find_unique(Model::TwoFactor, "userId", text(&bound.user, "id")?)
                .await?
        } else {
            None
        };
        if let Some(factor) = factor.as_mut()
            && self.check_account_lock(factor, now)?
        {
            tx.replace(Model::TwoFactor, factor.clone()).await?;
        }
        let Some(mut otp) = verification_row(&mut tx, &bound.identifier).await? else {
            tx.commit().await?;
            return Err(error("OTP_HAS_EXPIRED", "OTP has expired"));
        };
        if super::otp_codec_state::Envelope::read(&otp)?.is_some() {
            return Err(super::otp_codec::failure());
        }
        tx.delete(Model::Verification, text(&otp, "id")?).await?;
        if time(&otp, "expiresAt")? < now {
            tx.commit().await?;
            return Err(error("OTP_HAS_EXPIRED", "OTP has expired"));
        }
        let attempts = otp["attempts"].as_u64().ok_or_else(corrupt)?;
        if attempts >= u64::from(self.otp_options.allowed_attempts) {
            tx.commit().await?;
            return Err(error(
                "TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE",
                "Too many attempts. Please request a new code.",
            ));
        }
        let expected = text(&otp, "value")?;
        let decoded;
        let expected = if self.otp_storage == super::OtpStorage::Encrypted {
            decoded = match self.decrypt_otp(expected) {
                Ok(value) => value,
                Err(error) => {
                    tx.commit().await?;
                    return Err(error);
                }
            };
            decoded.as_str()
        } else {
            expected
        };
        if self.otp_storage != super::OtpStorage::Encrypted
            && !super::otp_storage::valid_stored(expected)
        {
            return Err(corrupt());
        }
        let comparison = self.otp_storage.comparison(supplied);
        let supplied = comparison.as_str();
        let difference = expected
            .bytes()
            .zip(supplied.bytes())
            .fold(0u8, |value, (a, b)| value | (a ^ b));
        if supplied.len() != expected.len() || difference != 0 {
            otp["attempts"] = json!(attempts + 1);
            tx.insert(Model::Verification, otp).await?;
            if let Some(mut factor) = factor
                && self.record_account_failure(&mut factor, now)?
            {
                tx.replace(Model::TwoFactor, factor).await?;
            }
            tx.commit().await?;
            return Err(error("INVALID_CODE", "Invalid code"));
        }
        if let Some(mut factor) = factor
            && self.account_lockout_options.enabled()
        {
            self.reset_account_failures(&mut factor);
            tx.replace(Model::TwoFactor, factor).await?;
        }
        let response = self.complete_otp(&mut tx, bound, trust_device, now).await?;
        tx.commit().await?;
        Ok(response)
    }
    pub(super) async fn complete_otp<T: AuthTransaction>(
        &self,
        tx: &mut T,
        bound: Authority,
        trust_device: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<FactorResponse> {
        let mut user = bound.user;
        let mut response = empty_response(
            json!({"token":bound.session.as_ref().map(|session|session["token"].clone()),"user":output_user(&user)}),
        );
        if let Some(session) = bound.session {
            if !enabled(&user)? {
                user["twoFactorEnabled"] = json!(true);
                user["updatedAt"] = json!(super::timestamp(now));
                tx.replace(Model::User, user.clone()).await?;
                let replacement = enrollment::rotate(tx, &user, &session, now).await?;
                response.body = json!({"token":replacement["token"],"user":output_user(&user)});
                response.session = Some(replacement);
            }
        } else {
            let challenge = bound.challenge.ok_or_else(invalid_challenge)?;
            tx.delete(Model::Verification, text(&challenge, "id")?)
                .await?;
            let session = super::new_session(
                text(&user, "id")?,
                challenge["ipAddress"].as_str().unwrap_or(""),
                challenge["userAgent"].as_str().unwrap_or(""),
                now,
            )?;
            tx.insert(Model::Session, session.clone()).await?;
            response.body["token"] = session["token"].clone();
            response.session = Some(session);
            response.clear_challenge = true;
            response.persistent = challenge["persistent"].as_bool().unwrap_or(true);
            if trust_device {
                response.trust = Some(self.create_trust(tx, &user, now).await?);
            }
        }
        Ok(response)
    }
}

#[cfg(test)]
mod binding_tests {
    use super::*;
    #[test]
    fn opaque_session_components_are_unambiguously_framed() {
        assert_ne!(
            session_identifier("a!b", "c"),
            session_identifier("a", "b!c")
        );
        assert_ne!(
            session_identifier("user", "session"),
            identifier("challenge:user!session")
        );
    }
}
