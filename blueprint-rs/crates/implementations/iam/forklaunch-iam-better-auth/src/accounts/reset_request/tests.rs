use super::*;
use crate::{
    accounts::CredentialService,
    store::{AuthCursor, AuthPage, AuthWindow, memory::MemoryAuthStore},
};
use std::sync::{
    Mutex,
    atomic::{AtomicU8, Ordering},
};
#[allow(clippy::duplicate_mod)]
#[path = "../../http/observer/fault_store.rs"]
mod fault_store;
#[derive(Default)]
pub(crate) struct Delivery {
    pub contexts: Mutex<Vec<PasswordResetDeliveryContext>>,
    pub mode: AtomicU8,
    pub entered: Arc<tokio::sync::Notify>,
    pub release: Arc<tokio::sync::Notify>,
}
impl PasswordResetDelivery for Delivery {
    fn prepare(&self, context: PasswordResetDeliveryContext) -> BlueprintResult<DeliveryFuture> {
        assert!(context.user().get("password").is_none());
        assert!(context.user().get("session").is_none());
        self.contexts.lock().unwrap().push(context);
        let mode = self.mode.load(Ordering::SeqCst);
        if mode == 1 {
            return Err(BlueprintError::new("private", "secret"));
        }
        let entered = self.entered.clone();
        let release = self.release.clone();
        Ok(Box::pin(async move {
            if mode == 3 {
                entered.notify_one();
                release.notified().await;
            }
            if mode == 2 {
                return Err(BlueprintError::new("private", "secret"));
            }
            Ok(())
        }))
    }
}
pub(crate) fn service<S: AuthStore>(
    store: Arc<S>,
    delivery: Arc<Delivery>,
) -> PasswordResetService<S> {
    PasswordResetService::new(
        store,
        "http://localhost:3000/api/auth".into(),
        Arc::new(OriginPolicy::new("http://localhost:3000", vec![]).unwrap()),
        delivery,
    )
    .unwrap()
}
async fn seed<S: AuthStore>(store: &S) {
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User,json!({"id":"reset-delivery-user","email":"reset-delivery@example.test","name":"Owned","emailVerified":false,"password":"private-not-public","session":{"token":"private"}})).await.unwrap();
    tx.commit().await.unwrap();
}
pub(crate) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    seed(store.as_ref()).await;
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let delivery = Arc::new(Delivery::default());
    let sender = service(store.clone(), delivery.clone());
    let missing = sender
        .request("missing-reset@example.test", "", now)
        .await
        .unwrap();
    assert!(delivery.contexts.lock().unwrap().is_empty());
    let response = sender
        .request("RESET-DELIVERY@EXAMPLE.TEST", "/owned-reset?x=! 'é", now)
        .await
        .unwrap();
    assert_eq!(response, missing);
    let (proof, url) = {
        let contexts = delivery.contexts.lock().unwrap();
        let context = &contexts[0];
        (context.token().to_owned(), context.url().to_owned())
    };
    assert_eq!(proof.len(), 24);
    assert!(proof.bytes().all(|b| b.is_ascii_alphanumeric()));
    assert_eq!(
        url,
        format!(
            "http://localhost:3000/api/auth/reset-password/{proof}?callbackURL=%2Fowned-reset%3Fx%3D!%20'%C3%A9"
        )
    );
    let mut tx = store.begin(false).await.unwrap();
    let record = tx
        .find_page(
            Model::Verification,
            "identifier",
            &format!("reset-password:{proof}"),
            None,
            1,
        )
        .await
        .unwrap()
        .records
        .remove(0);
    assert_eq!(record["value"], "reset-delivery-user");
    assert_eq!(
        record["expiresAt"],
        (now + chrono::Duration::hours(1)).to_rfc3339_opts(SecondsFormat::Millis, true)
    );
    tx.commit().await.unwrap();
    CredentialService::new(store.clone())
        .reset_password(
            &proof,
            "owned-reset-password".into(),
            now + chrono::Duration::hours(1),
        )
        .await
        .unwrap();
    assert_eq!(
        CredentialService::new(store.clone())
            .reset_password(&proof, "owned-reset-password".into(), now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_TOKEN"
    );
    assert_eq!(
        sender
            .request("reset-delivery@example.test", "https://evil.example/", now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_REDIRECT_URL"
    );
    assert_eq!(delivery.contexts.lock().unwrap().len(), 1);
    for mode in [1, 2] {
        delivery.mode.store(mode, Ordering::SeqCst);
        let result = sender.request("reset-delivery@example.test", "", now).await;
        if mode == 1 {
            assert_eq!(
                result.err().unwrap().code,
                "PASSWORD_RESET_DELIVERY_PREPARATION_FAILED"
            );
        } else {
            assert_eq!(result.unwrap(), success());
        }
    }
    assert_eq!(delivery.contexts.lock().unwrap().len(), 3);
}
#[tokio::test]
async fn reset_delivery_lifecycle_existing_consume_compatibility() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
#[tokio::test]
async fn reset_delivery_rollback_and_cancellation_boundaries() {
    let store = Arc::new(MemoryAuthStore::default());
    seed(store.as_ref()).await;
    let now = Utc::now();
    let delivery = Arc::new(Delivery::default());
    let failed = Arc::new(fault_store::FaultStore {
        store: store.clone(),
        read: None,
        page: None,
        delete: None,
        insert: None,
        begin: false,
        commit: true,
    });
    assert!(
        service(failed, delivery.clone())
            .request("reset-delivery@example.test", "", now)
            .await
            .is_err()
    );
    assert!(delivery.contexts.lock().unwrap().is_empty());
    let lock = store.begin(true).await.unwrap();
    let sender = service(store.clone(), delivery.clone());
    let pending =
        tokio::spawn(async move { sender.request("reset-delivery@example.test", "", now).await });
    tokio::task::yield_now().await;
    pending.abort();
    assert!(pending.await.err().unwrap().is_cancelled());
    lock.commit().await.unwrap();
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.list(Model::Verification).await.unwrap().is_empty());
    tx.commit().await.unwrap();
    delivery.mode.store(3, Ordering::SeqCst);
    let sender = service(store.clone(), delivery.clone());
    let pending =
        tokio::spawn(async move { sender.request("reset-delivery@example.test", "", now).await });
    delivery.entered.notified().await;
    let mut tx = store.begin(true).await.unwrap();
    assert_eq!(tx.list(Model::Verification).await.unwrap().len(), 1);
    tx.commit().await.unwrap();
    pending.abort();
    assert!(pending.await.err().unwrap().is_cancelled());
    let proof = delivery.contexts.lock().unwrap()[0].token().to_owned();
    CredentialService::new(store)
        .reset_password(&proof, "owned-reset-password".into(), now)
        .await
        .unwrap();
}

#[tokio::test]
async fn reset_delivery_configuration_and_nested_projection_fail_before_callback() {
    let store = Arc::new(MemoryAuthStore::default());
    let delivery = Arc::new(Delivery::default());
    let policy = Arc::new(OriginPolicy::new("http://localhost:3000", vec![]).unwrap());
    for base in [
        "ftp://localhost/auth",
        "http://user:password@localhost/auth",
        "http://localhost/auth?secret=x",
        "http://localhost/auth#secret",
    ] {
        assert!(
            PasswordResetService::new(store.clone(), base.into(), policy.clone(), delivery.clone())
                .is_err()
        );
    }
    seed(store.as_ref()).await;
    let mut tx = store.begin(true).await.unwrap();
    let mut user = tx
        .get(Model::User, "reset-delivery-user")
        .await
        .unwrap()
        .unwrap();
    user["name"] = json!({"password":"private"});
    tx.replace(Model::User, user).await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        service(store.clone(), delivery.clone())
            .request("reset-delivery@example.test", "", Utc::now())
            .await
            .is_err()
    );
    assert!(delivery.contexts.lock().unwrap().is_empty());
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.list(Model::Verification).await.unwrap().is_empty());
    tx.commit().await.unwrap();
}
