use super::*;
use crate::accounts::EmailVerificationDeliveryContext;
use crate::store::{AuthCursor, AuthPage, AuthWindow, memory::MemoryAuthStore};
use crate::store::{AuthTransaction, Model};
#[path = "fault_store.rs"]
mod fault_store;
struct Pending {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
impl EmailVerificationDelivery for Pending {
    fn prepare(
        &self,
        _: EmailVerificationDeliveryContext,
    ) -> BlueprintResult<crate::background::DeliveryFuture> {
        let entered = self.entered.clone();
        let release = self.release.clone();
        Ok(Box::pin(async move {
            entered.notify_one();
            release.notified().await;
            Ok(())
        }))
    }
}
fn pending_service<S: AuthStore>(store: Arc<S>, hook: Arc<Pending>) -> CredentialService<S> {
    let mut service =
        CredentialService::new(store).with_email_verification_policy(EmailVerificationPolicy {
            require: true,
            ..Default::default()
        });
    service.verification_delivery = Some(Delivery {
        hook,
        dispatcher: None,
        base: "http://localhost/api/auth".into(),
        secret: Zeroizing::new(vec![42; 32]),
    });
    service
}
fn input() -> SignupInput {
    serde_json::from_value(json!({"name":"Owned","firstName":"First","lastName":"Last","email":"cancel-policy@example.test","password":"owned-policy-password"})).unwrap()
}
#[tokio::test]
async fn verification_policy_cancel_reentry_duplicate_and_commit_rollback() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
    rollback().await;
}
pub(crate) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let hook = Arc::new(Pending {
        entered: Arc::new(tokio::sync::Notify::new()),
        release: Arc::new(tokio::sync::Notify::new()),
    });
    let service = pending_service(store.clone(), hook.clone());
    let task =
        tokio::spawn(async move { service.sign_up_outcome(input(), "", "", Utc::now()).await });
    hook.entered.notified().await;
    let mut tx = tokio::time::timeout(std::time::Duration::from_secs(1), store.begin(true))
        .await
        .unwrap()
        .unwrap();
    assert!(
        tx.find_unique(Model::User, "email", "cancel-policy@example.test")
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
    task.abort();
    assert!(task.await.err().unwrap().is_cancelled());
    let service = pending_service(store.clone(), hook.clone());
    let task =
        tokio::spawn(async move { service.sign_up_outcome(input(), "", "", Utc::now()).await });
    hook.entered.notified().await;
    let winner = CredentialService::new(store.clone())
        .sign_up(input(), "", "", Utc::now())
        .await
        .unwrap();
    hook.release.notify_one();
    let outcome = task.await.unwrap().unwrap().body();
    assert!(outcome["token"].is_null());
    assert_ne!(outcome["user"]["id"], winner.user["id"]);
}
async fn rollback() {
    let empty = Arc::new(MemoryAuthStore::default());
    let failing = Arc::new(fault_store::FaultStore {
        store: empty.clone(),
        read: None,
        page: None,
        delete: None,
        insert: None,
        begin: false,
        commit: true,
    });
    let service =
        CredentialService::new(failing).with_email_verification_policy(EmailVerificationPolicy {
            require: true,
            ..Default::default()
        });
    assert!(
        service
            .sign_up_outcome(input(), "", "", Utc::now())
            .await
            .is_err()
    );
    let mut tx = empty.begin(false).await.unwrap();
    assert!(
        tx.find_unique(Model::User, "email", "cancel-policy@example.test")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tx.list_page(Model::Account, None, 10)
            .await
            .unwrap()
            .records
            .is_empty()
    );
    tx.commit().await.unwrap();
}
