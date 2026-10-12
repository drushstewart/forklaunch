//! Persistent key selection and JWKS lifecycle for the configured JWT plugin.
use crate::{
    jwt::SigningKey,
    store::{AuthStore, AuthTransaction, Model},
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};
use std::{cmp::Reverse, sync::Arc};
use zeroize::Zeroizing;

mod sign;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod verify;

/// All private key records pass through the store's encrypted persistence.
/// Production uses `PostgresAuthStore`; memory stores are reference/test backends.
pub struct KeyRing<S> {
    store: Arc<S>,
    base_url: String,
}
impl<S: AuthStore> KeyRing<S> {
    #[must_use]
    pub fn new(store: Arc<S>, base_url: impl Into<String>) -> Self {
        Self {
            store,
            base_url: base_url.into(),
        }
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
        let issued_at = u64::try_from(now.timestamp()).map_err(|_| key_error())?;
        let mut tx = self.store.begin(true).await?;
        let rows = tx.list(Model::Jwks).await?;
        let mut candidates = Vec::new();
        for row in rows.iter().take(100) {
            if optional_time(row, "expiresAt")?.is_none_or(|expires| expires > now) {
                candidates.push((required_time(row, "createdAt")?, row));
            }
        }
        // Stable sorting preserves the actual adapter's first-insertion tie rule.
        candidates.sort_by_key(|(created, _)| Reverse(*created));
        let selected = candidates
            .iter()
            .find(|(_, row)| row["alg"] == "EdDSA" || row["alg"].is_null())
            .or_else(|| candidates.first());
        let key = if let Some((_, row)) = selected {
            restore(row)?
        } else {
            mint(&mut tx, now).await?
        };
        let token = key.issue(&self.base_url, user_id, email, organization, issued_at)?;
        tx.commit().await?;
        Ok(token)
    }

    /// Publishes live keys and expired keys still within the actual default
    /// 30-day grace period. An empty store initializes one key atomically.
    /// An existing store containing only retired keys may publish an empty set,
    /// matching Better Auth; token issuance then provisions a new live key.
    /// # Errors
    /// Returns sanitized storage or corrupted key metadata failures.
    pub async fn jwks(&self, now: DateTime<Utc>) -> BlueprintResult<Value> {
        let mut tx = self.store.begin(true).await?;
        let mut rows = tx.list(Model::Jwks).await?;
        if rows.is_empty() {
            mint(&mut tx, now).await?;
            rows = tx.list(Model::Jwks).await?;
        }
        let mut keys = Vec::new();
        for row in rows {
            let visible = match optional_time(&row, "expiresAt")? {
                None => true,
                Some(expires) => {
                    expires
                        .checked_add_signed(Duration::days(30))
                        .ok_or_else(key_error)?
                        > now
                }
            };
            if visible {
                keys.push(public_key(&row)?);
            }
        }
        tx.commit().await?;
        Ok(json!({"keys":keys}))
    }

    /// Operator-controlled rotation. Retired keys remain public for the default
    /// grace period; subsequent tokens select the new key. No public endpoint
    /// exposes this administrative operation.
    /// # Errors
    /// Returns sanitized storage or corrupted key metadata failures.
    pub async fn rotate(&self, now: DateTime<Utc>) -> BlueprintResult<Value> {
        let mut tx = self.store.begin(true).await?;
        for mut row in tx.list(Model::Jwks).await? {
            if optional_time(&row, "expiresAt")?.is_none_or(|expires| expires > now) {
                row["expiresAt"] = json!(timestamp(now));
                tx.replace(Model::Jwks, row).await?;
            }
        }
        let key = mint(&mut tx, now).await?;
        tx.commit().await?;
        Ok(key.public_jwk())
    }
}

async fn mint<T: AuthTransaction>(tx: &mut T, now: DateTime<Utc>) -> BlueprintResult<SigningKey> {
    mint_profile(tx, now, "EdDSA").await
}
async fn mint_profile<T: AuthTransaction>(
    tx: &mut T,
    now: DateTime<Utc>,
    algorithm: &str,
) -> BlueprintResult<SigningKey> {
    mint_config(tx, now, algorithm, 2048).await
}
async fn mint_config<T: AuthTransaction>(
    tx: &mut T,
    now: DateTime<Utc>,
    algorithm: &str,
    bits: usize,
) -> BlueprintResult<SigningKey> {
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
    tx.insert(Model::Jwks, row).await?;
    Ok(key)
}
fn restore(row: &Value) -> BlueprintResult<SigningKey> {
    let encoded = row["privateKey"].as_str().ok_or_else(key_error)?;
    if encoded.len() > 8192 {
        return Err(key_error());
    }
    let encoded = Zeroizing::new(STANDARD.decode(encoded).map_err(|_| key_error())?);
    let key = SigningKey::from_pkcs8_profile(
        row["id"].as_str().ok_or_else(key_error)?,
        &encoded,
        row["alg"].as_str().unwrap_or("EdDSA"),
    )?;
    if key.public_jwk() != public_key(row)? {
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
