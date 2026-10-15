use super::*;
use crate::accounts::CredentialService;
use crate::store::{AuthStore, AuthTransaction, Model, memory::MemoryAuthStore};
use chrono::{DateTime, Utc};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Observer<S> {
    store: Arc<S>,
    mode: &'static str,
    calls: Arc<AtomicUsize>,
    entered: Arc<tokio::sync::Notify>,
}
impl<S: AuthStore + 'static> PasswordResetObserver for Observer<S> {
    fn prepare(
        &self,
        context: PasswordResetObserverContext,
    ) -> BlueprintResult<crate::background::DeliveryFuture> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let user: Value = serde_json::from_str(context.user_json()).unwrap();
        assert!(user.get("password").is_none());
        assert!(user.get("token").is_none());
        if self.mode == "sync" {
            return Err(BlueprintError::new("owned-secret", "owned-secret"));
        }
        let store = self.store.clone();
        let mode = self.mode;
        let entered = self.entered.clone();
        Ok(Box::pin(async move {
            if mode == "async" {
                return Err(BlueprintError::new("owned-secret", "owned-secret"));
            }
            let id = user["id"].as_str().unwrap();
            let mut tx = store.begin(false).await?;
            let account = super::super::find_credential(&mut tx, id).await?.unwrap();
            assert!(crate::credentials::verify_password(
                "new-observer-password",
                account["password"].as_str().unwrap()
            ));
            tx.commit().await?;
            if mode == "pending" {
                entered.notify_one();
                std::future::pending::<()>().await;
            }
            crate::sessions::SessionService::new(store)
                .create(
                    id,
                    "",
                    "",
                    DateTime::from_timestamp(1_800_000_000, 0).unwrap(),
                )
                .await?;
            Ok(())
        }))
    }
}
pub(crate) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for mode in ["success", "sync", "async", "malformed", "no-hook-malformed"] {
        let id = format!("reset-observer-{mode}");
        super::super::reset::revocation_tests::seed(store.as_ref(), &id, now, 2).await;
        if mode.contains("malformed") {
            let mut tx = store.begin(true).await.unwrap();
            let mut row = tx.get(Model::User, &id).await.unwrap().unwrap();
            row["name"] = json!({"nested":"untrusted"});
            tx.replace(Model::User, row).await.unwrap();
            tx.commit().await.unwrap();
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let mut service =
            CredentialService::new(store.clone()).with_revoke_sessions_on_password_reset(true);
        if mode != "no-hook-malformed" {
            service = service.with_password_reset_observer(Arc::new(Observer {
                store: store.clone(),
                mode,
                calls: calls.clone(),
                entered: Arc::default(),
            }));
        }
        let result = service
            .reset_password(&id, "new-observer-password".into(), now)
            .await;
        let failed = matches!(mode, "sync" | "async" | "malformed");
        assert_eq!(result.is_err(), failed);
        if let Err(error) = result {
            assert_eq!(error.code, "PASSWORD_RESET_OBSERVER_FAILED");
            assert!(!error.message.contains("owned-secret"));
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            usize::from(!mode.contains("malformed"))
        );
        check(store.as_ref(), &id, failed).await;
    }
}
async fn check<S: AuthStore>(store: &S, id: &str, retained: bool) {
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.get(Model::Verification, id).await.unwrap().is_none());
    let account = super::super::find_credential(&mut tx, id)
        .await
        .unwrap()
        .unwrap();
    assert!(crate::credentials::verify_password(
        "new-observer-password",
        account["password"].as_str().unwrap()
    ));
    assert_eq!(
        tx.find_page(Model::Session, "userId", id, None, 256)
            .await
            .unwrap()
            .records
            .len(),
        if retained { 2 } else { 0 }
    );
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn reset_observer_order_failure_reentry_and_projection_boundaries() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
#[tokio::test]
async fn reset_observer_cancellation_preserves_committed_password_and_sessions() {
    let store = Arc::new(MemoryAuthStore::default());
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let id = "reset-observer-cancel";
    super::super::reset::revocation_tests::seed(store.as_ref(), id, now, 2).await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let service = CredentialService::new(store.clone())
        .with_revoke_sessions_on_password_reset(true)
        .with_password_reset_observer(Arc::new(Observer {
            store: store.clone(),
            mode: "pending",
            calls: Arc::default(),
            entered: entered.clone(),
        }));
    let task = tokio::spawn(async move {
        service
            .reset_password(id, "new-observer-password".into(), now)
            .await
    });
    entered.notified().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    check(store.as_ref(), id, true).await;
}
