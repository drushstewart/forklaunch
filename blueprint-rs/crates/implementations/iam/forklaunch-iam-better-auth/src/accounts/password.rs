//! Authenticated password changes with authoritative rechecks after derivation.
use super::{CredentialService, credentials, find_credential, new_session, text, validate_length};
use crate::sessions::{SessionService, load};
use crate::store::{AuthStore, AuthTransaction, Model};
use chrono::{DateTime, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde::Deserialize;
use serde_json::{Value, json};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordInput {
    pub current_password: String,
    pub new_password: String,
    #[serde(default)]
    pub revoke_other_sessions: bool,
}

/// A replacement session is present only when all previous sessions were revoked.
/// The HTTP adapter must emit its signed cookie in that case, retaining the
/// existing browser `dont_remember` flag.
pub struct PasswordChange {
    pub user: Value,
    pub session: Option<Value>,
}
impl PasswordChange {
    #[must_use]
    pub fn body(&self) -> Value {
        json!({"token":self.session.as_ref().map(|session| &session["token"]),"user":self.user})
    }
}

impl<S: AuthStore> CredentialService<S> {
    /// Validates a current user's password using authoritative session state.
    /// # Errors
    /// Returns `UNAUTHORIZED`, `PASSWORD_TOO_LONG`, `INVALID_PASSWORD` or storage errors.
    pub async fn verify_password(
        &self,
        token: &str,
        password: String,
        transient: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let view = SessionService::new(self.store.clone())
            .get(token, transient, false, now)
            .await?
            .ok_or_else(unauthorized)?;
        validate_length(&password, false)?;
        let mut tx = self.store.begin(false).await?;
        let account = find_credential(&mut tx, text(&view.user, "id")?).await?;
        tx.commit().await?;
        let Some(account) = account else {
            return Err(invalid_password());
        };
        let hash = Zeroizing::new(text(&account, "password")?.to_owned());
        if hash.is_empty() {
            return Err(invalid_password());
        }
        let expected = hash.clone();
        let password = Zeroizing::new(password);
        if !self
            .run_crypto(move || Ok(credentials::verify_password(&password, &hash)))
            .await?
        {
            return Err(invalid_password());
        }
        let mut tx = self.store.begin(true).await?;
        let current = load(&mut tx, token, true, now)
            .await?
            .ok_or_else(unauthorized)?;
        if current.user["id"] != view.user["id"] {
            return Err(unauthorized());
        }
        recheck_account(&mut tx, &account, &expected).await?;
        tx.commit().await?;
        Ok(json!({"status":true}))
    }

    /// Changes the credential atomically. With revokeOtherSessions=true the
    /// actual handler revokes *all* own sessions, including the current one, and
    /// creates a replacement session. Its browser cookie retains the existing
    /// `dont_remember` flag. Unrelated users remain untouched.
    /// Password work runs outside the write lock; session and credential state
    /// are rechecked inside the transaction to reject concurrent revocation or
    /// password replacement.
    /// # Errors
    /// Returns configured password/session codes or sanitized storage errors.
    pub async fn change_password(
        &self,
        token: &str,
        input: ChangePasswordInput,
        transient: bool,
        ip: &str,
        agent: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<PasswordChange> {
        let view = SessionService::new(self.store.clone())
            .get(token, transient, false, now)
            .await?
            .ok_or_else(unauthorized)?;
        validate_length(&input.new_password, true)?;
        validate_length(&input.current_password, false)?;
        let user_id = text(&view.user, "id")?;
        let mut tx = self.store.begin(false).await?;
        let account = find_credential(&mut tx, user_id).await?;
        tx.commit().await?;
        let account = account
            .filter(|account| {
                account["password"]
                    .as_str()
                    .is_some_and(|hash| !hash.is_empty())
            })
            .ok_or_else(|| {
                BlueprintError::new(
                    "CREDENTIAL_ACCOUNT_NOT_FOUND",
                    "Credential account not found",
                )
            })?;
        let expected = Zeroizing::new(text(&account, "password")?.to_owned());
        let old_hash = expected.clone();
        let current = Zeroizing::new(input.current_password);
        let replacement = Zeroizing::new(input.new_password);
        let password_hash = self
            .run_crypto(move || {
                // The actual handler derives the new password before checking old.
                let hash = credentials::hash_password(&replacement)?;
                if !credentials::verify_password(&current, &old_hash) {
                    return Err(invalid_password());
                }
                Ok(Zeroizing::new(hash))
            })
            .await?;
        let mut tx = self.store.begin(true).await?;
        let current = load(&mut tx, token, true, now)
            .await?
            .ok_or_else(unauthorized)?;
        if current.user["id"] != view.user["id"] {
            return Err(unauthorized());
        }
        let mut account = recheck_account(&mut tx, &account, &expected).await?;
        account["password"] = json!(password_hash.as_str());
        account["updatedAt"] = json!(now.to_rfc3339_opts(SecondsFormat::Millis, true));
        tx.replace(Model::Account, account).await?;
        let session = if input.revoke_other_sessions {
            for session in tx.list(Model::Session).await? {
                if session["userId"] == user_id {
                    tx.delete(Model::Session, text(&session, "id")?).await?;
                }
            }
            let session = new_session(user_id, ip, agent, now)?;
            tx.insert(Model::Session, session.clone()).await?;
            Some(session)
        } else {
            None
        };
        tx.commit().await?;
        Ok(PasswordChange {
            user: current.user,
            session,
        })
    }
}

async fn recheck_account<T: AuthTransaction>(
    tx: &mut T,
    previous: &Value,
    expected: &str,
) -> BlueprintResult<Value> {
    let current = tx
        .get(Model::Account, text(previous, "id")?)
        .await?
        .ok_or_else(invalid_password)?;
    if current["password"].as_str() != Some(expected)
        || current["userId"] != previous["userId"]
        || current["accountId"] != previous["accountId"]
        || current["providerId"] != "credential"
    {
        return Err(invalid_password());
    }
    Ok(current)
}
fn unauthorized() -> BlueprintError {
    BlueprintError::new("UNAUTHORIZED", "Unauthorized")
}
fn invalid_password() -> BlueprintError {
    BlueprintError::new("INVALID_PASSWORD", "Invalid password")
}
