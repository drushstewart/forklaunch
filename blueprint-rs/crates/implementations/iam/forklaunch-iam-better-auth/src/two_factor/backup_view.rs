//! Trusted server-only disclosure. No routes, proof consumption, or read-time migration.
use super::{
    AuthStore, AuthTransaction, BackupCodeCodecText, BlueprintResult, DateTime, Duration, Model,
    TwoFactorService, Utc, Value, backup_async, backup_codec, backup_codec_state as state,
    backup_storage, error, json, verification_row,
};
use std::time::Instant;
use zeroize::Zeroizing;

/// Sensitive server-only result. No Debug/Serialize; callers must authorize the
/// requested user and must not log the returned plaintext. Owned strings zeroize
/// on drop. This does not provide post-return revocation of a disclosed proof.
pub struct ViewedBackupCodes(Vec<Zeroizing<String>>);
impl ViewedBackupCodes {
    /// Borrows the plaintext without adding an implicit serialization surface.
    #[must_use]
    pub fn codes(&self) -> impl ExactSizeIterator<Item = &str> {
        self.0.iter().map(|value| value.as_str())
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    fn new(codes: Vec<String>) -> Self {
        Self(codes.into_iter().map(Zeroizing::new).collect())
    }
}
fn invalid() -> super::BlueprintError {
    error("INVALID_BACKUP_CODE", "Invalid backup code")
}
fn decode(value: &Value) -> BlueprintResult<Vec<String>> {
    if value.is_null()
        || value == &json!(false)
        || value.as_str() == Some("")
        || value.as_f64().is_some_and(|number| number == 0.0)
    {
        return Err(invalid());
    }
    backup_storage::codes(value)
}
pub(super) async fn recheck<T: AuthTransaction>(
    tx: &mut T,
    user_id: &str,
    generation: &str,
) -> BlueprintResult<()> {
    snapshot(tx, user_id).await.map_err(|error| {
        if error.code == "BACKUP_CODES_NOT_ENABLED" {
            backup_async::stale()
        } else {
            error
        }
    })?;
    // Re-read after the lease query for READ COMMITTED backends. A mutation after
    // this final read remains an ordinary post-read race, not revocation support.
    let row = tx
        .find_unique(Model::TwoFactor, "userId", user_id)
        .await?
        .ok_or_else(backup_async::stale)?;
    if state::factor_digest(&row)? != generation {
        return Err(backup_async::stale());
    }
    Ok(())
}
async fn snapshot<T: AuthTransaction>(tx: &mut T, user_id: &str) -> BlueprintResult<Value> {
    let row = tx
        .find_unique(Model::TwoFactor, "userId", user_id)
        .await?
        .ok_or_else(|| error("BACKUP_CODES_NOT_ENABLED", "Backup codes are not enabled"))?;
    // Even an expired reservation requires explicit recovery, never read-side cleanup.
    if verification_row(tx, &state::identifier(user_id))
        .await?
        .is_some()
    {
        return Err(backup_async::stale());
    }
    Ok(row)
}
impl<S: AuthStore> TwoFactorService<S> {
    /// Discloses current backup codes to trusted server code. The caller must
    /// authorize `user_id` (normally obtained from a verified session); this method
    /// does not authenticate it or require a User/session/password record.
    /// No HTTP/MCP/client endpoint is registered. No state is mutated.
    /// # Errors
    /// Rejects missing factors, incompatible codecs, invalid decoded arrays,
    /// active/expired reservations, and changes during asynchronous decoding.
    pub async fn view_backup_codes_for_user(
        &self,
        user_id: &str,
    ) -> BlueprintResult<ViewedBackupCodes> {
        self.view_backup_codes_for_user_with_clock(user_id, &Utc::now)
            .await
    }
    /// Trusted clock variant. Callback timeouts are cooperative and do not stop
    /// blocking/non-yielding code. Final checks cannot revoke already returned data.
    /// # Errors
    /// Has the same failures as `view_backup_codes_for_user`, plus backwards or
    /// expired callback clocks. The fixed tenant store supplies tenant isolation.
    pub async fn view_backup_codes_for_user_with_clock(
        &self,
        user_id: &str,
        clock: &(dyn Fn() -> DateTime<Utc> + Send + Sync),
    ) -> BlueprintResult<ViewedBackupCodes> {
        let mut tx = self.store.begin(false).await?;
        let started = clock();
        let elapsed = Instant::now();
        let row = snapshot(&mut tx, user_id).await?;
        let generation = state::factor_digest(&row)?;
        let Some(adapter) = &self.backup_codec else {
            let codes = decode(&self.read_backup_value(&row)?.ok_or_else(invalid)?)?;
            recheck(&mut tx, user_id, &generation).await?;
            tx.commit().await?;
            return Ok(ViewedBackupCodes::new(codes));
        };
        if row.get("backupCodesFormat").is_none() && row["backupCodes"].is_array() {
            let codes = backup_storage::codes(&row["backupCodes"])?;
            // No callback is polled for legacy arrays; no callback deadline applies.
            recheck(&mut tx, user_id, &generation).await?;
            tx.commit().await?;
            return Ok(ViewedBackupCodes::new(codes));
        }
        if row["backupCodesFormat"] != json!(1) && row["backupCodesFormat"] != json!(2) {
            return Err(backup_codec::failure());
        }
        let input = BackupCodeCodecText::new(
            row["backupCodes"]
                .as_str()
                .ok_or_else(backup_codec::failure)?
                .to_owned(),
        )?;
        let timeout = adapter.options.timeout();
        let deadline = started
            .checked_add_signed(Duration::from_std(timeout).map_err(|_| backup_codec::failure())?)
            .ok_or_else(backup_codec::failure)?;
        tx.commit().await?;
        let check = || {
            let now = clock();
            if now < started || now >= deadline || elapsed.elapsed() >= timeout {
                Err(backup_async::stale())
            } else {
                Ok(())
            }
        };
        check()?;
        let remaining = timeout
            .checked_sub(elapsed.elapsed())
            .ok_or_else(backup_codec::failure)?;
        let decoded = tokio::time::timeout(remaining, adapter.hook.decrypt(input))
            .await
            .map_err(|_| backup_codec::failure())?
            .map_err(|_| backup_codec::failure())?;
        if decoded.as_str().len() > backup_storage::MAX_JSON {
            return Err(backup_codec::failure());
        }
        let value = serde_json::from_str::<Value>(decoded.as_str()).map_err(|_| invalid())?;
        let codes = ViewedBackupCodes::new(decode(&value)?);
        check()?;
        let mut tx = self.store.begin(false).await?;
        recheck(&mut tx, user_id, &generation).await?;
        check()?;
        tx.commit().await?;
        check()?;
        Ok(codes)
    }
}
