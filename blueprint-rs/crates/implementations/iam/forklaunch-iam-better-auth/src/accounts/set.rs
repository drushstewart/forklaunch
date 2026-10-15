//! Server-only initial password assignment for an authoritative signed-in user.
use super::{CredentialService, credentials, find_credential, text, validate_length};
use crate::{
    sessions::{SessionService, load},
    store::{AuthStore, AuthTransaction, Model},
};
use chrono::{DateTime, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};
use zeroize::Zeroizing;

impl<S: AuthStore> CredentialService<S> {
    /// Sets an initial password without changing existing sessions. This is a
    /// server-only helper in Better Auth, so it deliberately has no HTTP route.
    /// Derivation runs outside write locks; authoritative session and credential
    /// reads inside the write transaction prevent concurrent replacement.
    /// # Errors
    /// Returns session, password length, already-set or sanitized storage errors.
    pub async fn set_password(
        &self,
        token: &str,
        password: String,
        transient: bool,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let view = SessionService::new(self.store.clone())
            .with_session_policy(self.session_policy)
            .get(token, transient, false, now)
            .await?
            .ok_or_else(unauthorized)?;
        validate_length(&password, true)?;
        let password = Zeroizing::new(password);
        let hash = self
            .run_crypto(move || credentials::hash_password(&password))
            .await?;
        let mut tx = self.store.begin(true).await?;
        let current = load(&mut tx, token, true, now)
            .await?
            .ok_or_else(unauthorized)?;
        if current.user["id"] != view.user["id"] {
            return Err(unauthorized());
        }
        let user_id = text(&current.user, "id")?;
        let timestamp = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        if let Some(mut account) = find_credential(&mut tx, user_id).await? {
            if super::profile::truthy(&account["password"]) {
                return Err(BlueprintError::new(
                    "PASSWORD_ALREADY_SET",
                    "User already has a password set",
                ));
            }
            account["password"] = json!(hash);
            account["updatedAt"] = json!(timestamp);
            tx.replace(Model::Account, account).await?;
        } else {
            tx.insert(Model::Account, json!({"id":uuid::Uuid::new_v4().to_string(),"userId":user_id,"accountId":user_id,"providerId":"credential","password":hash,"createdAt":timestamp,"updatedAt":timestamp})).await?;
        }
        tx.commit().await?;
        Ok(json!({"status":true}))
    }
}

fn unauthorized() -> BlueprintError {
    BlueprintError::new("UNAUTHORIZED", "Unauthorized")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Arc;

    pub(crate) async fn exercise_set<S: AuthStore>(store: Arc<S>) {
        let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(
            Model::User,
            json!({"id":"set-native-owner","email":"set-native@fixture.test","name":"Set Owner"}),
        )
        .await
        .unwrap();
        tx.insert(Model::Account, json!({"id":"set-legacy","userId":"set-native-owner","accountId":"legacy-subject","providerId":"credential","password":"legacy-retained"})).await.unwrap();
        tx.commit().await.unwrap();
        let sessions = SessionService::new(store.clone());
        let session = sessions
            .create("set-native-owner", "", "", now)
            .await
            .unwrap();
        let token = session["token"].as_str().unwrap();
        let service = CredentialService::new(store.clone());
        let (left, right) = tokio::join!(
            service.set_password(token, "left-set-password".into(), false, now),
            service.set_password(token, "right-set-password".into(), false, now),
        );
        assert_ne!(left.is_ok(), right.is_ok());
        let password = if left.is_ok() {
            "left-set-password"
        } else {
            "right-set-password"
        };
        assert_eq!(
            left.err().or_else(|| right.err()).unwrap().code,
            "PASSWORD_ALREADY_SET"
        );
        let mut tx = store.begin(false).await.unwrap();
        let account = find_credential(&mut tx, "set-native-owner")
            .await
            .unwrap()
            .unwrap();
        assert!(credentials::verify_password(
            password,
            account["password"].as_str().unwrap()
        ));
        assert_eq!(
            tx.get(Model::Account, "set-legacy").await.unwrap().unwrap()["password"],
            "legacy-retained"
        );
        tx.commit().await.unwrap();
        assert!(
            sessions
                .get(token, false, false, now)
                .await
                .unwrap()
                .is_some()
        );
        sessions.sign_out(token).await.unwrap();
        assert_eq!(
            service
                .set_password(token, "valid-password".into(), false, now)
                .await
                .unwrap_err()
                .code,
            "UNAUTHORIZED"
        );
        exercise_empty(store, now).await;
    }

    async fn exercise_empty<S: AuthStore>(store: Arc<S>, now: DateTime<Utc>) {
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(
            Model::User,
            json!({"id":"set-empty-owner","email":"set-empty@fixture.test","name":"Empty Owner"}),
        )
        .await
        .unwrap();
        tx.insert(Model::Account,json!({"id":"set-empty-credential","userId":"set-empty-owner","accountId":"set-empty-owner","providerId":"credential","password":null})).await.unwrap();
        tx.commit().await.unwrap();
        let sessions = SessionService::new(store.clone());
        let session = sessions
            .create("set-empty-owner", "", "", now)
            .await
            .unwrap();
        let token = session["token"].as_str().unwrap();
        let service = CredentialService::new(store.clone());
        service
            .set_password(token, "empty-set-password".into(), false, now)
            .await
            .unwrap();
        let mut tx = store.begin(false).await.unwrap();
        let account = tx
            .get(Model::Account, "set-empty-credential")
            .await
            .unwrap()
            .unwrap();
        assert!(credentials::verify_password(
            "empty-set-password",
            account["password"].as_str().unwrap()
        ));
        tx.commit().await.unwrap();
        assert_eq!(
            service
                .set_password(token, "short".into(), false, now)
                .await
                .unwrap_err()
                .code,
            "PASSWORD_TOO_SHORT"
        );
        assert_eq!(
            service
                .set_password(
                    token,
                    "expired-password".into(),
                    false,
                    now + chrono::Duration::days(2)
                )
                .await
                .unwrap_err()
                .code,
            "UNAUTHORIZED"
        );
        let mut tx = store.begin(false).await.unwrap();
        assert!(
            tx.get(Model::Session, session["id"].as_str().unwrap())
                .await
                .unwrap()
                .is_none()
        );
        tx.commit().await.unwrap();
    }
    struct PausedSecondWrite {
        inner: crate::store::memory::MemoryAuthStore,
        writes: std::sync::atomic::AtomicUsize,
        ready: tokio::sync::Notify,
        resume: tokio::sync::Notify,
    }
    impl AuthStore for PausedSecondWrite {
        type Transaction<'a> = crate::store::memory::MemoryAuthTransaction;
        async fn begin(&self, write: bool) -> BlueprintResult<Self::Transaction<'_>> {
            if write
                && self
                    .writes
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    == 1
            {
                self.ready.notify_one();
                self.resume.notified().await;
            }
            self.inner.begin(write).await
        }
    }
    #[tokio::test]
    async fn revoked_during_set_derivation_cannot_write_a_password() {
        let store = Arc::new(crate::store::memory::MemoryAuthStore::default());
        let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(
            Model::User,
            json!({"id":"set-race-owner","email":"set-race@fixture.test","name":"Race Owner"}),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        let sessions = SessionService::new(store.clone());
        let session = sessions
            .create("set-race-owner", "", "", now)
            .await
            .unwrap();
        let token = session["token"].as_str().unwrap();
        let paused = Arc::new(PausedSecondWrite {
            inner: (*store).clone(),
            writes: std::sync::atomic::AtomicUsize::new(0),
            ready: tokio::sync::Notify::new(),
            resume: tokio::sync::Notify::new(),
        });
        let service = CredentialService::new(paused.clone());
        let ((), result) = tokio::time::timeout(std::time::Duration::from_secs(90), async {
            tokio::join!(
                async {
                    paused.ready.notified().await;
                    sessions.sign_out(token).await.unwrap();
                    paused.resume.notify_one();
                },
                service.set_password(token, "set-race-password".into(), false, now)
            )
        })
        .await
        .expect("set/revoke race must complete");
        assert_eq!(result.unwrap_err().code, "UNAUTHORIZED");
        let mut tx = store.begin(false).await.unwrap();
        assert!(tx.list(Model::Account).await.unwrap().is_empty());
        tx.commit().await.unwrap();
    }
}
