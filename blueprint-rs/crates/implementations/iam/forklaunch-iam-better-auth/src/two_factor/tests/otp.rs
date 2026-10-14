use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicU8, Ordering},
};

#[derive(Default)]
pub(super) struct Delivery {
    pub codes: Mutex<Vec<String>>,
    pub mode: AtomicU8,
    pub entered: Arc<tokio::sync::Notify>,
    pub released: Arc<tokio::sync::Notify>,
}
impl OtpDelivery for Delivery {
    fn prepare(&self, context: OtpDeliveryContext) -> BlueprintResult<OtpDeliveryFuture> {
        assert!(context.user().get("token").is_none());
        assert_eq!(context.code().len(), 6);
        assert!(context.code().bytes().all(|value| value.is_ascii_digit()));
        self.codes.lock().unwrap().push(context.code().to_owned());
        let mode = self.mode.load(Ordering::SeqCst);
        if mode == 1 {
            return Err(error("private", "owned private preparation"));
        }
        let entered = self.entered.clone();
        let released = self.released.clone();
        Ok(Box::pin(async move {
            if mode == 2 {
                return Err(error("private", "owned private async failure"));
            }
            if mode == 3 {
                entered.notify_one();
                released.notified().await;
            }
            Ok(())
        }))
    }
}
impl Delivery {
    pub fn last(&self) -> String {
        self.codes.lock().unwrap().last().unwrap().clone()
    }
}
async fn challenge_for<S: AuthStore>(
    factor: &TwoFactorService<S>,
    user: &Value,
    now: DateTime<Utc>,
) -> TwoFactorChallenge {
    let mut tx = factor.store.begin(true).await.unwrap();
    let result = factor
        .gate(&mut tx, user, None, true, "", "", now)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    match result {
        signin::SigninGate::Challenge(value) => value,
        signin::SigninGate::Trusted(_) => panic!("no trust supplied"),
    }
}
#[allow(clippy::too_many_lines)]
pub(super) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let mut user = json!({"id":"otp-only-user","email":"otp-only@example.test","emailVerified":false,"name":"OTP"});
    let initial = new_session("otp-only-user", "", "", now).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User, user.clone()).await.unwrap();
    tx.insert(Model::Session, initial.clone()).await.unwrap();
    tx.commit().await.unwrap();
    let delivery = Arc::new(Delivery::default());
    let factor = Arc::new(
        TwoFactorService::new(store.clone(), vec![42; 32], "Better Auth".into())
            .unwrap()
            .with_otp_delivery(delivery.clone()),
    );
    let token = text(&initial, "token").unwrap();
    factor.send_otp(token, "", now).await.unwrap();
    let valid = delivery.last();
    let failed = TwoFactorService::new(
        Arc::new(fault_store::FaultStore {
            store: store.clone(),
            read: None,
            page: None,
            delete: None,
            insert: None,
            begin: false,
            commit: true,
        }),
        vec![42; 32],
        "Better Auth".into(),
    )
    .unwrap()
    .with_otp_delivery(delivery.clone());
    assert!(
        failed
            .verify_otp(token, "", &valid, false, now)
            .await
            .is_err()
    );
    // Removing the delivery hook blocks new sends, not existing durable proof.
    let hook_off =
        TwoFactorService::new(store.clone(), vec![42; 32], "Better Auth".into()).unwrap();
    assert_eq!(
        hook_off.send_otp(token, "", now).await.err().unwrap().code,
        "OTP_NOT_CONFIGURED"
    );
    let enrolled = hook_off
        .verify_otp(token, "", &valid, false, now)
        .await
        .unwrap();
    assert_eq!(enrolled.body()["user"]["twoFactorEnabled"], true);
    assert_ne!(enrolled.body()["token"], initial["token"]);
    let active = enrolled.replacement_session_token().unwrap();
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.get(Model::Session, text(&initial, "id").unwrap())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tx.find_unique(Model::TwoFactor, "userId", "otp-only-user")
            .await
            .unwrap()
            .is_none()
    );
    user = tx.get(Model::User, "otp-only-user").await.unwrap().unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        factor
            .verify_otp(active, "", &valid, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "OTP_HAS_EXPIRED"
    );
    for mode in [1, 2] {
        delivery.mode.store(mode, Ordering::SeqCst);
        let result = factor.send_otp(active, "", now).await;
        assert_eq!(result.is_err(), mode == 1);
        if let Err(error) = result {
            assert_eq!(error.code, "TWO_FACTOR_DELIVERY_PREPARATION_FAILED");
            assert!(!error.message.contains("private"));
        }
        assert!(
            factor
                .verify_otp(active, "", &delivery.last(), false, now)
                .await
                .is_ok()
        );
    }
    delivery.mode.store(0, Ordering::SeqCst);
    let pending = challenge_for(&factor, &user, now).await;
    assert_eq!(pending.methods(), ["otp"]);
    factor
        .send_otp("", pending.identifier(), now)
        .await
        .unwrap();
    factor
        .send_otp("", pending.identifier(), now)
        .await
        .unwrap();
    let current = delivery.last();
    assert_eq!(
        factor
            .verify_otp(active, "", &current, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "OTP_HAS_EXPIRED"
    );
    for _ in 0..5 {
        assert_eq!(
            factor
                .verify_otp("", pending.identifier(), "invalid", false, now)
                .await
                .err()
                .unwrap()
                .code,
            "INVALID_CODE"
        );
    }
    assert_eq!(
        factor
            .verify_otp("", pending.identifier(), &current, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE"
    );
    assert_eq!(
        factor
            .verify_otp("", pending.identifier(), &current, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "OTP_HAS_EXPIRED"
    );
    factor
        .send_otp("", pending.identifier(), now)
        .await
        .unwrap();
    let exact = factor
        .verify_otp(
            "",
            pending.identifier(),
            &delivery.last(),
            true,
            now + Duration::seconds(180),
        )
        .await
        .unwrap();
    assert!(exact.trusted_device_value().is_some());
    let next = challenge_for(&factor, &user, now).await;
    factor.send_otp("", next.identifier(), now).await.unwrap();
    assert_eq!(
        factor
            .verify_otp(
                "",
                next.identifier(),
                &delivery.last(),
                false,
                now + Duration::milliseconds(180_001)
            )
            .await
            .err()
            .unwrap()
            .code,
        "OTP_HAS_EXPIRED"
    );
    factor.send_otp("", next.identifier(), now).await.unwrap();
    let code = delivery.last();
    let (a, b) = tokio::join!(
        factor.verify_otp("", next.identifier(), &code, false, now),
        factor.verify_otp("", next.identifier(), &code, false, now)
    );
    assert_ne!(a.is_ok(), b.is_ok());
    assert_eq!(
        a.err().or_else(|| b.err()).unwrap().code,
        "INVALID_TWO_FACTOR_COOKIE"
    );
    // Pending asynchronous delivery releases the store lock and never creates a session.
    delivery.mode.store(3, Ordering::SeqCst);
    let count = rows(store.as_ref(), Model::Session).await.len();
    let task_factor = factor.clone();
    let token = active.to_owned();
    let task = tokio::spawn(async move { task_factor.send_otp(&token, "", now).await });
    delivery.entered.notified().await;
    assert!(!task.is_finished());
    assert_eq!(rows(store.as_ref(), Model::Session).await.len(), count);
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    assert!(
        factor
            .verify_otp(active, "", &delivery.last(), false, now)
            .await
            .is_ok()
    );
    delivery.mode.store(0, Ordering::SeqCst);
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::TwoFactor,json!({"id":"otp-shared-lock","userId":"otp-only-user","secret":STANDARD.encode([7u8;32]),"backupCodes":[],"verified":true,"failedVerificationCount":9,"lockedUntil":null})).await.unwrap();
    tx.commit().await.unwrap();
    let shared = challenge_for(&factor, &user, now).await;
    assert_eq!(shared.methods(), ["totp", "otp"]);
    factor.send_otp("", shared.identifier(), now).await.unwrap();
    assert_eq!(
        factor
            .verify_otp("", shared.identifier(), "invalid", false, now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_CODE"
    );
    assert_eq!(
        factor
            .verify_otp("", shared.identifier(), &delivery.last(), false, now)
            .await
            .err()
            .unwrap()
            .code,
        "ACCOUNT_TEMPORARILY_LOCKED"
    );
    let later = now + Duration::seconds(901);
    let unlocked = challenge_for(&factor, &user, later).await;
    assert_eq!(
        factor
            .verify_otp("", unlocked.identifier(), "123456", false, later)
            .await
            .err()
            .unwrap()
            .code,
        "OTP_HAS_EXPIRED"
    );
    let mut tx = store.begin(false).await.unwrap();
    let row = tx
        .get(Model::TwoFactor, "otp-shared-lock")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row["failedVerificationCount"], 0);
    assert!(row["lockedUntil"].is_null());
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn delivered_otp_atomic_lifecycle_and_callbacks() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
