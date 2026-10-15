use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicU8, AtomicUsize, Ordering},
};
#[derive(Default)]
pub(super) struct Queue {
    pub tasks: Mutex<Vec<OtpBackgroundTask>>,
    pub calls: AtomicUsize,
    pub mode: AtomicU8,
}
impl OtpBackgroundDispatcher for Queue {
    fn dispatch(&self, task: OtpBackgroundTask) -> BlueprintResult<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.mode.load(Ordering::SeqCst) == 1 {
            return Err(error("secret-queue-error", "private"));
        }
        self.tasks.lock().unwrap().push(task);
        Ok(())
    }
}
async fn setup() -> (Arc<MemoryAuthStore>, String, DateTime<Utc>) {
    let store = Arc::new(MemoryAuthStore::default());
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let session = new_session("background-user", "", "", now).unwrap();
    let token = text(&session, "token").unwrap().to_owned();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User,json!({"id":"background-user","email":"background@example.test","emailVerified":false,"name":"Owned"})).await.unwrap();
    tx.insert(Model::Session, session).await.unwrap();
    tx.commit().await.unwrap();
    (store, token, now)
}
#[tokio::test]
async fn otp_background_default_await_and_owned_lazy_handoff() {
    let (store, token, now) = setup().await;
    let delivery = Arc::new(otp::Delivery::default());
    delivery.mode.store(3, Ordering::SeqCst);
    let factor = Arc::new(
        TwoFactorService::new(store.clone(), vec![42; 32], "Owned".into())
            .unwrap()
            .with_otp_delivery(delivery.clone()),
    );
    let service = factor.clone();
    let credential = token.clone();
    let default = tokio::spawn(async move { service.send_otp(&credential, "", now).await });
    delivery.entered.notified().await;
    assert!(!default.is_finished());
    // A writer can acquire while delivery waits: persisted proof and no transaction held.
    let mut tx = store.begin(true).await.unwrap();
    assert!(!tx.list(Model::Verification).await.unwrap().is_empty());
    tx.commit().await.unwrap();
    delivery.released.notify_one();
    default.await.unwrap().unwrap();
    let queue = Arc::new(Queue::default());
    let background = TwoFactorService::new(store.clone(), vec![42; 32], "Owned".into())
        .unwrap()
        .with_otp_delivery(delivery.clone())
        .with_otp_background_dispatcher(queue.clone())
        .with_otp_options(OtpOptions::default());
    assert_eq!(
        background.send_otp(&token, "", now).await.unwrap().body,
        json!({"status":true})
    );
    assert_eq!(queue.calls.load(Ordering::SeqCst), 1);
    let task = queue.tasks.lock().unwrap().pop().unwrap();
    let tx = store.begin(true).await.unwrap();
    tx.commit().await.unwrap();
    let worker = tokio::spawn(task);
    delivery.entered.notified().await;
    assert!(!worker.is_finished());
    delivery.released.notify_one();
    worker.await.unwrap();
    #[cfg(feature = "http")]
    assert!(
        background
            .copy_configuration()
            .otp_background_dispatcher
            .is_some()
    );
}
#[tokio::test]
async fn otp_background_preparation_dispatch_delivery_errors_and_drop() {
    for mode in ["prepare", "reject", "delivery", "drop"] {
        let (store, token, now) = setup().await;
        let delivery = Arc::new(otp::Delivery::default());
        let queue = Arc::new(Queue::default());
        if mode == "prepare" {
            delivery.mode.store(1, Ordering::SeqCst);
        }
        if mode == "reject" {
            queue.mode.store(1, Ordering::SeqCst);
        }
        if mode == "delivery" {
            delivery.mode.store(2, Ordering::SeqCst);
        }
        let factor = TwoFactorService::new(store.clone(), vec![42; 32], "Owned".into())
            .unwrap()
            .with_otp_delivery(delivery.clone())
            .with_otp_background_dispatcher(queue.clone());
        let result = factor.send_otp(&token, "", now).await;
        if mode == "prepare" {
            assert_eq!(
                result.err().unwrap().code,
                "TWO_FACTOR_DELIVERY_PREPARATION_FAILED"
            );
            assert_eq!(queue.calls.load(Ordering::SeqCst), 0);
        } else {
            assert_eq!(result.unwrap().body, json!({"status":true}));
            assert_eq!(queue.calls.load(Ordering::SeqCst), 1);
        }
        let task = queue.tasks.lock().unwrap().pop();
        if mode == "delivery" {
            task.unwrap().await;
        } else {
            drop(task);
        }
        // All failure paths preserve the already committed proof.
        factor
            .verify_otp(&token, "", &delivery.last(), false, now)
            .await
            .unwrap();
        let mut tx = store.begin(false).await.unwrap();
        assert_eq!(tx.list(Model::Session).await.unwrap().len(), 1);
        tx.commit().await.unwrap();
    }
}
