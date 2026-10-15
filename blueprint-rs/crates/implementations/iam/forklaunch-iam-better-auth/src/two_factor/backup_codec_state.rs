//! Durable leases contain no plaintext backup codes or bearer tokens.
use super::{
    AuthTransaction, BlueprintResult, DateTime, Model, Utc, Value, corrupt, json, text, timestamp,
    verification_row,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
pub(super) const PREFIX: &str = "2fa-backup-codec-";
const FIELD: &str = "backupCodecLease";
const SENTINEL: &str = "!forklaunch-backup-codec-v1!";
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Phase {
    EnrollEncoding,
    RegenerateEncoding,
    Decoding,
    Reencoding,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Lease {
    pub version: u8,
    pub owner: String,
    pub phase: Phase,
    pub deadline: i64,
    pub started: i64,
    pub user_id: String,
    pub factor: Option<String>,
    pub authority: String,
    pub challenge: Option<(String, String)>,
}
pub(super) fn digest(value: &Value) -> BlueprintResult<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| corrupt())?;
    Ok(URL_SAFE_NO_PAD.encode(ring::digest::digest(&ring::digest::SHA256, &bytes).as_ref()))
}
pub(super) fn identifier(user: &str) -> String {
    format!(
        "{PREFIX}{}",
        URL_SAFE_NO_PAD
            .encode(ring::digest::digest(&ring::digest::SHA256, user.as_bytes()).as_ref())
    )
}
pub(super) fn factor_digest(row: &Value) -> BlueprintResult<String> {
    digest(&json!([
        row["id"],
        row["userId"],
        row["secret"],
        row["verified"],
        row["backupCodes"],
        row["backupCodesFormat"],
        row["backupCodesRevision"]
    ]))
}
impl Lease {
    pub fn read(row: &Value) -> BlueprintResult<Self> {
        let state: Self = serde_json::from_value(row[FIELD].clone()).map_err(|_| corrupt())?;
        if state.version != 1
            || uuid::Uuid::parse_str(&state.owner).is_err()
            || row["value"] != SENTINEL
            || row["identifier"] != identifier(&state.user_id)
            || state
                .deadline
                .checked_sub(state.started)
                .is_none_or(|ms| !(1..=30_000).contains(&ms))
            || state.authority.len() != 43
            || state.factor.as_ref().is_some_and(|hash| hash.len() != 43)
            || state
                .challenge
                .as_ref()
                .is_some_and(|(_, hash)| hash.len() != 43)
        {
            return Err(corrupt());
        }
        Ok(state)
    }
    pub fn write(&self, row: &mut Value) -> BlueprintResult<()> {
        row[FIELD] = serde_json::to_value(self).map_err(|_| corrupt())?;
        row["value"] = json!(SENTINEL);
        row["expiresAt"] = json!(timestamp(
            DateTime::from_timestamp_millis(self.deadline).ok_or_else(corrupt)?
        ));
        Ok(())
    }
    pub fn row(&self, now: DateTime<Utc>) -> BlueprintResult<Value> {
        let mut row = json!({"id":uuid::Uuid::new_v4().to_string(),"identifier":identifier(&self.user_id),"createdAt":timestamp(now),"updatedAt":timestamp(now)});
        self.write(&mut row)?;
        Ok(row)
    }
}
pub(super) async fn consume_matching_challenge<T: AuthTransaction>(
    tx: &mut T,
    lease: &Lease,
) -> BlueprintResult<()> {
    if let Some((id, hash)) = &lease.challenge
        && let Some(row) = tx.get(Model::Verification, id).await?
        && digest(&row)? == *hash
    {
        tx.delete(Model::Verification, id).await?;
    }
    Ok(())
}
pub(super) async fn recover<T: AuthTransaction>(
    tx: &mut T,
    row: &Value,
    lease: &Lease,
) -> BlueprintResult<()> {
    if lease.phase == Phase::Reencoding {
        consume_matching_challenge(tx, lease).await?;
    }
    tx.delete(Model::Verification, text(row, "id")?).await?;
    Ok(())
}
pub(super) async fn current<T: AuthTransaction>(
    tx: &mut T,
    lease: &Lease,
) -> BlueprintResult<Value> {
    let row = verification_row(tx, &identifier(&lease.user_id))
        .await?
        .ok_or_else(super::backup_async::stale)?;
    if Lease::read(&row)?.owner != lease.owner {
        return Err(super::backup_async::stale());
    }
    Ok(row)
}

/// Generic verification cleanup must not discard a reencoding lease without its
/// conditional attempt cleanup. Return true for recognized reserved rows.
pub(crate) async fn cleanup_backup_lease<T: AuthTransaction>(
    tx: &mut T,
    row: &Value,
    now: DateTime<Utc>,
) -> BlueprintResult<bool> {
    if row.get(FIELD).is_none()
        && !row["identifier"]
            .as_str()
            .is_some_and(|s| s.starts_with(PREFIX))
    {
        return Ok(false);
    }
    let state = Lease::read(row)?;
    if state.deadline <= now.timestamp_millis() {
        recover(tx, row, &state).await?;
    }
    Ok(true)
}
