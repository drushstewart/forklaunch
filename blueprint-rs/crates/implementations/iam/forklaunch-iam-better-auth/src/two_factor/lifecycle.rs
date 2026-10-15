//! Trusted lifecycle policy and transaction-bound password authorization.
use super::{
    AuthStore, AuthTransaction, BlueprintResult, CredentialService, DateTime, Model,
    TwoFactorService, Utc, Value, Zeroizing, authenticated, corrupt, credentials, enabled, error,
    factor, invalid_password, text,
};

/// Server-only lifecycle policy. Defaults preserve required credential passwords,
/// verified enrollment and enabled TOTP. Request fields cannot select these flags.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TwoFactorLifecycleOptions {
    pub(super) disable_totp: bool,
    pub(super) skip_verification_on_enable: bool,
    enrollment_issuer: Option<String>,
    uri_issuer: Option<String>,
    allow_passwordless: bool,
    uri_passwordless: Option<bool>,
    backup_passwordless: Option<bool>,
}
impl TwoFactorLifecycleOptions {
    #[must_use]
    pub fn with_totp_disabled(mut self, disabled: bool) -> Self {
        self.disable_totp = disabled;
        self
    }
    #[must_use]
    pub fn with_skip_verification_on_enable(mut self, skip: bool) -> Self {
        self.skip_verification_on_enable = skip;
        self
    }
    /// Allows missing passwords only when the authoritative credential account has
    /// no password. A password added before mutation invalidates the absence proof.
    #[must_use]
    pub fn with_passwordless(mut self, allow: bool) -> Self {
        self.allow_passwordless = allow;
        self
    }
    /// None inherits the global setting; Some(false) explicitly requires a password.
    #[must_use]
    pub fn with_uri_passwordless(mut self, allow: Option<bool>) -> Self {
        self.uri_passwordless = allow;
        self
    }
    /// None inherits the global setting; Some(false) explicitly requires a password.
    #[must_use]
    pub fn with_backup_passwordless(mut self, allow: Option<bool>) -> Self {
        self.backup_passwordless = allow;
        self
    }
    /// Enrollment and later URI retrieval use independent issuer overrides. Empty
    /// strings behave as absent, falling back to the service's application issuer.
    /// # Errors
    /// Rejects either issuer exceeding the existing 256-byte trusted bound.
    pub fn with_issuers(
        mut self,
        enrollment: Option<String>,
        uri: Option<String>,
    ) -> BlueprintResult<Self> {
        if enrollment.as_ref().is_some_and(|v| v.len() > 256)
            || uri.as_ref().is_some_and(|v| v.len() > 256)
        {
            return Err(error(
                "TWO_FACTOR_INVALID_CONFIG",
                "Invalid two-factor issuer",
            ));
        }
        self.enrollment_issuer = enrollment.filter(|v| !v.is_empty());
        self.uri_issuer = uri.filter(|v| !v.is_empty());
        Ok(self)
    }
    pub(crate) fn password_optional(&self, path: &str) -> bool {
        match path {
            "/two-factor/get-totp-uri" => self.uri_passwordless.unwrap_or(self.allow_passwordless),
            "/two-factor/generate-backup-codes" => {
                self.backup_passwordless.unwrap_or(self.allow_passwordless)
            }
            "/two-factor/enable" | "/two-factor/disable" => self.allow_passwordless,
            _ => false,
        }
    }
    pub(super) fn enrollment_issuer<'a>(
        &'a self,
        body: Option<&'a str>,
        fallback: &'a str,
    ) -> &'a str {
        body.filter(|v| !v.is_empty())
            .or(self.enrollment_issuer.as_deref())
            .unwrap_or(fallback)
    }
    pub(super) fn uri_issuer<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.uri_issuer.as_deref().unwrap_or(fallback)
    }
}
#[derive(Clone, Copy)]
pub(super) enum PasswordAction {
    General,
    Uri,
    Backup,
}
impl PasswordAction {
    fn path(self) -> &'static str {
        match self {
            Self::General => "/two-factor/enable",
            Self::Uri => "/two-factor/get-totp-uri",
            Self::Backup => "/two-factor/generate-backup-codes",
        }
    }
}
impl<S> TwoFactorService<S> {
    #[must_use]
    pub fn with_lifecycle_options(mut self, options: TwoFactorLifecycleOptions) -> Self {
        self.lifecycle_options = options;
        self
    }
    #[cfg(feature = "http")]
    pub(crate) fn lifecycle_options(&self) -> &TwoFactorLifecycleOptions {
        &self.lifecycle_options
    }
}
impl<S: AuthStore> TwoFactorService<S> {
    pub(super) async fn password_snapshot(
        &self,
        token: &str,
        password: String,
        now: DateTime<Utc>,
        action: PasswordAction,
    ) -> BlueprintResult<PasswordSnapshot> {
        let mut tx = self.store.begin(true).await?;
        let (user, _) = authenticated(&mut tx, token, now).await?;
        match action {
            PasswordAction::Uri => {
                if self.lifecycle_options.disable_totp {
                    return Err(error("TOTP_NOT_CONFIGURED", "totp isn't configured"));
                }
                factor(&mut tx, &user).await?;
            }
            PasswordAction::Backup if !enabled(&user)? => {
                return Err(error("TWO_FACTOR_NOT_ENABLED", "Two factor is not enabled"));
            }
            _ => {}
        }
        let optional = self.lifecycle_options.password_optional(action.path());
        if !optional {
            crate::accounts::validate_length(&password, false)?;
        }
        let user_id = text(&user, "id")?.to_owned();
        let account = crate::accounts::find_credential(&mut tx, &user_id).await?;
        let expected = credential_password(account.as_ref())?;
        if expected.is_none() && optional {
            tx.commit().await?;
            return Ok(PasswordSnapshot::Absent { user_id });
        }
        let expected = Zeroizing::new(expected.ok_or_else(invalid_password)?.to_owned());
        let account_id = text(account.as_ref().ok_or_else(invalid_password)?, "id")?.to_owned();
        if optional {
            crate::accounts::validate_length(&password, false)?;
        }
        tx.commit().await?;
        let supplied = Zeroizing::new(password);
        let hash = expected.clone();
        if !CredentialService::new(self.store.clone())
            .run_crypto(move || Ok(credentials::verify_password(&supplied, &hash)))
            .await?
        {
            return Err(invalid_password());
        }
        Ok(PasswordSnapshot::Required {
            account_id,
            user_id,
            expected,
        })
    }
}
fn credential_password(account: Option<&Value>) -> BlueprintResult<Option<&str>> {
    match account.and_then(|v| v.get("password")) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok((!value.is_empty()).then_some(value.as_str())),
        Some(_) => Err(corrupt()),
    }
}
pub(super) enum PasswordSnapshot {
    Required {
        account_id: String,
        user_id: String,
        expected: Zeroizing<String>,
    },
    Absent {
        user_id: String,
    },
}
impl PasswordSnapshot {
    pub(super) async fn check<T: AuthTransaction>(
        &self,
        tx: &mut T,
        user: &Value,
    ) -> BlueprintResult<()> {
        match self {
            Self::Required {
                account_id,
                user_id,
                expected,
            } => {
                let current = tx
                    .get(Model::Account, account_id)
                    .await?
                    .ok_or_else(invalid_password)?;
                if current["password"].as_str() != Some(expected.as_str())
                    || current["userId"] != *user_id
                    || user["id"] != *user_id
                {
                    return Err(invalid_password());
                }
            }
            Self::Absent { user_id } => {
                if user["id"] != *user_id {
                    return Err(invalid_password());
                }
                let current = crate::accounts::find_credential(tx, user_id).await?;
                if credential_password(current.as_ref())?.is_some() {
                    return Err(invalid_password());
                }
            }
        }
        Ok(())
    }
}
