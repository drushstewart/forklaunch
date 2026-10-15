//! Async field callbacks use durable leases and exact state comparison, not open transactions.
use super::backup_codec::{BackupCodeCodecText, Codec, failure};
use super::backup_codec_state::{self as state, Lease, Phase};
use super::otp::{Authority, authority};
use super::{
    AuthStore, AuthTransaction, BackupVerificationOptions, BlueprintResult, DateTime, Duration,
    Engine, FactorResponse, Model, STANDARD, TwoFactorService, Utc, Value, corrupt, empty_response,
    enabled, error, factor, invalid_challenge, json, text, time, timestamp, verification_row,
};
use std::time::Instant;
type Clock = dyn Fn() -> DateTime<Utc> + Send + Sync;
pub(super) fn stale() -> super::BlueprintError {
    error(
        "TWO_FACTOR_BACKUP_CODEC_CONFLICT",
        "Backup code state changed; retry the operation",
    )
}
fn auth_digest(bound: &Authority) -> BlueprintResult<String> {
    state::digest(&json!([
        bound.user["id"],
        bound.user["twoFactorEnabled"],
        bound.user["updatedAt"],
        bound
            .session
            .as_ref()
            .map(|s| json!([s["id"], s["userId"], s["expiresAt"]])),
        bound.challenge
    ]))
}
fn auth_deadline(bound: &Authority) -> BlueprintResult<(i64, bool)> {
    if let Some(session) = &bound.session {
        Ok((time(session, "expiresAt")?.timestamp_millis(), false))
    } else {
        Ok((
            time(
                bound.challenge.as_ref().ok_or_else(invalid_challenge)?,
                "expiresAt",
            )?
            .timestamp_millis(),
            true,
        ))
    }
}
fn guard(clock: &Clock, lease: &Lease, bound: &Authority) -> BlueprintResult<DateTime<Utc>> {
    let now = clock();
    let millis = now.timestamp_millis();
    let (until, inclusive) = auth_deadline(bound)?;
    if millis < lease.started
        || millis >= lease.deadline
        || millis > until
        || (!inclusive && millis == until)
    {
        return Err(stale());
    }
    Ok(now)
}
fn custom_field(row: &mut Value, value: &str) {
    row["backupCodes"] = json!(value);
    row["backupCodesFormat"] = json!(2);
    row["backupCodesRevision"] = json!(uuid::Uuid::new_v4().to_string());
}
async fn callback(
    adapter: &Codec,
    encrypt: bool,
    value: String,
    started: Instant,
) -> BlueprintResult<BackupCodeCodecText> {
    let remaining = adapter
        .options
        .timeout()
        .checked_sub(started.elapsed())
        .filter(|d| !d.is_zero())
        .ok_or_else(failure)?;
    let value = BackupCodeCodecText::new(value)?;
    let future = if encrypt {
        adapter.hook.encrypt(value)
    } else {
        adapter.hook.decrypt(value)
    };
    // No detached work; dropping this future cancels cooperative callback polling.
    tokio::time::timeout(remaining, future)
        .await
        .map_err(|_| failure())?
        .map_err(|_| failure())
}
fn parse(plain: &str) -> BlueprintResult<Option<Vec<String>>> {
    if plain.len() > super::backup_storage::MAX_JSON {
        return Err(failure());
    }
    match serde_json::from_str::<Value>(plain) {
        Err(_) | Ok(Value::Null) => Ok(None),
        Ok(value) => super::backup_storage::codes(&value)
            .map(Some)
            .map_err(|_| failure()),
    }
}
impl<S: AuthStore> TwoFactorService<S> {
    /// Fresh trusted-clock form of enrollment. Built-in policies retain one sampled time.
    /// # Errors
    /// Rejects stale authority, password changes, lease conflicts and adapter failures.
    pub async fn enable_with_backup_clock(
        &self,
        token: &str,
        password: String,
        issuer: Option<&str>,
        clock: &Clock,
    ) -> BlueprintResult<FactorResponse> {
        if self.backup_codec.is_none() {
            return self.enable(token, password, issuer, clock()).await;
        }
        self.mutate_custom_backup(token, password, issuer, true, clock)
            .await
    }
    /// Fresh trusted-clock form of regeneration with no transaction across callbacks.
    /// # Errors
    /// Rejects stale authority/password, lease conflicts and adapter failures.
    pub async fn regenerate_backup_codes_with_clock(
        &self,
        token: &str,
        password: String,
        clock: &Clock,
    ) -> BlueprintResult<FactorResponse> {
        if self.backup_codec.is_none() {
            return self.regenerate_backup_codes(token, password, clock()).await;
        }
        self.mutate_custom_backup(token, password, None, false, clock)
            .await
    }
    /// Fresh trusted-clock form of backup verification. A re-encoding failure
    /// preserves codes but consumes only an unchanged pending attempt capability.
    /// # Errors
    /// Rejects invalid proof, expired/superseded authority, lockout and adapter failures.
    pub async fn verify_backup_with_clock(
        &self,
        token: &str,
        challenge: &str,
        code: &str,
        options: BackupVerificationOptions,
        clock: &Clock,
    ) -> BlueprintResult<FactorResponse> {
        if self.backup_codec.is_none() {
            return self
                .verify_backup_with_options(token, challenge, code, options, clock())
                .await;
        }
        self.verify_custom_backup(token, challenge, code, options, clock)
            .await
    }
    async fn reserve<T: AuthTransaction>(
        &self,
        tx: &mut T,
        bound: &Authority,
        row: Option<&Value>,
        phase: Phase,
        now: DateTime<Utc>,
        adapter: &Codec,
    ) -> BlueprintResult<Option<Lease>> {
        let user_id = text(&bound.user, "id")?.to_owned();
        if let Some(previous) = verification_row(tx, &state::identifier(&user_id)).await? {
            let lease = Lease::read(&previous)?;
            if lease.deadline > now.timestamp_millis() {
                return Err(stale());
            }
            state::recover(tx, &previous, &lease).await?;
        }
        // Recovery may have invalidated an exact pending challenge. Recheck before reserving it.
        if let Some(challenge) = &bound.challenge
            && tx
                .get(Model::Verification, text(challenge, "id")?)
                .await?
                .as_ref()
                != Some(challenge)
        {
            return Ok(None);
        }
        let lease = Lease {
            version: 1,
            owner: uuid::Uuid::new_v4().to_string(),
            phase,
            started: now.timestamp_millis(),
            deadline: now
                .checked_add_signed(
                    Duration::from_std(adapter.options.timeout()).map_err(|_| failure())?,
                )
                .ok_or_else(failure)?
                .timestamp_millis(),
            user_id,
            factor: row.map(state::factor_digest).transpose()?,
            authority: auth_digest(bound)?,
            challenge: bound
                .challenge
                .as_ref()
                .map(|row| Ok((text(row, "id")?.to_owned(), state::digest(row)?)))
                .transpose()?,
        };
        tx.insert(Model::Verification, lease.row(now)?).await?;
        Ok(Some(lease))
    }
    async fn fresh<T: AuthTransaction>(
        &self,
        tx: &mut T,
        lease: &Lease,
        token: &str,
        challenge: &str,
        clock: &Clock,
    ) -> BlueprintResult<(Authority, Option<Value>, Value)> {
        let lease_row = state::current(tx, lease).await?;
        let now = clock();
        let bound = authority(tx, token, challenge, now).await?;
        if auth_digest(&bound)? != lease.authority {
            return Err(stale());
        }
        let row = tx
            .find_unique(Model::TwoFactor, "userId", &lease.user_id)
            .await?;
        if row.as_ref().map(state::factor_digest).transpose()? != lease.factor {
            return Err(stale());
        }
        guard(clock, lease, &bound)?;
        Ok((bound, row, lease_row))
    }
    async fn abandon(&self, lease: &Lease) -> BlueprintResult<()> {
        let mut tx = self.store.begin(true).await?;
        let Some(row) = verification_row(&mut tx, &state::identifier(&lease.user_id)).await? else {
            return Ok(());
        };
        let current = Lease::read(&row)?;
        if current.owner == lease.owner {
            state::recover(&mut tx, &row, &current).await?;
        }
        tx.commit().await
    }
    // Keep reservation, user callback and publication boundaries visible in one operation.
    #[allow(clippy::too_many_lines)]
    pub(super) async fn mutate_custom_backup(
        &self,
        token: &str,
        password: String,
        issuer: Option<&str>,
        enroll: bool,
        clock: &Clock,
    ) -> BlueprintResult<FactorResponse> {
        let issuer = self
            .lifecycle_options
            .enrollment_issuer(issuer, &self.issuer);
        if issuer.len() > 256 {
            return Err(error("VALIDATION_ERROR", "Invalid issuer"));
        }
        let checked = self
            .password_snapshot(
                token,
                password,
                clock(),
                if enroll {
                    super::PasswordAction::General
                } else {
                    super::PasswordAction::Backup
                },
            )
            .await?;
        if enroll && self.lifecycle_options.disable_totp {
            return Err(error("TOTP_NOT_CONFIGURED", "TOTP is not available"));
        }
        let adapter = self.backup_codec.as_ref().ok_or_else(failure)?;
        let mut tx = self.store.begin(true).await?;
        let start = clock();
        let elapsed = Instant::now();
        let bound = authority(&mut tx, token, "", start).await?;
        if bound.session.is_none() {
            return Err(super::unauthorized());
        }
        checked.check(&mut tx, &bound.user).await?;
        let row = tx
            .find_unique(Model::TwoFactor, "userId", text(&bound.user, "id")?)
            .await?;
        if enroll && row.as_ref().is_some_and(|r| r["verified"] != false) {
            return Err(error("TOTP_ALREADY_ENABLED", "TOTP is already enabled"));
        }
        if !enroll && (!enabled(&bound.user)? || row.is_none()) {
            return Err(error("TWO_FACTOR_NOT_ENABLED", "Two factor is not enabled"));
        }
        let codes = self.generate_backup_codes()?;
        let plain = serde_json::to_string(&codes).map_err(|_| failure())?;
        let lease = self
            .reserve(
                &mut tx,
                &bound,
                row.as_ref(),
                if enroll {
                    Phase::EnrollEncoding
                } else {
                    Phase::RegenerateEncoding
                },
                start,
                adapter,
            )
            .await?;
        let Some(lease) = lease else {
            tx.commit().await?;
            return Err(stale());
        };
        guard(clock, &lease, &bound)?;
        tx.commit().await?;
        guard(clock, &lease, &bound)?;
        let encoded = match callback(adapter, true, plain, elapsed).await {
            Ok(value) => value,
            Err(error) => {
                self.abandon(&lease).await?;
                return Err(error);
            }
        };
        let mut tx = self.store.begin(true).await?;
        let (fresh, existing, lease_row) = self.fresh(&mut tx, &lease, token, "", clock).await?;
        checked.check(&mut tx, &fresh.user).await?;
        let now = guard(clock, &lease, &fresh)?;
        let response = if enroll {
            let secret = super::crypto::random(32)?;
            let id = existing
                .as_ref()
                .map(|row| text(row, "id").map(str::to_owned))
                .transpose()?
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let mut row = json!({"id":id,"userId":fresh.user["id"],"secret":STANDARD.encode(secret.as_slice()),"verified":self.lifecycle_options.skip_verification_on_enable,"failedVerificationCount":0,"lockedUntil":null});
            custom_field(&mut row, encoded.as_str());
            if existing.is_some() {
                tx.replace(Model::TwoFactor, row).await?;
            } else {
                tx.insert(Model::TwoFactor, row).await?;
            }
            let mut user = fresh.user.clone();
            let replacement = if self.lifecycle_options.skip_verification_on_enable {
                user["twoFactorEnabled"] = json!(true);
                user["updatedAt"] = json!(timestamp(now));
                tx.replace(Model::User, user.clone()).await?;
                Some(
                    super::enrollment::rotate(
                        &mut tx,
                        &user,
                        fresh.session.as_ref().ok_or_else(super::unauthorized)?,
                        now,
                        self.session_policy,
                    )
                    .await?,
                )
            } else {
                None
            };
            let mut response = empty_response(
                json!({"method":"totp","totpURI":super::enrollment::uri(issuer,text(&user,"email")?,&secret,self.totp_options),"backupCodes":codes}),
            );
            response.session = replacement;
            response
        } else {
            let mut row = existing.ok_or_else(stale)?;
            custom_field(&mut row, encoded.as_str());
            tx.replace(Model::TwoFactor, row).await?;
            empty_response(json!({"status":true,"backupCodes":codes}))
        };
        tx.delete(Model::Verification, text(&lease_row, "id")?)
            .await?;
        guard(clock, &lease, &fresh)?;
        tx.commit().await?;
        Ok(response)
    }
    async fn check_pending<T: AuthTransaction>(
        &self,
        tx: &mut T,
        bound: &Authority,
        row: &mut Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<()> {
        if let Some(challenge) = &bound.challenge {
            if self.check_account_lock(row, now)? {
                tx.replace(Model::TwoFactor, row.clone()).await?;
            }
            if challenge["attempts"]
                .as_u64()
                .ok_or_else(invalid_challenge)?
                >= 5
            {
                return Err(error(
                    "TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE",
                    "Too many attempts. Please request a new code.",
                ));
            }
        }
        Ok(())
    }
    #[allow(clippy::too_many_lines)]
    pub(super) async fn verify_custom_backup(
        &self,
        token: &str,
        challenge: &str,
        supplied: &str,
        options: BackupVerificationOptions,
        clock: &Clock,
    ) -> BlueprintResult<FactorResponse> {
        if supplied.len() > 128 {
            return Err(error("INVALID_CODE", "Invalid code"));
        }
        let adapter = self.backup_codec.as_ref().ok_or_else(failure)?;
        let mut tx = self.store.begin(true).await?;
        let start = clock();
        let elapsed = Instant::now();
        let bound = authority(&mut tx, token, challenge, start).await?;
        let mut row = factor(&mut tx, &bound.user).await.map_err(|e| {
            if e.code == "TOTP_NOT_ENABLED" {
                error("BACKUP_CODES_NOT_ENABLED", "Backup codes are not enabled")
            } else {
                e
            }
        })?;
        if let Err(error) = self.check_pending(&mut tx, &bound, &mut row, start).await {
            if error.code == "TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE" {
                tx.delete(
                    Model::Verification,
                    text(
                        bound.challenge.as_ref().ok_or_else(invalid_challenge)?,
                        "id",
                    )?,
                )
                .await?;
                tx.commit().await?;
            }
            return Err(error);
        }
        let legacy = if row.get("backupCodesFormat").is_none() && row["backupCodes"].is_array() {
            Some(super::backup_storage::codes(&row["backupCodes"])?)
        } else {
            if row["backupCodesFormat"] != 1 && row["backupCodesFormat"] != 2 {
                return Err(failure());
            }
            None
        };
        let encoded = if legacy.is_none() {
            Some(BackupCodeCodecText::new(
                text(&row, "backupCodes")?.to_owned(),
            )?)
        } else {
            None
        };
        let lease = self
            .reserve(&mut tx, &bound, Some(&row), Phase::Decoding, start, adapter)
            .await?;
        let Some(mut lease) = lease else {
            tx.commit().await?;
            return Err(stale());
        };
        guard(clock, &lease, &bound)?;
        tx.commit().await?;
        guard(clock, &lease, &bound)?;
        let decoded = if let Some(codes) = legacy {
            Ok(Some(codes))
        } else {
            match callback(
                adapter,
                false,
                encoded.ok_or_else(failure)?.as_str().to_owned(),
                elapsed,
            )
            .await
            {
                Ok(value) => parse(value.as_str()),
                Err(error) => Err(error),
            }
        };
        let codes = match decoded {
            Ok(value) => value,
            Err(error) => {
                self.abandon(&lease).await?;
                return Err(error);
            }
        };
        let valid = codes.as_ref().is_some_and(|codes| {
            codes.iter().fold(false, |yes, code| {
                let difference = code
                    .bytes()
                    .zip(supplied.bytes())
                    .fold(0u8, |n, (a, b)| n | (a ^ b));
                yes | (code.len() == supplied.len() && difference == 0)
            })
        });
        let mut tx = self.store.begin(true).await?;
        let (fresh, existing, mut lease_row) =
            self.fresh(&mut tx, &lease, token, challenge, clock).await?;
        let mut row = existing.ok_or_else(stale)?;
        let now = guard(clock, &lease, &fresh)?;
        self.check_pending(&mut tx, &fresh, &mut row, now).await?;
        if !valid {
            if let Some(mut pending) = fresh.challenge.clone() {
                pending["attempts"] = json!(pending["attempts"].as_u64().ok_or_else(corrupt)? + 1);
                if self.record_account_failure(&mut row, now)? {
                    tx.replace(Model::TwoFactor, row).await?;
                }
                tx.replace(Model::Verification, pending).await?;
            }
            tx.delete(Model::Verification, text(&lease_row, "id")?)
                .await?;
            guard(clock, &lease, &fresh)?;
            tx.commit().await?;
            return Err(error("INVALID_BACKUP_CODE", "Invalid backup code"));
        }
        lease.phase = Phase::Reencoding;
        lease.write(&mut lease_row)?;
        tx.replace(Model::Verification, lease_row).await?;
        guard(clock, &lease, &fresh)?;
        tx.commit().await?;
        guard(clock, &lease, &fresh)?;
        let remaining: Vec<_> = codes
            .ok_or_else(failure)?
            .into_iter()
            .filter(|code| code != supplied)
            .collect();
        let encoded = match callback(
            adapter,
            true,
            serde_json::to_string(&remaining).map_err(|_| failure())?,
            elapsed,
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                self.abandon(&lease).await?;
                return Err(error);
            }
        };
        let mut tx = self.store.begin(true).await?;
        let (fresh, existing, lease_row) =
            self.fresh(&mut tx, &lease, token, challenge, clock).await?;
        let mut row = existing.ok_or_else(stale)?;
        let now = guard(clock, &lease, &fresh)?;
        self.check_pending(&mut tx, &fresh, &mut row, now).await?;
        custom_field(&mut row, encoded.as_str());
        let response = self
            .complete_verification(
                &mut tx,
                row,
                fresh.user.clone(),
                fresh.session.clone(),
                fresh.challenge.clone(),
                true,
                options.trust_device,
                options.disable_session,
                now,
            )
            .await?;
        tx.delete(Model::Verification, text(&lease_row, "id")?)
            .await?;
        guard(clock, &lease, &fresh)?;
        tx.commit().await?;
        Ok(response)
    }
}
/// Bounded tenant maintenance; continue with the returned opaque cursor.
pub struct BackupCodecCleanup {
    pub inspected: usize,
    pub removed: usize,
    pub next: Option<crate::store::AuthCursor>,
}
impl<S: AuthStore> TwoFactorService<S> {
    /// Reclaims up to256 examined records. Expired re-encoding leases consume only
    /// an unchanged pending challenge; other leases leave proofs unchanged.
    /// No scheduler is installed. Upgraded no-adapter instances may perform cleanup.
    /// # Errors
    /// Malformed recognized state and persistence failures roll back the page.
    pub async fn cleanup_backup_codec_states(
        &self,
        after: Option<crate::store::AuthCursor>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<BackupCodecCleanup> {
        let mut tx = self.store.begin(true).await?;
        let page = tx.list_page(Model::Verification, after, 256).await?;
        let mut removed = 0;
        for row in &page.records {
            if !row["identifier"]
                .as_str()
                .is_some_and(|s| s.starts_with(state::PREFIX))
            {
                continue;
            }
            let lease = Lease::read(row)?;
            if lease.deadline <= now.timestamp_millis() {
                state::recover(&mut tx, row, &lease).await?;
                removed += 1;
            }
        }
        tx.commit().await?;
        Ok(BackupCodecCleanup {
            inspected: page.records.len(),
            removed,
            next: page.next,
        })
    }
}
