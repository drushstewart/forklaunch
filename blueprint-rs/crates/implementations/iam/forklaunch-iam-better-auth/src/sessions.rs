//! Configured persistent session lifetime, refresh, freshness and revocation.
use crate::store::{AuthStore, AuthTransaction, Model};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use rand_core::{OsRng, RngCore};
use serde_json::{Value, json};
use std::sync::Arc;

#[cfg(test)]
pub(crate) mod tests;

pub struct SessionService<S> {
    store: Arc<S>,
}

/// A validated persistent session. Cookie signature verification is the HTTP
/// adapter's responsibility; this service verifies authoritative stored state.
pub struct SessionView {
    pub session: Value,
    pub user: Value,
    pub refreshed: bool,
}
impl SessionView {
    #[must_use]
    pub fn body(&self) -> Value {
        json!({"session":self.session,"user":self.user})
    }
}

impl<S: AuthStore> SessionService<S> {
    #[must_use]
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }

    /// Creates a session for an existing trusted user, with an opaque random
    /// token and a 24-hour lifetime. Signup must use the same record builder within
    /// its user/account transaction to preserve atomicity.
    /// # Errors
    /// Rejects unknown users or storage/random-source/timestamp failures.
    pub async fn create(
        &self,
        user_id: &str,
        ip_address: &str,
        user_agent: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let mut tx = self.store.begin(true).await?;
        if tx.get(Model::User, user_id).await?.is_none() {
            return Err(unauthorized());
        }
        let session = new_session(user_id, ip_address, user_agent, now)?;
        tx.insert(Model::Session, session.clone()).await?;
        tx.commit().await?;
        Ok(session)
    }

    /// Reads persistent state, removes expired sessions and performs hourly
    /// refresh unless transient cookies or explicit query flags suppress it.
    /// Missing/revoked sessions return None. Token and creation time stay stable.
    /// # Errors
    /// Returns sanitized storage or corrupt-record/timestamp failures.
    pub async fn get(
        &self,
        token: &str,
        transient: bool,
        disable_refresh: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Option<SessionView>> {
        let mut tx = self.store.begin(true).await?;
        let result = load(&mut tx, token, transient || disable_refresh, now).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Lists only this user's active sessions. Creation-based freshness is
    /// required even after refresh extends the authoritative session's expiry.
    /// # Errors
    /// Returns `UNAUTHORIZED`/`SESSION_NOT_FRESH` or sanitized storage errors.
    pub async fn list(
        &self,
        token: &str,
        transient: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let mut tx = self.store.begin(true).await?;
        let Some(current) = load(&mut tx, token, transient, now).await? else {
            tx.commit().await?;
            return Err(unauthorized());
        };
        let created = field_time(&current.session, "createdAt")?;
        if now.signed_duration_since(created) >= Duration::days(1) {
            // The real middleware persists refresh before rejecting freshness.
            tx.commit().await?;
            return Err(BlueprintError::new(
                "SESSION_NOT_FRESH",
                "Session is not fresh",
            ));
        }
        let mut sessions = Vec::new();
        for session in tx.list(Model::Session).await? {
            if session["userId"] == current.user["id"] && field_time(&session, "expiresAt")? > now {
                sessions.push(public_session(&session));
            }
        }
        tx.commit().await?;
        Ok(json!(sessions))
    }

    /// Authoritatively revokes one own session. Unknown and other-user tokens
    /// are deliberately indistinguishable successful no-ops.
    /// # Errors
    /// Returns UNAUTHORIZED or sanitized storage errors.
    pub async fn revoke_one(
        &self,
        current: &str,
        target: &str,
        transient: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.revoke(current, Revoke::One(target), transient, now)
            .await
    }
    /// # Errors
    /// Returns UNAUTHORIZED or sanitized storage errors.
    pub async fn revoke_all(
        &self,
        current: &str,
        transient: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.revoke(current, Revoke::All, transient, now).await
    }
    /// Revokes other active sessions, preserving the current session and
    /// already-expired rows, matching the actual handler's active-session filter.
    /// # Errors
    /// Returns UNAUTHORIZED or sanitized storage errors.
    pub async fn revoke_others(
        &self,
        current: &str,
        transient: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.revoke(current, Revoke::Others, transient, now).await
    }
    async fn revoke(
        &self,
        token: &str,
        mode: Revoke<'_>,
        transient: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let mut tx = self.store.begin(true).await?;
        let Some(current) = load(&mut tx, token, transient, now).await? else {
            tx.commit().await?;
            return Err(unauthorized());
        };
        for session in tx.list(Model::Session).await? {
            if session["userId"] != current.user["id"] {
                continue;
            }
            let candidate = field_text(&session, "token")?;
            let remove = match mode {
                Revoke::One(target) => candidate == target,
                Revoke::All => true,
                Revoke::Others => candidate != token && field_time(&session, "expiresAt")? > now,
            };
            if remove {
                tx.delete(Model::Session, field_text(&session, "id")?)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(json!({"status":true}))
    }

    /// Removes a signed-cookie session without requiring a fresh/active login.
    /// Cookie clearing belongs to the adapter and also happens for missing rows.
    /// # Errors
    /// Returns sanitized storage failures.
    pub async fn sign_out(&self, token: &str) -> BlueprintResult<()> {
        let mut tx = self.store.begin(true).await?;
        if let Some(row) = find_token(&mut tx, token).await? {
            tx.delete(Model::Session, field_text(&row, "id")?).await?;
        }
        tx.commit().await
    }
}
enum Revoke<'a> {
    One(&'a str),
    All,
    Others,
}

pub(crate) fn new_session(
    user_id: &str,
    ip: &str,
    agent: &str,
    now: DateTime<Utc>,
) -> BlueprintResult<Value> {
    let expires = now
        .checked_add_signed(Duration::days(1))
        .ok_or_else(storage_error)?;
    let alphabet = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut token = String::with_capacity(32);
    let mut bytes = [0_u8; 64];
    while token.len() < 32 {
        OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|_| storage_error())?;
        for byte in bytes {
            if byte < 248 {
                token.push(char::from(alphabet[usize::from(byte) % 62]));
            }
            if token.len() == 32 {
                break;
            }
        }
    }
    Ok(
        json!({"id":uuid::Uuid::new_v4().to_string(),"userId":user_id,"token":token,
        "createdAt":timestamp(now),"updatedAt":timestamp(now),"expiresAt":timestamp(expires),
        "ipAddress":ip,"userAgent":agent}),
    )
}
async fn find_token<T: AuthTransaction>(tx: &mut T, token: &str) -> BlueprintResult<Option<Value>> {
    if token.is_empty() {
        return Ok(None);
    }
    tx.find_unique(Model::Session, "token", token).await
}
pub(crate) async fn load<T: AuthTransaction>(
    tx: &mut T,
    token: &str,
    skip_refresh: bool,
    now: DateTime<Utc>,
) -> BlueprintResult<Option<SessionView>> {
    let Some(mut session) = find_token(tx, token).await? else {
        return Ok(None);
    };
    let expires = field_time(&session, "expiresAt")?;
    if expires < now {
        tx.delete(Model::Session, field_text(&session, "id")?)
            .await?;
        return Ok(None);
    }
    let Some(user) = tx.get(Model::User, field_text(&session, "userId")?).await? else {
        return Ok(None);
    };
    let threshold = expires
        .checked_sub_signed(Duration::hours(23))
        .ok_or_else(storage_error)?;
    let refreshed = !skip_refresh && threshold <= now;
    if refreshed {
        session["expiresAt"] = json!(timestamp(
            now.checked_add_signed(Duration::days(1))
                .ok_or_else(storage_error)?
        ));
        session["updatedAt"] = json!(timestamp(now));
        tx.replace(Model::Session, session.clone()).await?;
    }
    Ok(Some(SessionView {
        session: public_session(&session),
        user: public_user(&user),
        refreshed,
    }))
}
fn public_session(row: &Value) -> Value {
    select(
        row,
        &[
            "id",
            "expiresAt",
            "token",
            "createdAt",
            "updatedAt",
            "ipAddress",
            "userAgent",
            "userId",
            "activeOrganizationId",
            "activeTeamId",
        ],
    )
}
pub(crate) fn public_user(row: &Value) -> Value {
    select(
        row,
        &[
            "id",
            "name",
            "email",
            "emailVerified",
            "image",
            "createdAt",
            "updatedAt",
            "firstName",
            "lastName",
            "phoneNumber",
        ],
    )
}
fn select(row: &Value, fields: &[&str]) -> Value {
    Value::Object(
        fields
            .iter()
            .filter_map(|field| {
                row.get(*field)
                    .map(|value| ((*field).into(), value.clone()))
            })
            .collect(),
    )
}
fn field_text<'a>(row: &'a Value, field: &str) -> BlueprintResult<&'a str> {
    row[field].as_str().ok_or_else(storage_error)
}
fn field_time(row: &Value, field: &str) -> BlueprintResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(field_text(row, field)?)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|_| storage_error())
}
fn timestamp(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn unauthorized() -> BlueprintError {
    BlueprintError::new("UNAUTHORIZED", "Unauthorized")
}
fn storage_error() -> BlueprintError {
    BlueprintError::new(
        "better_auth_storage_error",
        "Authentication storage operation failed",
    )
}
