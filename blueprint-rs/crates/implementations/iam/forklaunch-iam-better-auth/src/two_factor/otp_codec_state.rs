//! Private encrypted-record envelope. Never accepted from an HTTP request.
use super::{BlueprintResult, Value, corrupt, json, text, time};
use serde::{Deserialize, Serialize};

pub(super) const FIELD: &str = "otpCodecState";
const SENTINEL: &str = "!forklaunch-otp-codec-v1!";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Proof {
    pub value: String,
    pub attempts: u64,
    pub expires_ms: i64,
    pub custom: bool,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Phase {
    Active,
    Encoding,
    Verifying,
    Consumed,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Envelope {
    pub version: u8,
    pub revision: String,
    pub phase: Phase,
    pub lease_until_ms: i64,
    pub proof: Option<Proof>,
}
impl Envelope {
    pub fn new(phase: Phase, lease_until_ms: i64, proof: Option<Proof>) -> Self {
        Self {
            version: 1,
            revision: uuid::Uuid::new_v4().to_string(),
            phase,
            lease_until_ms,
            proof,
        }
    }
    pub fn read(row: &Value) -> BlueprintResult<Option<Self>> {
        let Some(value) = row.get(FIELD) else {
            return Ok(None);
        };
        let state: Self = serde_json::from_value(value.clone()).map_err(|_| corrupt())?;
        if state.version != 1
            || uuid::Uuid::parse_str(&state.revision).is_err()
            || text(row, "value")? != SENTINEL
            || matches!(state.phase, Phase::Active) && state.proof.is_none()
            || matches!(state.phase, Phase::Verifying | Phase::Consumed) && state.proof.is_some()
            || state
                .proof
                .as_ref()
                .is_some_and(|proof| proof.value.len() > 1024 || proof.attempts > 100)
        {
            return Err(corrupt());
        }
        Ok(Some(state))
    }
    pub fn write(&self, row: &mut Value) -> BlueprintResult<()> {
        row["value"] = json!(SENTINEL);
        row["attempts"] = json!(0);
        let expiry = self.proof.as_ref().map_or(self.lease_until_ms, |proof| {
            if self.phase == Phase::Active {
                proof.expires_ms
            } else {
                proof.expires_ms.max(self.lease_until_ms)
            }
        });
        row["expiresAt"] = json!(super::timestamp(
            chrono::DateTime::from_timestamp_millis(expiry).ok_or_else(corrupt)?
        ));
        row[FIELD] = serde_json::to_value(self).map_err(|_| corrupt())?;
        Ok(())
    }
    pub fn recover(&mut self, now_ms: i64) {
        if self.lease_until_ms > now_ms {
            return;
        }
        if self.phase == Phase::Encoding {
            if self
                .proof
                .as_ref()
                .is_some_and(|proof| proof.expires_ms >= now_ms)
            {
                self.phase = Phase::Active;
            } else {
                self.phase = Phase::Consumed;
                self.proof = None;
            }
        } else if self.phase == Phase::Verifying {
            self.phase = Phase::Consumed;
        }
    }
}
pub(super) fn legacy_proof(row: &Value) -> BlueprintResult<Proof> {
    let attempts = row["attempts"].as_u64().ok_or_else(corrupt)?;
    let value = text(row, "value")?;
    if attempts > 100 || value.len() > 1024 {
        return Err(corrupt());
    }
    Ok(Proof {
        value: value.to_owned(),
        attempts,
        expires_ms: time(row, "expiresAt")?.timestamp_millis(),
        custom: false,
    })
}
pub(super) fn restore(row: &mut Value, proof: Proof) -> BlueprintResult<()> {
    if proof.custom {
        Envelope::new(Phase::Active, 0, Some(proof)).write(row)
    } else {
        row.as_object_mut().ok_or_else(corrupt)?.remove(FIELD);
        row["value"] = json!(proof.value);
        row["attempts"] = json!(proof.attempts);
        row["expiresAt"] = json!(super::timestamp(
            chrono::DateTime::from_timestamp_millis(proof.expires_ms).ok_or_else(corrupt)?
        ));
        Ok(())
    }
}
