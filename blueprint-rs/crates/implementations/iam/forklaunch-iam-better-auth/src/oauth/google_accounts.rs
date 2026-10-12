//! Persistent Google identity resolution after a trusted provider verification.
use crate::{
    accounts::AuthGrant,
    sessions::{new_session, public_user},
    store::{AuthStore, AuthTransaction, Model},
};
use chrono::{DateTime, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};
use std::sync::Arc;

#[cfg(test)]
pub(crate) mod tests;

pub struct GoogleAccounts<S> {
    store: Arc<S>,
}
pub struct GoogleGrant {
    pub auth: AuthGrant,
    pub registered: bool,
}
impl<S: AuthStore> GoogleAccounts<S> {
    #[must_use]
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }
    /// Links a verified direct ID token to the authenticated local user.
    /// Existing own accounts may update tokens without re-running initial email policy.
    /// # Errors
    /// Returns configured direct-link ownership/email errors or storage failures.
    pub async fn link_direct_verified(
        &self,
        profile: &Value,
        tokens: &Value,
        user_id: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<()> {
        if !crate::keyring::verify::truthy(&profile["email"]) {
            return Err(error("USER_EMAIL_NOT_FOUND", "User email not found"));
        }
        let subject = account_subject(profile)?;
        let mut tx = self.store.begin(true).await?;
        let user = tx
            .get(Model::User, user_id)
            .await?
            .ok_or_else(|| error("UNAUTHORIZED", "Unauthorized"))?;
        let existing = tx
            .list(Model::Account)
            .await?
            .into_iter()
            .find(|row| row["providerId"] == "google" && row["accountId"] == subject);
        let timestamp = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        if let Some(mut account) = existing {
            if account["userId"] != user_id {
                return Err(error(
                    "SOCIAL_ACCOUNT_ALREADY_LINKED",
                    "Social account already linked",
                ));
            }
            copy_tokens(&mut account, tokens);
            account["updatedAt"] = json!(timestamp);
            tx.replace(Model::Account, account).await?;
        } else {
            if !crate::keyring::verify::truthy(&profile["email_verified"]) {
                return Err(error(
                    "LINKING_NOT_ALLOWED",
                    "Account not linked - linking not allowed",
                ));
            }
            let email = profile["email"].as_str().ok_or_else(internal)?;
            if Some(email.to_lowercase()) != user["email"].as_str().map(str::to_lowercase) {
                return Err(error(
                    "LINKING_DIFFERENT_EMAILS_NOT_ALLOWED",
                    "Account not linked - different emails not allowed",
                ));
            }
            let mut account = json!({"id":uuid::Uuid::new_v4().to_string(),"userId":user_id,"accountId":subject,
                "providerId":"google","createdAt":timestamp,"updatedAt":timestamp});
            copy_tokens(&mut account, tokens);
            tx.insert(Model::Account, account).await?;
        }
        tx.commit().await
    }
    /// Links a trusted provider profile to the server-owned identity saved in OAuth state.
    /// # Errors
    /// Rejects unverified provider email, mismatching email or another account owner.
    pub async fn link_verified(
        &self,
        profile: &Value,
        tokens: &Value,
        link: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<()> {
        if !crate::keyring::verify::truthy(&profile["email_verified"]) {
            return Err(error("unable_to_link_account", "Unable to link account"));
        }
        if profile["email"].as_str().map(str::to_lowercase)
            != link["email"].as_str().map(str::to_lowercase)
        {
            return Err(error("email_does_not_match", "Email does not match"));
        }
        let subject = account_subject(profile)?;
        let user_id = text(link, "userId")?;
        let mut tx = self.store.begin(true).await?;
        if tx.get(Model::User, user_id).await?.is_none() {
            return Err(error("unable_to_link_account", "Unable to link account"));
        }
        let existing = tx
            .list(Model::Account)
            .await?
            .into_iter()
            .find(|row| row["providerId"] == "google" && row["accountId"] == subject);
        let timestamp = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        if let Some(mut account) = existing {
            if account["userId"] != user_id {
                return Err(error(
                    "account_already_linked_to_different_user",
                    "Account already linked to different user",
                ));
            }
            copy_tokens(&mut account, tokens);
            let scope = merged_scope(&account, tokens);
            if !scope.is_empty() {
                account["scope"] = json!(scope);
            }
            account["updatedAt"] = json!(timestamp);
            tx.replace(Model::Account, account).await?;
        } else {
            let mut account = json!({"id":uuid::Uuid::new_v4().to_string(),"userId":user_id,
                "accountId":subject,"providerId":"google","createdAt":timestamp,"updatedAt":timestamp});
            copy_tokens(&mut account, tokens);
            if let Some(scope) = tokens.get("scope") {
                account["scope"] = scope.clone();
            }
            tx.insert(Model::Account, account).await?;
        }
        tx.commit().await
    }
    /// Resolves a verified Google profile and persists user/account/session atomically.
    /// The caller must verify the ID token or obtain it from Google's trusted token
    /// endpoint first. Request JSON is never a trusted profile.
    /// # Errors
    /// Rejects missing email/subject, orphaned accounts and unsafe implicit linking.
    pub async fn sign_in_verified(
        &self,
        profile: &Value,
        tokens: &Value,
        ip: &str,
        agent: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<GoogleGrant> {
        let email_value = profile.get("email");
        if !email_value.is_some_and(crate::keyring::verify::truthy) {
            return Err(error("USER_EMAIL_NOT_FOUND", "User email not found"));
        }
        let email = email_value
            .and_then(Value::as_str)
            .ok_or_else(internal)?
            .to_lowercase();
        let subject = account_subject(profile)?;
        let verified = profile
            .get("email_verified")
            .filter(|value| !value.is_null())
            .cloned()
            .unwrap_or(json!(false));
        let mut tx = self.store.begin(true).await?;
        let existing = tx
            .list(Model::Account)
            .await?
            .into_iter()
            .find(|row| row["providerId"] == "google" && row["accountId"] == subject);
        let user = if let Some(account) = &existing {
            tx.get(Model::User, text(account, "userId")?)
                .await?
                .ok_or_else(|| link_error("unable to link account"))?
                .into()
        } else {
            tx.list(Model::User)
                .await?
                .into_iter()
                .find(|row| row["email"] == email)
        };
        let registered = user.is_none();
        let timestamp = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let mut user = if let Some(user) = user {
            if existing.is_none()
                && (!crate::keyring::verify::truthy(&verified)
                    || !crate::keyring::verify::truthy(&user["emailVerified"]))
            {
                return Err(link_error("account not linked"));
            }
            user
        } else {
            let row = new_user(profile, &email, &verified, &timestamp);
            tx.insert(Model::User, row.clone()).await?;
            row
        };
        if let Some(mut account) = existing {
            copy_tokens(&mut account, tokens);
            account["updatedAt"] = json!(timestamp);
            tx.replace(Model::Account, account).await?;
            if crate::keyring::verify::truthy(&verified)
                && !crate::keyring::verify::truthy(&user["emailVerified"])
                && user["email"] == email
            {
                // Upstream returns its original user object for this sign-in;
                // only subsequent authoritative reads see the verification update.
                let mut updated = user.clone();
                updated["emailVerified"] = json!(true);
                updated["updatedAt"] = json!(timestamp);
                tx.replace(Model::User, updated).await?;
            }
        } else {
            let mut account = json!({"id":uuid::Uuid::new_v4().to_string(),"providerId":"google",
                "accountId":subject,"userId":user["id"],"createdAt":timestamp,"updatedAt":timestamp});
            copy_tokens(&mut account, tokens);
            if let Some(scope) = tokens.get("scope") {
                account["scope"] = scope.clone();
            }
            tx.insert(Model::Account, account).await?;
        }
        let session = new_session(text(&user, "id")?, ip, agent, now)?;
        tx.insert(Model::Session, session.clone()).await?;
        tx.commit().await?;
        user = public_user(&user);
        Ok(GoogleGrant {
            auth: AuthGrant {
                user,
                session,
                persistent: true,
                callback_url: None,
            },
            registered,
        })
    }
}
fn copy_tokens(account: &mut Value, tokens: &Value) {
    for field in [
        "idToken",
        "accessToken",
        "refreshToken",
        "accessTokenExpiresAt",
        "refreshTokenExpiresAt",
    ] {
        if let Some(value) = tokens.get(field) {
            account[field] = value.clone();
        }
    }
}
fn text<'a>(row: &'a Value, field: &str) -> BlueprintResult<&'a str> {
    row[field].as_str().ok_or_else(internal)
}
fn error(code: &str, message: &str) -> BlueprintError {
    BlueprintError::new(code, message)
}
fn link_error(message: &str) -> BlueprintError {
    error("OAUTH_LINK_ERROR", message)
}
fn internal() -> BlueprintError {
    error("INTERNAL_SERVER_ERROR", "Authentication operation failed")
}

fn new_user(profile: &Value, email: &str, verified: &Value, timestamp: &str) -> Value {
    let name = profile
        .get("name")
        .filter(|value| crate::keyring::verify::truthy(value))
        .cloned()
        .unwrap_or(json!(""));
    let mut row = json!({"id":uuid::Uuid::new_v4().to_string(),"name":name,"email":email,
                "emailVerified":verified,"firstName":profile["given_name"].as_str().unwrap_or(""),
                "lastName":profile["family_name"].as_str().unwrap_or(""),
                "createdAt":timestamp,"updatedAt":timestamp});
    if let Some(image) = profile.get("picture") {
        row["image"] = image.clone();
    }

    row
}

pub(crate) fn account_subject(profile: &Value) -> BlueprintResult<String> {
    let subject = crate::ecmascript::string(profile.get("sub"))
        .ok_or_else(|| error("FAILED_TO_GET_USER_INFO", "Failed to get user info"))?;
    if subject
        .trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace() && c != '\u{85}')
        .is_empty()
        || matches!(subject.as_str(), "null" | "undefined")
        || profile["sub"].is_number() && !profile["sub"].as_f64().is_some_and(f64::is_finite)
    {
        return Err(error("FAILED_TO_GET_USER_INFO", "Failed to get user info"));
    }

    Ok(subject)
}

fn merged_scope(account: &Value, tokens: &Value) -> String {
    let mut incoming = Vec::new();
    if let Some(scopes) = tokens["scopes"].as_array() {
        incoming.extend(scopes.iter().filter_map(Value::as_str));
    } else if let Some(scope) = tokens["scope"].as_str() {
        incoming.extend(scope.split(','));
    }
    let mut merged = Vec::new();
    for scope in account["scope"]
        .as_str()
        .into_iter()
        .flat_map(|scope| scope.split(','))
        .chain(incoming)
    {
        let scope =
            scope.trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace() && c != '\u{85}');
        if !scope.is_empty() && !merged.contains(&scope) {
            merged.push(scope);
        }
    }
    merged.join(",")
}
