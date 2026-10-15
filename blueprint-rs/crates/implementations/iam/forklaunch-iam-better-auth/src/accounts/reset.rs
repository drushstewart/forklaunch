//! Single-use password reset tokens over authoritative encrypted records.
use super::{CredentialService, credentials, storage_error, text};
use crate::store::{AuthStore, AuthTransaction, Model};
use chrono::{DateTime, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};
use zeroize::Zeroizing;

impl<S: AuthStore> CredentialService<S> {
    /// Installs the trusted `onPasswordReset` callback subset. It is always awaited
    /// after password commit and before optional session revocation.
    #[must_use]
    pub fn with_password_reset_observer(
        mut self,
        observer: std::sync::Arc<dyn super::PasswordResetObserver>,
    ) -> Self {
        self.password_reset_observer = Some(observer);
        self
    }

    /// Enables a separate session revocation transaction after password commit.
    /// Defaults to false. A revocation error does not undo the password update or
    /// restore the consumed reset proof. All current user sessions, including expired
    /// ones, are selected through the tenant-bound store's indexed user lookup.
    #[must_use]
    pub fn with_revoke_sessions_on_password_reset(mut self, enabled: bool) -> Self {
        self.revoke_sessions_on_password_reset = enabled;
        self
    }

    /// Consumes the newest verification and invalidates duplicates before hashing.
    /// Token consumption persists even for expired tokens or missing users, matching
    /// the configured handler. Resets retain sessions unless revocation is enabled.
    /// # Errors
    /// Returns token/password/user errors or sanitized storage failures.
    pub async fn reset_password(
        &self,
        token: &str,
        password: String,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        if token.is_empty() {
            return Err(invalid_token());
        }
        self.credential_policy.validate(&password, true)?;
        let identifier = format!("reset-password:{token}");
        let mut tx = self.store.begin(true).await?;
        let mut latest: Option<(DateTime<Utc>, Value)> = None;
        for row in
            crate::store::find_all(&mut tx, Model::Verification, "identifier", &identifier).await?
        {
            if row["identifier"].as_str() != Some(&identifier) {
                continue;
            }
            let created = time(&row, "createdAt")?;
            if latest.as_ref().is_none_or(|(best, _)| created > *best) {
                latest = Some((created, row.clone()));
            }
            tx.delete(Model::Verification, text(&row, "id")?).await?;
        }
        tx.commit().await?;
        let (_, row) = latest.ok_or_else(invalid_token)?;
        if time(&row, "expiresAt")? < now {
            return Err(invalid_token());
        }
        let user_id = text(&row, "value")?.to_owned();
        let mut tx = self.store.begin(false).await?;
        let user = tx.get(Model::User, &user_id).await?;
        tx.commit().await?;
        let user = user.ok_or_else(user_not_found)?;
        // Capture before the password update, matching the source callback snapshot.
        // Projection validation errors are delayed until after password commit.
        let observer_context = self
            .password_reset_observer
            .as_ref()
            .map(|_| super::reset_observer::context(&user));
        let password = Zeroizing::new(password);
        let hash = self
            .run_crypto(move || credentials::hash_password(&password))
            .await?;
        let mut tx = self.store.begin(true).await?;
        if tx.get(Model::User, &user_id).await?.is_none() {
            return Err(user_not_found());
        }
        let account = super::find_credential(&mut tx, &user_id).await?;
        let timestamp = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        if account.is_some() {
            for mut row in
                crate::store::find_all(&mut tx, Model::Account, "userId", &user_id).await?
            {
                if row["userId"] == user_id
                    && row["providerId"] == "credential"
                    && row["accountId"] == user_id
                {
                    row["password"] = json!(hash);
                    row["updatedAt"] = json!(timestamp);
                    tx.replace(Model::Account, row).await?;
                }
            }
        } else {
            tx.insert(Model::Account, json!({"id":uuid::Uuid::new_v4().to_string(),"userId":user_id,"accountId":user_id,"providerId":"credential","password":hash,"createdAt":timestamp,"updatedAt":timestamp})).await?;
        }
        tx.commit().await?;
        if let (Some(observer), Some(context)) = (&self.password_reset_observer, observer_context) {
            observer
                .prepare(context?)
                .map_err(|_| super::reset_observer::failure())?
                .await
                .map_err(|_| super::reset_observer::failure())?;
        }
        if self.revoke_sessions_on_password_reset {
            self.revoke_reset_sessions(&user_id).await?;
        }
        Ok(json!({"status":true}))
    }
    async fn revoke_reset_sessions(&self, user_id: &str) -> BlueprintResult<()> {
        let mut tx = self.store.begin(true).await?;
        let mut after = None;
        loop {
            let page = tx
                .find_page(Model::Session, "userId", user_id, after, 256)
                .await?;
            for session in page.records {
                if session["userId"].as_str() == Some(user_id) {
                    tx.delete(Model::Session, text(&session, "id")?).await?;
                }
            }
            after = page.next;
            if after.is_none() {
                break;
            }
        }
        tx.commit().await
    }
}
fn time(row: &Value, field: &str) -> BlueprintResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text(row, field)?)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|_| storage_error())
}
fn invalid_token() -> BlueprintError {
    BlueprintError::new("INVALID_TOKEN", "Invalid token")
}
fn user_not_found() -> BlueprintError {
    BlueprintError::new("USER_NOT_FOUND", "User not found")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::sessions::SessionService;
    use std::sync::Arc;

    pub(crate) async fn exercise_reset<S: AuthStore>(store: Arc<S>) {
        let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        seed_reset(store.as_ref(), now).await;
        let sessions = SessionService::new(store.clone());
        let session = sessions
            .create("reset-native-owner", "", "", now)
            .await
            .unwrap();
        let service = CredentialService::new(store.clone());
        let (left, right) = tokio::join!(
            service.reset_password("single", "left-reset-password".into(), now),
            service.reset_password("single", "right-reset-password".into(), now),
        );
        assert_ne!(left.is_ok(), right.is_ok());
        let password = if left.is_ok() {
            "left-reset-password"
        } else {
            "right-reset-password"
        };
        assert_eq!(
            left.err().or_else(|| right.err()).unwrap().code,
            "INVALID_TOKEN"
        );
        let mut tx = store.begin(false).await.unwrap();
        let account = tx
            .list(Model::Account)
            .await
            .unwrap()
            .into_iter()
            .find(|row| {
                row["userId"] == "reset-native-owner" && row["accountId"] == "reset-native-owner"
            })
            .unwrap();
        assert!(credentials::verify_password(
            password,
            account["password"].as_str().unwrap()
        ));
        assert_eq!(
            tx.get(Model::Account, "legacy-reset-account")
                .await
                .unwrap()
                .unwrap()["password"],
            "not-selected"
        );
        assert!(
            tx.get(Model::Verification, "single-reset")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            tx.get(Model::Verification, "unrelated-reset")
                .await
                .unwrap()
                .is_some()
        );
        tx.commit().await.unwrap();
        assert!(
            sessions
                .get(session["token"].as_str().unwrap(), false, false, now)
                .await
                .unwrap()
                .is_some()
        );
        check_updated_credential(store.as_ref(), &service, now).await;
        assert_eq!(
            service
                .reset_password("expired", "valid-password".into(), now)
                .await
                .unwrap_err()
                .code,
            "INVALID_TOKEN"
        );
        let mut tx = store.begin(false).await.unwrap();
        assert!(
            tx.get(Model::Verification, "expired-reset")
                .await
                .unwrap()
                .is_none()
        );
        tx.commit().await.unwrap();
    }
    async fn check_updated_credential<S: AuthStore>(
        store: &S,
        service: &CredentialService<S>,
        now: DateTime<Utc>,
    ) {
        service
            .reset_password("update-all", "updated-again-password".into(), now)
            .await
            .unwrap();
        let mut tx = store.begin(false).await.unwrap();
        let accounts: Vec<_> = tx
            .list(Model::Account)
            .await
            .unwrap()
            .into_iter()
            .filter(|row| {
                row["userId"] == "reset-native-owner" && row["providerId"] == "credential"
            })
            .collect();
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0]["password"], "not-selected");
        assert!(credentials::verify_password(
            "updated-again-password",
            accounts[1]["password"].as_str().unwrap()
        ));
        tx.commit().await.unwrap();
    }

    async fn seed_reset<S: AuthStore>(store: &S, now: DateTime<Utc>) {
        let timestamp = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(Model::User, json!({"id":"reset-native-owner","email":"reset-native@fixture.test","name":"Reset Owner"})).await.unwrap();
        tx.insert(Model::Account,json!({"id":"legacy-reset-account","userId":"reset-native-owner","providerId":"credential","accountId":"legacy-subject","password":"not-selected"})).await.unwrap();
        for (id, identifier, expires) in [
            (
                "single-reset",
                "reset-password:single",
                now + chrono::Duration::minutes(1),
            ),
            (
                "update-all-reset",
                "reset-password:update-all",
                now + chrono::Duration::minutes(1),
            ),
            (
                "expired-reset",
                "reset-password:expired",
                now - chrono::Duration::milliseconds(1),
            ),
            (
                "unrelated-reset",
                "other:retained",
                now + chrono::Duration::minutes(1),
            ),
        ] {
            tx.insert(Model::Verification, json!({"id":id,"identifier":identifier,"value":"reset-native-owner","createdAt":timestamp,"updatedAt":timestamp,"expiresAt":expires.to_rfc3339_opts(SecondsFormat::Millis,true)})).await.unwrap();
        }
        tx.commit().await.unwrap();
    }
}

#[cfg(all(test, feature = "http"))]
#[path = "reset_revocation_tests.rs"]
pub(crate) mod revocation_tests;
