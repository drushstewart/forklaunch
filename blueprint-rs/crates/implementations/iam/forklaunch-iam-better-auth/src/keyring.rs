//! Persistent key selection and JWKS lifecycle for the configured JWT plugin.
use crate::{
    jwt::SigningKey,
    store::{AuthStore, AuthTransaction, Model},
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use chrono::{DateTime, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};
use std::sync::Arc;
use zeroize::Zeroizing;

pub mod adapter;
mod config;
pub mod custom;
pub use config::KeyPairConfig;
mod repository;
mod sign;
#[cfg(test)]
use repository::KeyRepository;
use repository::{NewKeyRecord, session::KeySession};
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod verify;

/// Default private key records use the tenant store's encrypted persistence.
/// Custom callbacks explicitly own their key persistence and encryption.
pub struct KeyRing<S> {
    store: Arc<S>,
    base_url: String,
    grace_millis: i64,
    key_config: KeyPairConfig,
    adapter: Option<adapter::JwksAdapter>,
}
impl<S: AuthStore> KeyRing<S> {
    #[must_use]
    pub fn new(store: Arc<S>, base_url: impl Into<String>) -> Self {
        Self {
            store,
            base_url: base_url.into(),
            grace_millis: 2_592_000_000,
            adapter: None,
            key_config: KeyPairConfig::default(),
        }
    }

    /// Configures local issuance, rotation and legacy missing-algorithm inheritance.
    #[must_use]
    pub fn with_key_pair_config(mut self, config: KeyPairConfig) -> Self {
        self.key_config = config;
        self
    }
    /// Uses trusted tenant-bound repository callbacks for all local key operations.
    /// Custom callbacks own persistence; default/custom stores are not one transaction.
    #[must_use]
    pub fn with_jwks_adapter(mut self, adapter: adapter::JwksAdapter) -> Self {
        self.adapter = Some(adapter);
        self
    }
    fn repository(&self, write: bool, now: DateTime<Utc>) -> KeySession<'_, S> {
        KeySession::new(
            self.store.as_ref(),
            write,
            self.adapter.as_ref(),
            &self.base_url,
            now,
        )
    }
    /// Sets the publication grace period for expired public keys. This does
    /// not permit expired private keys to issue new tokens.
    /// # Errors
    /// Rejects sub-millisecond precision or durations exceeding signed milliseconds.
    pub fn with_jwks_grace_period(mut self, period: std::time::Duration) -> BlueprintResult<Self> {
        self.grace_millis = grace_millis(period)?;
        Ok(self)
    }

    /// Issues claims from an authenticated persisted session. Callers supply
    /// trusted identity and server time, never unchecked request values.
    /// # Errors
    /// Returns sanitized storage, corrupted-key or timestamp failures.
    pub async fn issue(
        &self,
        user_id: &str,
        email: &str,
        organization: Option<&str>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<String> {
        self.issue_session(user_id, email, organization, None, now)
            .await
    }
    /// Issue claims for an exact authenticated persisted session.
    /// # Errors
    /// Returns sanitized storage, key or timestamp failures.
    pub async fn issue_session(
        &self,
        user_id: &str,
        email: &str,
        organization: Option<&str>,
        session_id: Option<&str>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<String> {
        let issued_at = u64::try_from(now.timestamp()).map_err(|_| key_error())?;
        let key_expiration = self.key_config.expiration(now)?;
        let mut tx = self.repository(true, now);
        let selected = tx.select(self.key_config.algorithm()).await?;
        let key = if let Some(row) = selected {
            row.signing_key_with_algorithm(self.key_config.algorithm())?
        } else {
            mint_session(
                &mut tx,
                now,
                self.key_config.algorithm(),
                self.key_config.rsa_bits(),
                key_expiration,
            )
            .await?
        };
        let token = key.issue_session(
            &self.base_url,
            user_id,
            email,
            organization,
            session_id,
            issued_at,
        )?;
        tx.commit().await?;
        Ok(token)
    }

    /// Publishes live keys and expired keys still within the actual default
    /// configured grace period (30 days by default), within the configured adapter's first 100 records.
    /// An empty store initializes one key atomically.
    /// An existing store containing only retired keys may publish an empty set,
    /// matching Better Auth; token issuance then provisions a new live key.
    /// # Errors
    /// Returns sanitized storage or corrupted key metadata failures.
    pub async fn jwks(&self, now: DateTime<Utc>) -> BlueprintResult<Value> {
        let key_expiration = self.key_config.expiration(now)?;
        let mut tx = self.repository(true, now);
        let mut rows = tx.read().await?;
        if rows.is_empty() {
            mint_session(
                &mut tx,
                now,
                self.key_config.algorithm(),
                self.key_config.rsa_bits(),
                key_expiration,
            )
            .await?;
            rows = tx.read().await?;
        }
        if rows.is_empty() {
            return Err(key_error());
        }
        let mut keys = Vec::new();
        for row in rows {
            let visible = match row.expires_at()? {
                None => true,
                Some(expires) => {
                    expires
                        .timestamp_millis()
                        .checked_add(self.grace_millis)
                        .ok_or_else(key_error)?
                        > now.timestamp_millis()
                }
            };
            if visible {
                keys.push(row.public_key_with_algorithm(self.key_config.algorithm())?);
            }
        }
        tx.commit().await?;
        Ok(json!({"keys":keys}))
    }

    /// Operator-controlled rotation. Retired keys remain public for the default
    /// grace period. Public key selection retains the configured first-100 limit;
    /// operators must prune old records before that window fills. No public endpoint
    /// exposes this administrative operation. Custom repositories must rotate their
    /// own records; this operation rejects any active custom hook.
    /// # Errors
    /// Returns sanitized storage or corrupted key metadata failures.
    pub async fn rotate(&self, now: DateTime<Utc>) -> BlueprintResult<Value> {
        let key_expiration = self.key_config.expiration(now)?;
        let mut tx = self.repository(true, now);
        tx.retire().await?;
        let key = mint_session(
            &mut tx,
            now,
            self.key_config.algorithm(),
            self.key_config.rsa_bits(),
            key_expiration,
        )
        .await?;
        tx.commit().await?;
        Ok(key.public_jwk())
    }
}

#[cfg(test)]
async fn mint<T: AuthTransaction>(tx: &mut T, now: DateTime<Utc>) -> BlueprintResult<SigningKey> {
    mint_profile(tx, now, "EdDSA").await
}
#[cfg(test)]
async fn mint_profile<T: AuthTransaction>(
    tx: &mut T,
    now: DateTime<Utc>,
    algorithm: &str,
) -> BlueprintResult<SigningKey> {
    mint_config(tx, now, algorithm, 2048, None).await
}
#[cfg(test)]
async fn mint_config<T: AuthTransaction>(
    tx: &mut T,
    now: DateTime<Utc>,
    algorithm: &str,
    bits: usize,
    expires_at: Option<DateTime<Utc>>,
) -> BlueprintResult<SigningKey> {
    KeyRepository::new(tx)
        .create(generate_key_record(now, algorithm, bits, expires_at).await?)
        .await?
        .signing_key()
}
async fn mint_session<S: AuthStore>(
    session: &mut KeySession<'_, S>,
    now: DateTime<Utc>,
    algorithm: &str,
    bits: usize,
    expires_at: Option<DateTime<Utc>>,
) -> BlueprintResult<SigningKey> {
    session
        .create(generate_key_record(now, algorithm, bits, expires_at).await?)
        .await?
        .signing_key()
}
async fn generate_key_record(
    now: DateTime<Utc>,
    algorithm: &str,
    bits: usize,
    expires_at: Option<DateTime<Utc>>,
) -> BlueprintResult<NewKeyRecord> {
    let id = uuid::Uuid::new_v4().to_string();
    let key_id = id.clone();
    let key_algorithm = algorithm.to_string();
    // RSA prime generation must not block an async HTTP/MCP executor thread.
    let key = tokio::task::spawn_blocking(move || {
        SigningKey::generate_config(key_id, &key_algorithm, bits)
    })
    .await
    .map_err(|_| key_error())??;
    let mut row = json!({"id":id,"alg":algorithm,"createdAt":timestamp(now),
        "publicKey":serde_json::to_string(&key.public_jwk()).map_err(|_| key_error())?,
        "privateKey":STANDARD.encode(key.private_pkcs8())});
    if let Some(curve) = key.public_jwk().get("crv") {
        row["crv"] = curve.clone();
    }
    if let Some(expires) = expires_at {
        row["expiresAt"] = json!(timestamp(expires));
    }
    Ok(NewKeyRecord(row))
}
fn restore(row: &Value) -> BlueprintResult<SigningKey> {
    restore_with_algorithm(row, "EdDSA")
}
fn restore_with_algorithm(row: &Value, fallback: &str) -> BlueprintResult<SigningKey> {
    let encoded = row["privateKey"].as_str().ok_or_else(key_error)?;
    if encoded.len() > 8192 {
        return Err(key_error());
    }
    let encoded = Zeroizing::new(STANDARD.decode(encoded).map_err(|_| key_error())?);
    let key = SigningKey::from_pkcs8_profile(
        row["id"].as_str().ok_or_else(key_error)?,
        &encoded,
        config::algorithm(row, fallback)?,
    )?;
    if key.public_jwk() != config::public(row, fallback)? {
        return Err(key_error());
    }
    Ok(key)
}
fn public_key(row: &Value) -> BlueprintResult<Value> {
    let key: Value = serde_json::from_str(row["publicKey"].as_str().ok_or_else(key_error)?)
        .map_err(|_| key_error())?;
    let algorithm = row["alg"].as_str().unwrap_or("EdDSA");
    if matches!(
        algorithm,
        "RS256" | "RS384" | "RS512" | "PS256" | "PS384" | "PS512"
    ) {
        if key["kid"] != row["id"]
            || key["alg"] != algorithm
            || key["kty"] != "RSA"
            || key.as_object().is_none_or(|key| key.len() != 5)
            || ["n", "e"].iter().any(|field| {
                key[field]
                    .as_str()
                    .is_none_or(|value| URL_SAFE_NO_PAD.decode(value).is_err() || value.is_empty())
            })
        {
            return Err(key_error());
        }
        return Ok(key);
    }
    let (kind, curve, width, fields) = match algorithm {
        "EdDSA" | "Ed25519" => ("OKP", "Ed25519", 32, 5),
        "ES256" => ("EC", "P-256", 32, 6),
        "ES384" => ("EC", "P-384", 48, 6),
        "ES512" => ("EC", "P-521", 66, 6),
        _ => return Err(key_error()),
    };
    let coordinate = |field: &str| {
        key[field].as_str().is_some_and(|value| {
            URL_SAFE_NO_PAD
                .decode(value)
                .is_ok_and(|bytes| bytes.len() == width)
        })
    };
    if key["kid"] != row["id"]
        || key["alg"] != algorithm
        || key["kty"] != kind
        || key["crv"] != curve
        || row["crv"] != curve
        || !coordinate("x")
        || kind == "EC" && !coordinate("y")
        || key.as_object().is_none_or(|key| key.len() != fields)
    {
        return Err(key_error());
    }
    Ok(key)
}
fn timestamp(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn optional_time(row: &Value, field: &str) -> BlueprintResult<Option<DateTime<Utc>>> {
    match row.get(field).filter(|value| !value.is_null()) {
        None => Ok(None),
        Some(value) => DateTime::parse_from_rfc3339(value.as_str().ok_or_else(key_error)?)
            .map(|date| Some(date.with_timezone(&Utc)))
            .map_err(|_| key_error()),
    }
}
fn required_time(row: &Value, field: &str) -> BlueprintResult<DateTime<Utc>> {
    optional_time(row, field)?.ok_or_else(key_error)
}
fn key_error() -> BlueprintError {
    BlueprintError::new(
        "better_auth_key_error",
        "Unable to use authentication signing key",
    )
}

// Validate once before any storage access; retain exact whole milliseconds.
pub(crate) fn grace_millis(period: std::time::Duration) -> BlueprintResult<i64> {
    if !period.subsec_nanos().is_multiple_of(1_000_000) {
        return Err(BlueprintError::new(
            "better_auth_invalid_config",
            "JWKS grace period must use whole milliseconds",
        ));
    }
    i64::try_from(period.as_millis()).map_err(|_| {
        BlueprintError::new(
            "better_auth_invalid_config",
            "JWKS grace period exceeds supported milliseconds",
        )
    })
}
