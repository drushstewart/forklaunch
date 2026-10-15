use super::{
    AuthStore, AuthTransaction, BlueprintResult, DateTime, FactorResponse, Model, TwoFactorService,
    Utc, authenticated, corrupt, crypto, empty_response, enabled, enrollment, error, expiry,
    factor, invalid_challenge, json, new_session, output_user, secret, text, time, timestamp,
    unauthorized, verification_row,
};
/// Trusted recovery verification options. Defaults retain session issuance.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BackupVerificationOptions {
    /// Suppress session issuance and trusted-device cookies. A pending challenge
    /// is consumed even though no authenticated session is issued.
    pub disable_session: bool,
    /// Issue a trusted-device cookie only when session issuance is enabled.
    pub trust_device: bool,
}

impl<S: AuthStore> TwoFactorService<S> {
    /// Verifies the default TOTP or one recovery code against an authoritative
    /// session or signed-cookie challenge. Code timesteps are not replay-tracked;
    /// challenges and recovery codes are consumed atomically with session issuance.
    /// # Errors
    /// Failed attempts commit their counters before returning an error. Other
    /// persistence failures roll back factor/session/verification mutations.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub async fn verify(
        &self,
        session_token: &str,
        challenge: &str,
        code: &str,
        recovery: bool,
        trust_device: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<FactorResponse> {
        self.verify_inner(
            session_token,
            challenge,
            code,
            recovery,
            trust_device,
            false,
            now,
        )
        .await
    }

    /// Verifies a recovery code with explicit session issuance options.
    /// An authenticated caller keeps its existing token; a pending caller receives
    /// only the user projection when session issuance is disabled. Both consume
    /// the recovery code atomically, and pending proof cannot later mint a session.
    /// # Errors
    /// Uses the same failure counters and transactional rollback as [`Self::verify`].
    pub async fn verify_backup_with_options(
        &self,
        session_token: &str,
        challenge: &str,
        code: &str,
        options: BackupVerificationOptions,
        now: DateTime<Utc>,
    ) -> BlueprintResult<FactorResponse> {
        self.verify_inner(
            session_token,
            challenge,
            code,
            true,
            options.trust_device,
            options.disable_session,
            now,
        )
        .await
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    async fn verify_inner(
        &self,
        session_token: &str,
        challenge: &str,
        code: &str,
        recovery: bool,
        trust_device: bool,
        disable_session: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<FactorResponse> {
        if !recovery && self.lifecycle_options.disable_totp {
            return Err(error("TOTP_NOT_CONFIGURED", "totp isn't configured"));
        }
        if code.len() > 128 {
            return Err(error("INVALID_CODE", "Invalid code"));
        }
        let mut tx = self.store.begin(true).await?;
        let authenticated = if session_token.is_empty() {
            None
        } else {
            match authenticated(&mut tx, session_token, now).await {
                Ok(value) => Some(value),
                Err(error) if error.code == "UNAUTHORIZED" => None,
                Err(error) => return Err(error),
            }
        };
        let mut pending = None;
        let (user, session) = if let Some((user, session)) = authenticated {
            (user, Some(session))
        } else {
            if !challenge.starts_with("2fa-") {
                return Err(invalid_challenge());
            }
            let row = verification_row(&mut tx, challenge)
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
            pending = Some(row);
            (user, None)
        };
        let mut row = factor(&mut tx, &user).await.map_err(|failure| {
            if recovery && failure.code == "TOTP_NOT_ENABLED" {
                error("BACKUP_CODES_NOT_ENABLED", "Backup codes are not enabled")
            } else {
                failure
            }
        })?;
        if pending.is_some() {
            if !recovery && row["verified"] == false {
                return Err(error("TOTP_NOT_ENABLED", "TOTP is not enabled"));
            }
            if self.check_account_lock(&mut row, now)? {
                tx.replace(Model::TwoFactor, row.clone()).await?;
            }
            let challenge_row = pending.as_ref().ok_or_else(invalid_challenge)?;
            let attempts = challenge_row["attempts"]
                .as_u64()
                .ok_or_else(invalid_challenge)?;
            if attempts >= 5 {
                tx.delete(Model::Verification, text(challenge_row, "id")?)
                    .await?;
                tx.commit().await?;
                return Err(error(
                    "TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE",
                    "Too many attempts. Please request a new code.",
                ));
            }
        }
        let valid = if recovery {
            let codes = row["backupCodes"].as_array().ok_or_else(corrupt)?;
            if codes.len() > super::backup_options::MAX_CODES
                || codes.iter().any(|value| {
                    value
                        .as_str()
                        .is_none_or(|value| value.len() > super::backup_options::MAX_CODE_BYTES)
                })
            {
                return Err(corrupt());
            }
            codes.iter().fold(false, |matched, value| {
                let expected = value.as_str().unwrap_or("");
                let difference = expected
                    .bytes()
                    .zip(code.bytes())
                    .fold(0u8, |acc, (a, b)| acc | (a ^ b));
                matched | (code.len() == expected.len() && difference == 0)
            })
        } else {
            crypto::matches_with_options(
                &secret(&row)?,
                code,
                now.timestamp_millis(),
                self.totp_options,
            )
        };
        if !valid {
            if let Some(mut challenge_row) = pending {
                let attempts = challenge_row["attempts"].as_u64().ok_or_else(corrupt)?;
                challenge_row["attempts"] = json!(attempts + 1);
                if self.record_account_failure(&mut row, now)? {
                    tx.replace(Model::TwoFactor, row).await?;
                }
                tx.replace(Model::Verification, challenge_row).await?;
                tx.commit().await?;
            }
            return Err(if recovery {
                error("INVALID_BACKUP_CODE", "Invalid backup code")
            } else {
                error("INVALID_CODE", "Invalid code")
            });
        }
        let mut factor_changed = recovery;
        if recovery {
            row["backupCodes"]
                .as_array_mut()
                .ok_or_else(corrupt)?
                .retain(|value| value.as_str() != Some(code));
        }
        let mut response = empty_response(
            json!({"token":session.as_ref().map(|row|row["token"].clone()),"user":output_user(&user)}),
        );
        if let Some(challenge_row) = pending {
            factor_changed |= self.account_lockout_options.enabled();
            self.reset_account_failures(&mut row);
            tx.delete(Model::Verification, text(&challenge_row, "id")?)
                .await?;
            if disable_session {
                // Upstream consumes its separate attempt capability and leaves an
                // inert cookie. Our single durable challenge is the capability:
                // delete it atomically with the recovery code, without cookies.
                response
                    .body
                    .as_object_mut()
                    .ok_or_else(corrupt)?
                    .remove("token");
                tx.replace(Model::TwoFactor, row).await?;
                tx.commit().await?;
                return Ok(response);
            }
            let mut next = new_session(
                text(&user, "id")?,
                challenge_row["ipAddress"].as_str().unwrap_or(""),
                challenge_row["userAgent"].as_str().unwrap_or(""),
                now,
            )?;
            response.persistent = challenge_row["persistent"].as_bool().unwrap_or(true);
            if !response.persistent {
                next["expiresAt"] = json!(expiry(now, 86400)?);
            }
            tx.insert(Model::Session, next.clone()).await?;
            response.body["token"] = next["token"].clone();
            response.session = Some(next);
            response.clear_challenge = true;
            if trust_device {
                response.trust = Some(self.create_trust(&mut tx, &user, now).await?);
            }
        } else if !recovery && row["verified"] != true {
            if !enabled(&user)? {
                let mut updated = user.clone();
                updated["twoFactorEnabled"] = json!(true);
                updated["updatedAt"] = json!(timestamp(now));
                tx.replace(Model::User, updated.clone()).await?;
                response.session = Some(
                    enrollment::rotate(
                        &mut tx,
                        &updated,
                        session.as_ref().ok_or_else(unauthorized)?,
                        now,
                    )
                    .await?,
                );
                // The audited upstream enrollment body retains the old token/user
                // snapshot even though only the replacement cookie session is active.
            }
            row["verified"] = json!(true);
            factor_changed = true;
        }
        if factor_changed {
            tx.replace(Model::TwoFactor, row).await?;
        }
        tx.commit().await?;
        Ok(response)
    }
}
