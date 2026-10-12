//! Native email signup/signin with atomic credential/session persistence.
use crate::{
    credentials,
    sessions::{new_session, public_user},
    store::{AuthStore, AuthTransaction, Model},
};
use chrono::{DateTime, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

mod password;
mod profile;
mod reset;
mod set;
pub use password::{ChangePasswordInput, PasswordChange};

#[cfg(test)]
pub(crate) mod tests;

/// Parsed signup input. The HTTP adapter additionally validates body shape,
/// media type, trusted Origin and callback URLs. Never log credential DTOs.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignupInput {
    pub name: String,
    pub email: String,
    pub password: String,
    pub first_name: String,
    pub last_name: String,
    pub image: Option<String>,
    pub phone_number: Option<String>,
    #[serde(default = "remember")]
    pub remember_me: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SigninInput {
    pub email: String,
    pub password: String,
    #[serde(rename = "callbackURL")]
    pub callback_url: Option<String>,
    #[serde(default = "remember")]
    pub remember_me: bool,
}
const fn remember() -> bool {
    true
}

/// Successful credential authentication. Cookie signing/attributes and redirect
/// header emission belong to the HTTP adapter. Private account data is excluded.
pub struct AuthGrant {
    pub user: Value,
    pub session: Value,
    pub persistent: bool,
    pub callback_url: Option<String>,
}
impl AuthGrant {
    #[must_use]
    pub fn signup_body(&self) -> Value {
        json!({"token":self.session["token"],"user":self.user})
    }
    #[must_use]
    pub fn signin_body(&self) -> Value {
        let mut body = json!({"redirect":self.callback_url.as_ref().is_some_and(|url| !url.is_empty()),"token":self.session["token"],"user":self.user});
        if let Some(url) = &self.callback_url {
            body["url"] = json!(url);
        }
        body
    }
}
pub struct CredentialService<S> {
    store: Arc<S>,
    workers: Arc<Semaphore>,
}
impl<S: AuthStore> CredentialService<S> {
    #[must_use]
    pub fn new(store: Arc<S>) -> Self {
        // Shared across service instances. Cancelling an HTTP future does not
        // release capacity until its blocking hash job has actually completed.
        static WORKERS: OnceLock<Arc<Semaphore>> = OnceLock::new();
        Self {
            store,
            workers: WORKERS.get_or_init(|| Arc::new(Semaphore::new(4))).clone(),
        }
    }

    /// Creates the user, credential account and session in one transaction.
    /// Password derivation occurs outside tenant write locks, then duplicate
    /// email is checked again inside the atomic write to handle concurrent signup.
    /// # Errors
    /// Returns actual credential error codes or sanitized persistence failures.
    pub async fn sign_up(
        &self,
        input: SignupInput,
        ip: &str,
        agent: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<AuthGrant> {
        self.sign_up_with_fields(input, serde_json::Map::new(), ip, agent, now)
            .await
    }

    pub(crate) async fn sign_up_with_fields(
        &self,
        input: SignupInput,
        fields: serde_json::Map<String, Value>,
        ip: &str,
        agent: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<AuthGrant> {
        validate_email(&input.email)?;
        validate_length(&input.password, true)?;
        let email = input.email.to_lowercase();
        let mut tx = self.store.begin(false).await?;
        let exists = find_email(&mut tx, &email).await?.is_some();
        tx.commit().await?;
        if exists {
            return Err(duplicate());
        }
        let password = Zeroizing::new(input.password);
        let hash = self
            .run_crypto(move || credentials::hash_password(&password))
            .await?;
        let mut tx = self.store.begin(true).await?;
        if find_email(&mut tx, &email).await?.is_some() {
            return Err(duplicate());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let timestamp = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let mut user = json!({"id":id,"email":email,"name":input.name,"firstName":input.first_name,"lastName":input.last_name,"emailVerified":false,"createdAt":timestamp,"updatedAt":timestamp});
        if let Some(image) = input.image {
            user["image"] = json!(image);
        }
        if let Some(phone) = input.phone_number {
            user["phoneNumber"] = json!(phone);
        }
        // Configured Better Auth additional fields are opaque at its HTTP
        // schema boundary. Preserve present null/JSON values without granting
        // writes to IDs, email verification, timestamps or authorization fields.
        for field in ["firstName", "lastName", "phoneNumber"] {
            if let Some(value) = fields.get(field) {
                user[field] = value.clone();
            }
        }
        tx.insert(Model::User, user.clone()).await?;
        tx.insert(Model::Account,json!({"id":uuid::Uuid::new_v4().to_string(),"userId":id,"accountId":id,"providerId":"credential","password":hash,"createdAt":timestamp,"updatedAt":timestamp})).await?;
        let session = new_session(&id, ip, agent, now)?;
        tx.insert(Model::Session, session.clone()).await?;
        tx.commit().await?;
        Ok(AuthGrant {
            user: public_user(&user),
            session,
            persistent: input.remember_me,
            callback_url: None,
        })
    }

    /// Verifies a credential and issues a new session. A second authoritative
    /// account read prevents concurrent password changes from accepting stale
    /// verification results. Missing credentials still perform a scrypt hash.
    /// # Errors
    /// Returns actual credential error codes or sanitized persistence failures.
    pub async fn sign_in(
        &self,
        input: SigninInput,
        ip: &str,
        agent: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<AuthGrant> {
        validate_email(&input.email)?;
        validate_length(&input.password, false)?;
        let mut tx = self.store.begin(false).await?;
        let user = find_email(&mut tx, &input.email.to_lowercase()).await?;
        let account = if let Some(user) = &user {
            find_credential(&mut tx, text(user, "id")?).await?
        } else {
            None
        };
        tx.commit().await?;
        let password = Zeroizing::new(input.password);
        let hash = account
            .as_ref()
            .and_then(|account| account["password"].as_str())
            .filter(|hash| !hash.is_empty())
            .map(|hash| Zeroizing::new(hash.to_owned()));
        let Some(hash) = hash else {
            self.run_crypto(move || credentials::hash_password(&password))
                .await?;
            return Err(invalid_credentials());
        };
        let expected = hash.clone();
        if !self
            .run_crypto(move || Ok(credentials::verify_password(&password, &hash)))
            .await?
        {
            return Err(invalid_credentials());
        }
        let user = user.ok_or_else(invalid_credentials)?;
        let account = account.ok_or_else(invalid_credentials)?;
        let mut tx = self.store.begin(true).await?;
        let current = tx
            .get(Model::Account, text(&account, "id")?)
            .await?
            .ok_or_else(invalid_credentials)?;
        if current["password"].as_str() != Some(expected.as_str())
            || current["userId"] != user["id"]
            || current["providerId"] != "credential"
            || current["accountId"] != user["id"]
        {
            return Err(invalid_credentials());
        }
        let user = tx
            .get(Model::User, text(&user, "id")?)
            .await?
            .ok_or_else(invalid_credentials)?;
        let session = new_session(text(&user, "id")?, ip, agent, now)?;
        tx.insert(Model::Session, session.clone()).await?;
        tx.commit().await?;
        Ok(AuthGrant {
            user: public_user(&user),
            session,
            persistent: input.remember_me,
            callback_url: input.callback_url,
        })
    }

    async fn run_crypto<T: Send + 'static>(
        &self,
        job: impl FnOnce() -> BlueprintResult<T> + Send + 'static,
    ) -> BlueprintResult<T> {
        let permit = self
            .workers
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| storage_error())?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            job()
        })
        .await
        .map_err(|_| storage_error())?
    }
}
async fn find_email<T: AuthTransaction>(tx: &mut T, email: &str) -> BlueprintResult<Option<Value>> {
    tx.find_unique(Model::User, "email", email).await
}
async fn find_credential<T: AuthTransaction>(
    tx: &mut T,
    user_id: &str,
) -> BlueprintResult<Option<Value>> {
    Ok(tx.list(Model::Account).await?.into_iter().find(|account| {
        account["userId"] == user_id
            && account["providerId"] == "credential"
            && account["accountId"] == user_id
    }))
}
pub(crate) fn validate_email(email: &str) -> BlueprintResult<()> {
    static EMAIL: OnceLock<Regex> = OnceLock::new();
    // The actual configured Zod 4 default email expression.
    let regex = EMAIL.get_or_init(|| Regex::new(r"^(?:[A-Za-z0-9_'+\-]+\.)*[A-Za-z0-9_'+\-]*[A-Za-z0-9_+-]@(?:[A-Za-z0-9][A-Za-z0-9\-]*\.)+[A-Za-z]{2,}$").expect("fixed email expression"));
    if regex.is_match(email) {
        Ok(())
    } else {
        Err(BlueprintError::new("INVALID_EMAIL", "Invalid email"))
    }
}
pub(crate) fn validate_length(password: &str, minimum: bool) -> BlueprintResult<()> {
    let length = password.encode_utf16().count();
    if minimum && length < 8 {
        return Err(BlueprintError::new(
            "PASSWORD_TOO_SHORT",
            "Password too short",
        ));
    }
    if length > 128 {
        return Err(BlueprintError::new(
            "PASSWORD_TOO_LONG",
            "Password too long",
        ));
    }
    Ok(())
}
fn text<'a>(row: &'a Value, field: &str) -> BlueprintResult<&'a str> {
    row[field].as_str().ok_or_else(storage_error)
}
fn duplicate() -> BlueprintError {
    BlueprintError::new(
        "USER_ALREADY_EXISTS_USE_ANOTHER_EMAIL",
        "User already exists. Use another email.",
    )
}
fn invalid_credentials() -> BlueprintError {
    BlueprintError::new("INVALID_EMAIL_OR_PASSWORD", "Invalid email or password")
}
fn storage_error() -> BlueprintError {
    BlueprintError::new(
        "better_auth_storage_error",
        "Authentication storage operation failed",
    )
}
