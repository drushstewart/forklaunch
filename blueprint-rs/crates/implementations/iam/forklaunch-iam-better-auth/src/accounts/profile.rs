//! Authoritative profile updates and linked-account listing/removal.
use super::{CredentialService, storage_error as internal, text};
use crate::{
    sessions::{SessionView, load_with_time},
    store::{AuthStore, AuthTransaction, Model},
};
use chrono::{DateTime, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};

impl<S: AuthStore> CredentialService<S> {
    /// Lists only the current user's linked accounts and omits all token/hash data.
    /// # Errors
    /// Rejects absent sessions or corrupt storage.
    pub async fn list_accounts(&self, token: &str, now: DateTime<Utc>) -> BlueprintResult<Value> {
        let mut tx = self.store.begin(true).await?;
        let view = load_with_time(
            &mut tx,
            token,
            true,
            now,
            self.session_policy,
            &self.time_source,
        )
        .await?
        .ok_or_else(unauthorized)?;
        let mut accounts = Vec::new();
        for row in
            crate::store::find_all(&mut tx, Model::Account, "userId", text(&view.user, "id")?)
                .await?
        {
            if row["userId"] != view.user["id"] {
                continue;
            }
            let mut output = json!({});
            for field in [
                "id",
                "accountId",
                "providerId",
                "userId",
                "createdAt",
                "updatedAt",
            ] {
                if let Some(value) = row.get(field) {
                    output[field] = value.clone();
                }
            }
            let scopes = match row.get("scope") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::String(scope)) => scope
                    .split(',')
                    .map(|scope| {
                        scope.trim_matches(|c: char| {
                            c == '\u{feff}' || c.is_whitespace() && c != '\u{85}'
                        })
                    })
                    .filter(|scope| !scope.is_empty())
                    .collect(),
                _ => return Err(internal()),
            };
            output["scopes"] = json!(scopes);
            accounts.push(output);
        }
        tx.commit().await?;
        Ok(json!(accounts))
    }

    /// Atomically removes an owned account while retaining at least one login.
    /// # Errors
    /// Rejects stale sessions, unknown accounts and removal of the last account.
    pub async fn unlink_account(
        &self,
        token: &str,
        account_id: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let mut tx = self.store.begin(true).await?;
        let view = load_with_time(
            &mut tx,
            token,
            true,
            now,
            self.session_policy,
            &self.time_source,
        )
        .await?
        .ok_or_else(unauthorized)?;
        let created = DateTime::parse_from_rfc3339(text(&view.session, "createdAt")?)
            .map_err(|_| internal())?;
        let now = self.time_source.sample(now);
        if !self.session_policy.fresh(created.with_timezone(&Utc), now) {
            return Err(BlueprintError::new(
                "SESSION_NOT_FRESH",
                "Session is not fresh",
            ));
        }
        let accounts: Vec<_> =
            crate::store::find_all(&mut tx, Model::Account, "userId", text(&view.user, "id")?)
                .await?
                .into_iter()
                .filter(|row| row["userId"] == view.user["id"])
                .collect();
        if accounts.len() == 1 {
            return Err(BlueprintError::new(
                "FAILED_TO_UNLINK_LAST_ACCOUNT",
                "You can't unlink your last account",
            ));
        }
        if !accounts
            .iter()
            .any(|row| row["id"].as_str() == Some(account_id))
        {
            return Err(BlueprintError::new(
                "ACCOUNT_NOT_FOUND",
                "Account not found",
            ));
        }
        tx.delete(Model::Account, account_id).await?;
        tx.commit().await?;
        Ok(json!({"status":true}))
    }

    /// Applies configured writable profile fields to the authoritative user.
    /// # Errors
    /// Rejects email changes, absent fields, revoked sessions or storage failures.
    pub async fn update_profile(
        &self,
        token: &str,
        body: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<SessionView> {
        if body.get("email").is_some_and(truthy) {
            return Err(BlueprintError::new(
                "EMAIL_CAN_NOT_BE_UPDATED",
                "Email can not be updated",
            ));
        }
        let mut updates = serde_json::Map::new();
        for field in ["name", "image", "firstName", "lastName", "phoneNumber"] {
            if let Some(value) = body.get(field) {
                updates.insert(field.into(), value.clone());
            }
        }
        if updates.is_empty() {
            return Err(BlueprintError::new(
                "HTTP_MESSAGE_ONLY",
                "No fields to update",
            ));
        }
        let mut tx = self.store.begin(true).await?;
        let mut view = load_with_time(
            &mut tx,
            token,
            true,
            now,
            self.session_policy,
            &self.time_source,
        )
        .await?
        .ok_or_else(unauthorized)?;
        let mut user = tx
            .get(Model::User, text(&view.user, "id")?)
            .await?
            .ok_or_else(unauthorized)?;
        for (field, value) in updates {
            user[&field] = value;
        }
        user["updatedAt"] = json!(now.to_rfc3339_opts(SecondsFormat::Millis, true));
        tx.replace(Model::User, user.clone()).await?;
        tx.commit().await?;
        view.user = crate::sessions::public_user(&user);
        Ok(view)
    }
}
fn unauthorized() -> BlueprintError {
    BlueprintError::new("UNAUTHORIZED", "Unauthorized")
}
pub(super) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(string) => !string.is_empty(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{sessions::SessionService, store::memory::MemoryAuthStore};
    use std::sync::Arc;

    #[tokio::test]
    async fn concurrent_unlink_keeps_one_account_and_revocation_blocks_profile_updates() {
        let store = Arc::new(MemoryAuthStore::default());
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(Model::User, json!({"id":"profile-owner","email":"profile@fixture.test","name":"Owner","privateSecret":"preserved"})).await.unwrap();
        for id in ["first", "second"] {
            tx.insert(Model::Account, json!({"id":id,"userId":"profile-owner","providerId":"google","accountId":id,"accessToken":"never-public"})).await.unwrap();
        }
        tx.commit().await.unwrap();
        let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        let session = SessionService::new(store.clone())
            .create("profile-owner", "", "", now)
            .await
            .unwrap();
        let token = session["token"].as_str().unwrap();
        let service = CredentialService::new(store.clone());
        let (first, second) = tokio::join!(
            service.unlink_account(token, "first", now),
            service.unlink_account(token, "second", now)
        );
        assert_ne!(first.is_ok(), second.is_ok());
        let error = first.err().or_else(|| second.err()).unwrap();
        assert_eq!(error.code, "FAILED_TO_UNLINK_LAST_ACCOUNT");
        let accounts = service.list_accounts(token, now).await.unwrap();
        assert_eq!(accounts.as_array().unwrap().len(), 1);
        assert!(accounts[0].get("accessToken").is_none());
        service
            .update_profile(
                token,
                &json!({"firstName":"Changed", "privateSecret":"injected"}),
                now,
            )
            .await
            .unwrap();
        SessionService::new(store.clone())
            .sign_out(token)
            .await
            .unwrap();
        let error = service
            .update_profile(token, &json!({"firstName":"revoked"}), now)
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, "UNAUTHORIZED");
        let mut tx = store.begin(false).await.unwrap();
        let user = tx.get(Model::User, "profile-owner").await.unwrap().unwrap();
        assert_eq!(user["firstName"], "Changed");
        assert_eq!(user["privateSecret"], "preserved");
        tx.commit().await.unwrap();
    }
}
