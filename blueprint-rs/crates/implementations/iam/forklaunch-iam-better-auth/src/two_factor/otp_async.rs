//! Durable leases for trusted asynchronous OTP callbacks.
use super::otp::{Authority, authority};
use super::otp_codec::{
    Codec, CodecKind, OtpCodecOptions, OtpEncryptionCodec, OtpHashCodec, failure,
};
use super::otp_codec_state::{Envelope, Phase, Proof};
use super::{
    AuthStore, AuthTransaction, BlueprintResult, DateTime, FactorResponse, Model, TwoFactorService,
    Utc, Value, error, json, text, verification_row,
};
use std::sync::Arc;

type Clock = dyn Fn() -> DateTime<Utc> + Send + Sync;
fn stale() -> super::BlueprintError {
    error("OTP_HAS_EXPIRED", "OTP has expired")
}
fn deadline(now: DateTime<Utc>, codec: &Codec) -> BlueprintResult<i64> {
    now.checked_add_signed(
        chrono::Duration::from_std(codec.options.timeout()).map_err(|_| failure())?,
    )
    .map(|time| time.timestamp_millis())
    .ok_or_else(failure)
}
fn same_authority(before: &Authority, after: &Authority) -> bool {
    before.identifier == after.identifier
        && before.user["id"] == after.user["id"]
        && before.user["twoFactorEnabled"] == after.user["twoFactorEnabled"]
        && before.user["updatedAt"] == after.user["updatedAt"]
        && before.session.as_ref().map(|row| &row["id"])
            == after.session.as_ref().map(|row| &row["id"])
        && before.challenge == after.challenge
}
fn authority_deadline(bound: &Authority) -> BlueprintResult<(i64, bool)> {
    if let Some(session) = bound.session.as_ref() {
        Ok((super::time(session, "expiresAt")?.timestamp_millis(), false))
    } else {
        Ok((
            super::time(bound.challenge.as_ref().ok_or_else(stale)?, "expiresAt")?
                .timestamp_millis(),
            true,
        ))
    }
}
fn final_clock_check(
    clock: &Clock,
    start: DateTime<Utc>,
    lease: i64,
    proof_expiry: i64,
    authority: (i64, bool),
) -> BlueprintResult<()> {
    let now = clock();
    let millis = now.timestamp_millis();
    if now < start
        || millis >= lease
        || millis > proof_expiry
        || millis > authority.0
        || (millis == authority.0 && !authority.1)
    {
        return Err(stale());
    }
    Ok(())
}
async fn owned<T: AuthTransaction>(
    tx: &mut T,
    snapshot: &Value,
    revision: &str,
) -> BlueprintResult<(Value, Envelope)> {
    let row = tx
        .get(Model::Verification, text(snapshot, "id")?)
        .await?
        .ok_or_else(stale)?;
    let state = Envelope::read(&row)?.ok_or_else(stale)?;
    if row["identifier"] != snapshot["identifier"] || state.revision != revision {
        return Err(stale());
    }
    Ok((row, state))
}
impl<S: AuthStore> TwoFactorService<S> {
    #[cfg(feature = "http")]
    pub(crate) fn has_custom_otp_codec(&self) -> bool {
        self.otp_codec.is_some()
    }

    /// Installs trusted async hashing. Callbacks run without a store transaction.
    #[must_use]
    pub fn with_otp_hash_codec(
        mut self,
        codec: Arc<dyn OtpHashCodec>,
        options: OtpCodecOptions,
    ) -> Self {
        self.otp_codec = Some(Arc::new(Codec {
            kind: CodecKind::Hash(codec),
            options,
        }));
        self
    }
    /// Installs trusted async encryption/decryption, independent of record encryption.
    #[must_use]
    pub fn with_otp_encryption_codec(
        mut self,
        codec: Arc<dyn OtpEncryptionCodec>,
        options: OtpCodecOptions,
    ) -> Self {
        self.otp_codec = Some(Arc::new(Codec {
            kind: CodecKind::Encryption(codec),
            options,
        }));
        self
    }
    /// Uses a fresh trusted clock before committing asynchronous callback results.
    /// # Errors
    /// Rejects invalid authority, superseded/expired leases and codec failures.
    pub async fn send_otp_with_clock(
        &self,
        token: &str,
        challenge: &str,
        clock: &Clock,
    ) -> BlueprintResult<FactorResponse> {
        if self.otp_codec.is_none() {
            return self.send_otp(token, challenge, clock()).await;
        }
        self.send_custom_otp(token, challenge, clock).await
    }
    /// Uses a fresh trusted clock before committing asynchronous callback results.
    /// # Errors
    /// Rejects invalid proofs, expired/revoked authority and superseded leases.
    pub async fn verify_otp_with_clock(
        &self,
        token: &str,
        challenge: &str,
        supplied: &str,
        trust: bool,
        clock: &Clock,
    ) -> BlueprintResult<FactorResponse> {
        if self.otp_codec.is_none() {
            return self
                .verify_otp(token, challenge, supplied, trust, clock())
                .await;
        }
        self.verify_custom_otp(token, challenge, supplied, trust, clock)
            .await
    }
    // Keep reservation, callback and publication transaction boundaries together.
    #[allow(clippy::too_many_lines)]
    pub(super) async fn send_custom_otp(
        &self,
        token: &str,
        challenge: &str,
        clock: &Clock,
    ) -> BlueprintResult<FactorResponse> {
        if self.otp_delivery.is_none() {
            return Err(error("OTP_NOT_CONFIGURED", "otp isn't configured"));
        }
        let codec = self.otp_codec.as_ref().ok_or_else(failure)?;
        let mut tx = self.store.begin(true).await?;
        let start = clock();
        let until = deadline(start, codec)?;
        let expires = start
            .checked_add_signed(chrono::Duration::milliseconds(
                self.otp_options
                    .validity()
                    .as_millis()
                    .try_into()
                    .map_err(|_| failure())?,
            ))
            .ok_or_else(failure)?;
        let code = super::otp::code(self.otp_options.digits())?;
        let bound = authority(&mut tx, token, challenge, start).await?;
        let previous = verification_row(&mut tx, &bound.identifier).await?;
        let mut row = previous.clone().unwrap_or_else(
            || json!({"id":uuid::Uuid::new_v4().to_string(),"identifier":bound.identifier}),
        );
        let prior = if let Some(previous) = previous.as_ref() {
            if let Some(mut state) = Envelope::read(previous)? {
                state.recover(start.timestamp_millis());
                state.proof
            } else {
                Some(super::otp_codec_state::legacy_proof(previous)?)
            }
        } else {
            None
        };
        let reserved = Envelope::new(Phase::Encoding, until, prior);
        reserved.write(&mut row)?;
        if previous.is_some() {
            tx.replace(Model::Verification, row.clone()).await?;
        } else {
            tx.insert(Model::Verification, row.clone()).await?;
        }
        final_clock_check(
            clock,
            start,
            until,
            expires.timestamp_millis(),
            authority_deadline(&bound)?,
        )?;
        tx.commit().await?;
        // Commit itself can wait on storage/network I/O; do not invoke a codec
        // after that wait has exhausted the persisted authority/lease.
        final_clock_check(
            clock,
            start,
            until,
            expires.timestamp_millis(),
            authority_deadline(&bound)?,
        )?;
        let result = codec.encode(&code).await;
        let mut tx = self.store.begin(true).await?;
        let (mut current, mut state) = owned(&mut tx, &row, &reserved.revision).await?;
        // Sample after acquiring the transaction and loading the reserved revision:
        // a wait behind another writer must not extend this callback lease.
        let now = clock();
        if state.phase != Phase::Encoding {
            return Err(stale());
        }
        if result.is_err() || now < start || now.timestamp_millis() >= until {
            if let Some(prior) = state.proof.take() {
                super::otp_codec_state::restore(&mut current, prior)?;
            } else {
                state.phase = Phase::Consumed;
                state.write(&mut current)?;
            }
            tx.replace(Model::Verification, current).await?;
            tx.commit().await?;
            return Err(failure());
        }
        let fresh = authority(&mut tx, token, challenge, now).await?;
        if !same_authority(&bound, &fresh) || now > expires {
            return Err(stale());
        }
        let encoded = result.map_err(|_| failure())?;
        let active = Envelope::new(
            Phase::Active,
            0,
            Some(Proof {
                value: encoded.as_str().to_owned(),
                attempts: 0,
                expires_ms: expires.timestamp_millis(),
                custom: true,
            }),
        );
        active.write(&mut current)?;
        tx.replace(Model::Verification, current).await?;
        final_clock_check(
            clock,
            start,
            until,
            expires.timestamp_millis(),
            authority_deadline(&fresh)?,
        )?;
        tx.commit().await?;
        self.deliver_otp(super::output_user(&fresh.user), code)
            .await
    }
    // Keep both transaction boundaries and the intervening callback visible together.
    #[allow(clippy::too_many_lines)]
    pub(super) async fn verify_custom_otp(
        &self,
        token: &str,
        challenge: &str,
        supplied: &str,
        trust: bool,
        clock: &Clock,
    ) -> BlueprintResult<FactorResponse> {
        if supplied.len() > 128 {
            return Err(error("INVALID_CODE", "Invalid code"));
        }
        let codec = self.otp_codec.as_ref().ok_or_else(failure)?;
        let mut tx = self.store.begin(true).await?;
        let start = clock();
        let until = deadline(start, codec)?;
        let bound = authority(&mut tx, token, challenge, start).await?;
        self.check_custom_lock(&mut tx, &bound, start).await?;
        let mut row = verification_row(&mut tx, &bound.identifier)
            .await?
            .ok_or_else(stale)?;
        let proof = if let Some(mut state) = Envelope::read(&row)? {
            state.recover(start.timestamp_millis());
            if state.phase != Phase::Active {
                return Err(stale());
            }
            state.proof.take().ok_or_else(failure)?
        } else {
            super::otp_codec_state::legacy_proof(&row)?
        };
        let reserved = Envelope::new(Phase::Verifying, until, None);
        reserved.write(&mut row)?;
        tx.replace(Model::Verification, row.clone()).await?;
        if proof.expires_ms < start.timestamp_millis() {
            tx.commit().await?;
            return Err(stale());
        }
        if proof.attempts >= u64::from(self.otp_options.allowed_attempts()) {
            tx.commit().await?;
            return Err(error(
                "TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE",
                "Too many attempts. Please request a new code.",
            ));
        }
        final_clock_check(
            clock,
            start,
            until,
            proof.expires_ms,
            authority_deadline(&bound)?,
        )?;
        tx.commit().await?;
        final_clock_check(
            clock,
            start,
            until,
            proof.expires_ms,
            authority_deadline(&bound)?,
        )?;
        let comparison = codec.compare(&proof.value, supplied).await;
        let mut tx = self.store.begin(true).await?;
        let (mut current, mut state) = owned(&mut tx, &row, &reserved.revision).await?;
        // Sample after acquiring the transaction and loading the reserved revision:
        // a wait behind another writer must not extend this callback lease.
        let now = clock();
        if state.phase != Phase::Verifying {
            return Err(stale());
        }
        state.phase = Phase::Consumed;
        state.write(&mut current)?;
        tx.replace(Model::Verification, current.clone()).await?;
        if comparison.is_err() {
            tx.commit().await?;
            return Err(failure());
        }
        if now < start
            || now.timestamp_millis() >= until
            || now.timestamp_millis() > proof.expires_ms
        {
            tx.commit().await?;
            return Err(stale());
        }
        let fresh = match authority(&mut tx, token, challenge, now).await {
            Ok(value) => value,
            Err(error) => {
                tx.commit().await?;
                return Err(error);
            }
        };
        if !same_authority(&bound, &fresh) {
            tx.commit().await?;
            return Err(stale());
        }
        self.check_custom_lock(&mut tx, &fresh, now).await?;
        let proof_expiry = proof.expires_ms;
        if !comparison.map_err(|_| failure())? {
            let mut proof = proof;
            proof.attempts += 1;
            super::otp_codec_state::restore(&mut current, proof)?;
            tx.replace(Model::Verification, current).await?;
            if fresh.challenge.is_some()
                && let Some(mut factor) = tx
                    .find_unique(Model::TwoFactor, "userId", text(&fresh.user, "id")?)
                    .await?
                && self.record_account_failure(&mut factor, now)?
            {
                tx.replace(Model::TwoFactor, factor).await?;
            }
            final_clock_check(
                clock,
                start,
                until,
                proof_expiry,
                authority_deadline(&fresh)?,
            )?;
            tx.commit().await?;
            return Err(error("INVALID_CODE", "Invalid code"));
        }
        if fresh.challenge.is_some()
            && self.account_lockout_options.enabled()
            && let Some(mut factor) = tx
                .find_unique(Model::TwoFactor, "userId", text(&fresh.user, "id")?)
                .await?
        {
            self.reset_account_failures(&mut factor);
            tx.replace(Model::TwoFactor, factor).await?;
        }
        let authority_expiry = authority_deadline(&fresh)?;
        let response = self.complete_otp(&mut tx, fresh, trust, now).await?;
        final_clock_check(clock, start, until, proof_expiry, authority_expiry)?;
        tx.commit().await?;
        Ok(response)
    }
    async fn check_custom_lock<T: AuthTransaction>(
        &self,
        tx: &mut T,
        bound: &Authority,
        now: DateTime<Utc>,
    ) -> BlueprintResult<()> {
        if bound.challenge.is_some()
            && let Some(mut factor) = tx
                .find_unique(Model::TwoFactor, "userId", text(&bound.user, "id")?)
                .await?
            && self.check_account_lock(&mut factor, now)?
        {
            tx.replace(Model::TwoFactor, factor).await?;
        }
        Ok(())
    }
}

/// One bounded maintenance result. Continue with the opaque cursor until absent.
pub struct OtpCodecCleanup {
    pub inspected: usize,
    pub removed: usize,
    pub next: Option<crate::store::AuthCursor>,
}
impl<S: AuthStore> TwoFactorService<S> {
    /// Visits at most 256 tenant verification records. No scheduler is installed.
    /// Expired encoding leases recover a still-live prior proof; other expired
    /// custom records are deleted. Default no-codec instances may run maintenance.
    /// # Errors
    /// Storage failures roll back the whole page; malformed recognized states fail closed.
    pub async fn cleanup_otp_codec_states(
        &self,
        after: Option<crate::store::AuthCursor>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<OtpCodecCleanup> {
        let mut tx = self.store.begin(true).await?;
        let page = tx.list_page(Model::Verification, after, 256).await?;
        let mut removed = 0;
        for mut row in page.records.iter().cloned() {
            if !row["identifier"]
                .as_str()
                .is_some_and(|value| value.starts_with("2fa-otp-"))
            {
                continue;
            }
            let Some(mut state) = Envelope::read(&row)? else {
                continue;
            };
            if state.lease_until_ms > now.timestamp_millis() {
                continue;
            }
            state.recover(now.timestamp_millis());
            if state.phase == Phase::Active
                && state
                    .proof
                    .as_ref()
                    .is_some_and(|proof| proof.expires_ms >= now.timestamp_millis())
            {
                let proof = state.proof.take().ok_or_else(failure)?;
                super::otp_codec_state::restore(&mut row, proof)?;
                tx.replace(Model::Verification, row).await?;
            } else {
                tx.delete(Model::Verification, text(&row, "id")?).await?;
                removed += 1;
            }
        }
        tx.commit().await?;
        Ok(OtpCodecCleanup {
            inspected: page.records.len(),
            removed,
            next: page.next,
        })
    }
}
