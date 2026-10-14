//! Opt-in default TOTP lifecycle for credential sign-in. Secret records are never
//! exposed through user/session projections; durable stores must encrypt records.
//!
//! The supported profile is SHA-1, six digits, a 30-second timestep and a one-step
//! window, with no timestep replay suppression. Recovery codes, browser challenges
//! and trusted-device identifiers are consumed under the store transaction lock.
//! Enabling protection affects subsequent password logins; existing sessions and
//! social/provider flows retain their independently configured policy. Public
//! service methods accept trusted server time, not caller-selected HTTP timestamps.
//! OTP delivery, custom TOTP parameters and backup `disableSession` are not exposed
//! by this initial typed profile. Recovery values are authorized disclosures and
//! must not be logged; this module intentionally provides no secret-bearing Debug.
mod crypto;
mod enrollment;
pub(crate) mod signin;
mod verification;
use crate::{
    accounts::CredentialService,
    credentials,
    sessions::{new_session, public_user},
    store::{AuthStore, AuthTransaction, Model},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};
use std::sync::Arc;
use zeroize::Zeroizing;

/// Trusted fixed defaults. No Debug/Serialize: the service holds signing secrets.
pub struct TwoFactorService<S> {
    pub(crate) store: Arc<S>,
    secret: Zeroizing<Vec<u8>>,
    issuer: String,
}
/// A successful state transition. Explicit wire projection can contain recovery
/// credentials and must only be returned to the authorized browser.
pub struct FactorResponse {
    pub(crate) body: Value,
    pub(crate) session: Option<Value>,
    pub(crate) persistent: bool,
    pub(crate) trust: Option<String>,
    pub(crate) clear_challenge: bool,
    pub(crate) clear_trust: bool,
}
impl FactorResponse {
    /// Explicit authorized response projection; never log its contents.
    #[must_use]
    pub fn body(&self) -> &Value {
        &self.body
    }
    /// Token for the replacement cookie; may differ from the audited enrollment body.
    #[must_use]
    pub fn replacement_session_token(&self) -> Option<&str> {
        self.session
            .as_ref()
            .and_then(|session| session["token"].as_str())
    }
    /// Explicit trusted transport access; never log the returned credential.
    #[must_use]
    pub fn trusted_device_value(&self) -> Option<&str> {
        self.trust.as_deref()
    }
    #[must_use]
    pub const fn persistent(&self) -> bool {
        self.persistent
    }
    #[must_use]
    pub const fn clear_challenge(&self) -> bool {
        self.clear_challenge
    }
    #[must_use]
    pub const fn clear_trust(&self) -> bool {
        self.clear_trust
    }
}
/// Pending credential challenge; no authenticated session exists yet.
pub struct TwoFactorChallenge {
    pub(crate) identifier: String,
    pub(crate) persistent: bool,
}
impl TwoFactorChallenge {
    /// Opaque bearer challenge for an authenticated, signed `HttpOnly` cookie only.
    #[must_use]
    pub fn identifier(&self) -> &str {
        &self.identifier
    }
    #[must_use]
    pub const fn persistent(&self) -> bool {
        self.persistent
    }
}
/// Credential outcome forces callers to distinguish authentication from a challenge.
pub enum CredentialSigninOutcome {
    Authenticated {
        grant: crate::accounts::AuthGrant,
        trust: Option<String>,
    },
    Challenge(TwoFactorChallenge),
}
impl<S: AuthStore> TwoFactorService<S> {
    /// The fixed profile protects password sign-in. Social callback behavior is
    /// independent, matching the upstream plugin's credential-hook scope.
    /// # Errors
    /// Rejects short signing secrets and invalid issuer lengths.
    pub fn new(store: Arc<S>, secret: Vec<u8>, issuer: String) -> BlueprintResult<Self> {
        if secret.len() < 32 || issuer.is_empty() || issuer.len() > 256 {
            return Err(error(
                "TWO_FACTOR_INVALID_CONFIG",
                "Invalid two-factor configuration",
            ));
        }
        Ok(Self {
            store,
            secret: Zeroizing::new(secret),
            issuer,
        })
    }
    async fn password_snapshot(
        &self,
        token: &str,
        password: String,
        now: DateTime<Utc>,
    ) -> BlueprintResult<PasswordSnapshot> {
        crate::accounts::validate_length(&password, false)?;
        let mut tx = self.store.begin(true).await?;
        let (user, _) = authenticated(&mut tx, token, now).await?;
        let account = crate::accounts::find_credential(&mut tx, text(&user, "id")?)
            .await?
            .ok_or_else(invalid_password)?;
        let expected = Zeroizing::new(text(&account, "password")?.to_owned());
        tx.commit().await?;
        let supplied = Zeroizing::new(password);
        let hash = expected.clone();
        if !CredentialService::new(self.store.clone())
            .run_crypto(move || Ok(credentials::verify_password(&supplied, &hash)))
            .await?
        {
            return Err(invalid_password());
        }
        Ok(PasswordSnapshot {
            account_id: text(&account, "id")?.to_owned(),
            user_id: text(&user, "id")?.to_owned(),
            expected,
        })
    }
}
struct PasswordSnapshot {
    account_id: String,
    user_id: String,
    expected: Zeroizing<String>,
}
impl PasswordSnapshot {
    async fn check<T: AuthTransaction>(&self, tx: &mut T, user: &Value) -> BlueprintResult<()> {
        let current = tx
            .get(Model::Account, &self.account_id)
            .await?
            .ok_or_else(invalid_password)?;
        if current["password"].as_str() != Some(self.expected.as_str())
            || current["userId"] != self.user_id
            || user["id"] != self.user_id
        {
            return Err(invalid_password());
        }
        Ok(())
    }
}
async fn authenticated<T: AuthTransaction>(
    tx: &mut T,
    token: &str,
    now: DateTime<Utc>,
) -> BlueprintResult<(Value, Value)> {
    let session = tx
        .find_unique(Model::Session, "token", token)
        .await?
        .ok_or_else(unauthorized)?;
    if time(&session, "expiresAt")? <= now {
        return Err(unauthorized());
    }
    let user = tx
        .get(Model::User, text(&session, "userId")?)
        .await?
        .ok_or_else(unauthorized)?;
    Ok((user, session))
}
pub(crate) fn enabled(user: &Value) -> BlueprintResult<bool> {
    match user.get("twoFactorEnabled") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err(corrupt()),
    }
}
fn output_user(user: &Value) -> Value {
    let mut user = public_user(user);
    if user.get("twoFactorEnabled").is_none() {
        user["twoFactorEnabled"] = json!(false);
    }
    user
}
fn text<'a>(row: &'a Value, field: &str) -> BlueprintResult<&'a str> {
    row[field].as_str().ok_or_else(corrupt)
}
fn time(row: &Value, field: &str) -> BlueprintResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text(row, field)?)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|_| corrupt())
}
fn timestamp(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn expiry(now: DateTime<Utc>, seconds: i64) -> BlueprintResult<String> {
    now.checked_add_signed(Duration::seconds(seconds))
        .map(timestamp)
        .ok_or_else(corrupt)
}
fn error(code: &str, message: &str) -> BlueprintError {
    BlueprintError::new(code, message)
}
fn corrupt() -> BlueprintError {
    error(
        "TWO_FACTOR_INTERNAL",
        "Unable to use two-factor authentication state",
    )
}
fn unauthorized() -> BlueprintError {
    error("UNAUTHORIZED", "Unauthorized")
}
fn invalid_password() -> BlueprintError {
    error("INVALID_PASSWORD", "Invalid password")
}
fn invalid_challenge() -> BlueprintError {
    error("INVALID_TWO_FACTOR_COOKIE", "Invalid two factor cookie")
}
fn empty_response(body: Value) -> FactorResponse {
    FactorResponse {
        body,
        session: None,
        persistent: true,
        trust: None,
        clear_challenge: false,
        clear_trust: false,
    }
}
async fn factor<T: AuthTransaction>(tx: &mut T, user: &Value) -> BlueprintResult<Value> {
    tx.find_unique(Model::TwoFactor, "userId", text(user, "id")?)
        .await?
        .ok_or_else(|| error("TOTP_NOT_ENABLED", "TOTP is not enabled"))
}
fn secret(row: &Value) -> BlueprintResult<Zeroizing<Vec<u8>>> {
    let encoded = text(row, "secret")?;
    if encoded.len() > 128 {
        return Err(corrupt());
    }
    let bytes = STANDARD.decode(encoded).map_err(|_| corrupt())?;
    if bytes.len() != 32 {
        return Err(corrupt());
    }
    Ok(Zeroizing::new(bytes))
}
async fn verification_row<T: AuthTransaction>(
    tx: &mut T,
    identifier: &str,
) -> BlueprintResult<Option<Value>> {
    if identifier.len() > 128 {
        return Ok(None);
    }
    let page = tx
        .find_page(Model::Verification, "identifier", identifier, None, 2)
        .await?;
    if page.records.len() > 1 {
        return Err(corrupt());
    }
    Ok(page.records.into_iter().next())
}

#[cfg(test)]
mod tests;
