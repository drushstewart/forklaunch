use super::{
    AuthStore, AuthTransaction, BlueprintResult, DateTime, Engine, FactorResponse, Model, STANDARD,
    TwoFactorService, Utc, Value, authenticated, crypto, empty_response, enabled, error, factor,
    json, new_session, secret, text, timestamp, verification_row,
};
impl<S: AuthStore> TwoFactorService<S> {
    /// Begins or replaces enrollment. Default policy requires verification; trusted
    /// skip-verification policy enables protection and rotates the current session.
    /// # Errors
    /// Requires authoritative session/password and rejects verified enrollment.
    pub async fn enable(
        &self,
        token: &str,
        password: String,
        issuer: Option<&str>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<FactorResponse> {
        let issuer = self
            .lifecycle_options
            .enrollment_issuer(issuer, &self.issuer);
        if issuer.len() > 256 {
            return Err(error("VALIDATION_ERROR", "Invalid issuer"));
        }
        let checked = self
            .password_snapshot(token, password, now, super::PasswordAction::General)
            .await?;
        if self.lifecycle_options.disable_totp {
            return Err(error("TOTP_NOT_CONFIGURED", "TOTP is not available"));
        }
        let mut tx = self.store.begin(true).await?;
        let (mut user, old_session) = authenticated(&mut tx, token, now).await?;
        checked.check(&mut tx, &user).await?;
        let existing = tx
            .find_unique(Model::TwoFactor, "userId", text(&user, "id")?)
            .await?;
        if existing
            .as_ref()
            .is_some_and(|row| row["verified"] != false)
        {
            return Err(error("TOTP_ALREADY_ENABLED", "TOTP is already enabled"));
        }
        let codes = self.generate_backup_codes()?;
        let secret = crypto::random(32)?;
        let id = existing
            .as_ref()
            .map(|row| text(row, "id").map(str::to_owned))
            .transpose()?
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut row = json!({"id":id,"userId":user["id"],"secret":STANDARD.encode(secret.as_slice()),"backupCodes":codes,"verified":self.lifecycle_options.skip_verification_on_enable,"failedVerificationCount":0,"lockedUntil":null});
        self.write_backup_codes(&mut row, &codes)?;
        if existing.is_some() {
            tx.replace(Model::TwoFactor, row).await?;
        } else {
            tx.insert(Model::TwoFactor, row).await?;
        }
        let replacement = if self.lifecycle_options.skip_verification_on_enable {
            user["twoFactorEnabled"] = json!(true);
            user["updatedAt"] = json!(timestamp(now));
            tx.replace(Model::User, user.clone()).await?;
            Some(rotate(&mut tx, &user, &old_session, now).await?)
        } else {
            None
        };
        tx.commit().await?;
        let mut response = empty_response(
            json!({"method":"totp","totpURI":uri(issuer,text(&user,"email")?,&secret,self.totp_options),"backupCodes":codes}),
        );
        response.session = replacement;
        Ok(response)
    }
    /// Returns an enrollment URI after checking the current password and session.
    /// # Errors
    /// Rejects missing factors, revoked sessions, changed passwords and storage errors.
    pub async fn get_uri(
        &self,
        token: &str,
        password: String,
        now: DateTime<Utc>,
    ) -> BlueprintResult<FactorResponse> {
        let checked = self
            .password_snapshot(token, password, now, super::PasswordAction::Uri)
            .await?;
        let mut tx = self.store.begin(true).await?;
        let (user, _) = authenticated(&mut tx, token, now).await?;
        checked.check(&mut tx, &user).await?;
        let row = factor(&mut tx, &user).await?;
        let result = uri(
            self.lifecycle_options.uri_issuer(&self.issuer),
            text(&user, "email")?,
            &secret(&row)?,
            self.totp_options,
        );
        tx.commit().await?;
        Ok(empty_response(json!({"totpURI":result})))
    }
    /// Replaces all backup codes atomically. The plaintext result is one authorized disclosure.
    /// # Errors
    /// Requires enabled protection, current session/password and valid storage state.
    pub async fn regenerate_backup_codes(
        &self,
        token: &str,
        password: String,
        now: DateTime<Utc>,
    ) -> BlueprintResult<FactorResponse> {
        let checked = self
            .password_snapshot(token, password, now, super::PasswordAction::Backup)
            .await?;
        let mut tx = self.store.begin(true).await?;
        let (user, _) = authenticated(&mut tx, token, now).await?;
        checked.check(&mut tx, &user).await?;
        if !enabled(&user)? {
            return Err(error("TWO_FACTOR_NOT_ENABLED", "Two factor is not enabled"));
        }
        let mut row = factor(&mut tx, &user).await?;
        let codes = self.generate_backup_codes()?;
        self.write_backup_codes(&mut row, &codes)?;
        tx.replace(Model::TwoFactor, row).await?;
        tx.commit().await?;
        Ok(empty_response(json!({"status":true,"backupCodes":codes})))
    }
    /// Disables protection and rotates only the current session, preserving other sessions.
    /// # Errors
    /// Rejects stale/revoked sessions, changed passwords and persistence failures atomically.
    pub async fn disable(
        &self,
        token: &str,
        password: String,
        trust: Option<&str>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<FactorResponse> {
        let checked = self
            .password_snapshot(token, password, now, super::PasswordAction::General)
            .await?;
        let mut tx = self.store.begin(true).await?;
        let (mut user, old_session) = authenticated(&mut tx, token, now).await?;
        checked.check(&mut tx, &user).await?;
        user["twoFactorEnabled"] = json!(false);
        user["updatedAt"] = json!(timestamp(now));
        tx.replace(Model::User, user.clone()).await?;
        if let Some(row) = tx
            .find_unique(Model::TwoFactor, "userId", text(&user, "id")?)
            .await?
        {
            tx.delete(Model::TwoFactor, text(&row, "id")?).await?;
        }
        if let Some(identifier) = trust.and_then(|trust| trust.rsplit_once('!').map(|(_, id)| id))
            && identifier.starts_with("trust-device-")
            && let Some(row) = verification_row(&mut tx, identifier).await?
            && row["value"] == user["id"]
        {
            tx.delete(Model::Verification, text(&row, "id")?).await?;
        }
        let session = rotate(&mut tx, &user, &old_session, now).await?;
        tx.commit().await?;
        let mut response = empty_response(json!({"status":true}));
        response.session = Some(session);
        response.clear_trust = trust.is_some();
        Ok(response)
    }
}
pub(super) async fn rotate<T: AuthTransaction>(
    tx: &mut T,
    user: &Value,
    old: &Value,
    now: DateTime<Utc>,
) -> BlueprintResult<Value> {
    let mut session = new_session(
        text(user, "id")?,
        old["ipAddress"].as_str().unwrap_or(""),
        old["userAgent"].as_str().unwrap_or(""),
        now,
    )?;
    for field in ["activeOrganizationId", "activeTeamId"] {
        if let Some(value) = old.get(field) {
            session[field] = value.clone();
        }
    }
    tx.insert(Model::Session, session.clone()).await?;
    tx.delete(Model::Session, text(old, "id")?).await?;
    Ok(session)
}
fn uri(issuer: &str, email: &str, secret: &[u8], options: super::TotpOptions) -> String {
    // Path components use encodeURIComponent; query values use URLSearchParams.
    const COMPONENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'!')
        .remove(b'~')
        .remove(b'*')
        .remove(b'\'')
        .remove(b'(')
        .remove(b')');
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("secret", &crypto::base32(secret))
        .append_pair("issuer", issuer)
        .append_pair("digits", &options.digits().count().to_string())
        .append_pair(
            "period",
            ryu_js::Buffer::new().format(options.period_seconds()),
        )
        .finish();
    format!(
        "otpauth://totp/{}:{}?{query}",
        percent_encoding::utf8_percent_encode(issuer, COMPONENT),
        percent_encoding::utf8_percent_encode(email, COMPONENT)
    )
}
