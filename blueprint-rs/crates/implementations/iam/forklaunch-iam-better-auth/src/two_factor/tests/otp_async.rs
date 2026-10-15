#![allow(clippy::items_after_statements)]
use super::*;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Default)]
struct Codec {
    mode: AtomicU8,
    calls: std::sync::atomic::AtomicUsize,
    entered: Arc<tokio::sync::Notify>,
    released: Arc<tokio::sync::Notify>,
}
impl OtpHashCodec for Codec {
    fn hash(&self, input: OtpCodecText) -> OtpCodecFuture {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mode = self.mode.load(Ordering::SeqCst);
        let entered = self.entered.clone();
        let released = self.released.clone();
        Box::pin(async move {
            if mode == 1 {
                entered.notify_one();
                released.notified().await;
            }
            if mode == 2 {
                return Err(error("private_secret", "secret callback detail"));
            }
            OtpCodecText::new(format!("hash{}", input.as_str()))
        })
    }
}
fn service<S: AuthStore>(
    store: Arc<S>,
    codec: Arc<Codec>,
    delivery: Arc<otp::Delivery>,
) -> TwoFactorService<S> {
    TwoFactorService::new(store, vec![42; 32], "OTP".into())
        .unwrap()
        .with_otp_delivery(delivery)
        .with_otp_hash_codec(codec, OtpCodecOptions::default())
}
async fn seed<S: AuthStore>(store: &S, now: DateTime<Utc>, id: &str) -> String {
    let session = new_session(id, "", "", now).unwrap();
    let token = text(&session, "token").unwrap().to_owned();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":id,"email":format!("{id}@example.test"),"twoFactorEnabled":true}),
    )
    .await
    .unwrap();
    tx.insert(Model::Session, session).await.unwrap();
    tx.commit().await.unwrap();
    token
}
#[test]
fn custom_otp_codec_bounds() {
    let mut row = json!({"id":"epoch-boundary", "identifier":"2fa-otp-epoch"});
    let state = super::super::otp_codec_state::Envelope::new(
        super::super::otp_codec_state::Phase::Active,
        0,
        Some(super::super::otp_codec_state::Proof {
            value: String::new(),
            attempts: 0,
            expires_ms: -1,
            custom: true,
        }),
    );
    state.write(&mut row).unwrap();
    assert_eq!(time(&row, "expiresAt").unwrap().timestamp_millis(), -1);

    for ms in [1, 30_000] {
        assert!(OtpCodecOptions::new(std::time::Duration::from_millis(ms)).is_ok());
    }
    for ms in [0, 30_001] {
        assert!(OtpCodecOptions::new(std::time::Duration::from_millis(ms)).is_err());
    }
    assert!(OtpCodecOptions::new(std::time::Duration::from_nanos(1_000_001)).is_err());
    assert!(OtpCodecText::new(String::new()).is_ok());
    assert!(OtpCodecText::new("é".repeat(512)).is_ok());
    assert!(OtpCodecText::new("é".repeat(513)).is_err());
}
#[tokio::test]
async fn custom_otp_codec_one_use_failure_restore_and_default_closed() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
pub(super) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let token = seed(store.as_ref(), now, "custom-codec-user").await;
    let codec = Arc::new(Codec::default());
    let delivery = Arc::new(otp::Delivery::default());
    let factor = Arc::new(service(store.clone(), codec.clone(), delivery.clone()));
    factor.send_otp(&token, "", now).await.unwrap();
    let code = delivery.last();
    let plain = TwoFactorService::new(store.clone(), vec![42; 32], "OTP".into()).unwrap();
    assert_eq!(
        plain
            .verify_otp(&token, "", &code, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "TWO_FACTOR_CUSTOM_CODEC_FAILED"
    );
    codec.mode.store(2, Ordering::SeqCst);
    assert_eq!(
        factor.send_otp(&token, "", now).await.err().unwrap().code,
        "TWO_FACTOR_CUSTOM_CODEC_FAILED"
    );
    codec.mode.store(0, Ordering::SeqCst);
    factor
        .verify_otp(&token, "", &code, false, now)
        .await
        .unwrap();
    assert_eq!(
        factor
            .verify_otp(&token, "", &code, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "OTP_HAS_EXPIRED"
    );
    factor.send_otp(&token, "", now).await.unwrap();
    let code = delivery.last();
    codec.mode.store(2, Ordering::SeqCst);
    let error = factor
        .verify_otp(&token, "", &code, false, now)
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "TWO_FACTOR_CUSTOM_CODEC_FAILED");
    assert!(!format!("{error:?}").contains("private_secret"));
    codec.mode.store(0, Ordering::SeqCst);
    assert_eq!(
        factor
            .verify_otp(&token, "", &code, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "OTP_HAS_EXPIRED"
    );
    factor.send_otp(&token, "", now).await.unwrap();
    let code = delivery.last();
    let a = factor.clone();
    let b = factor.clone();
    let ta = token.clone();
    let tb = token.clone();
    let ca = code.clone();
    let (first, second) = tokio::join!(
        a.verify_otp(&ta, "", &ca, false, now),
        b.verify_otp(&tb, "", &code, false, now)
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
}
#[tokio::test]
async fn custom_otp_codec_delayed_verify_cannot_resurrect_consumed_resend() {
    let store = Arc::new(MemoryAuthStore::default());
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let token = seed(store.as_ref(), now, "resend-codec-user").await;
    let codec = Arc::new(Codec::default());
    let delivery = Arc::new(otp::Delivery::default());
    let factor = Arc::new(service(store, codec.clone(), delivery.clone()));
    factor.send_otp(&token, "", now).await.unwrap();
    let code = delivery.last();
    codec.mode.store(1, Ordering::SeqCst);
    let pending = {
        let factor = factor.clone();
        let token = token.clone();
        tokio::spawn(async move { factor.verify_otp(&token, "", &code, false, now).await })
    };
    codec.entered.notified().await;
    codec.mode.store(0, Ordering::SeqCst);
    factor.send_otp(&token, "", now).await.unwrap();
    let replacement = delivery.last();
    factor
        .verify_otp(&token, "", &replacement, false, now)
        .await
        .unwrap();
    codec.released.notify_one();
    assert!(pending.await.unwrap().is_err());
    assert!(
        factor
            .verify_otp(&token, "", &replacement, false, now)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn custom_otp_codec_cancel_restart_cleanup_and_expiry() {
    let store = Arc::new(MemoryAuthStore::default());
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let token = seed(store.as_ref(), now, "cancel-codec-user").await;
    let codec = Arc::new(Codec::default());
    let delivery = Arc::new(otp::Delivery::default());
    let factor = Arc::new(service(store.clone(), codec.clone(), delivery.clone()));
    factor.send_otp(&token, "", now).await.unwrap();
    let code = delivery.last();
    codec.mode.store(1, Ordering::SeqCst);
    let pending = {
        let factor = factor.clone();
        let token = token.clone();
        tokio::spawn(async move { factor.send_otp(&token, "", now).await })
    };
    codec.entered.notified().await;
    pending.abort();
    let _ = pending.await;
    let restarted = service(store.clone(), Arc::new(Codec::default()), delivery.clone());
    let later = now + Duration::seconds(31);
    let cleanup = restarted
        .cleanup_otp_codec_states(None, later)
        .await
        .unwrap();
    assert_eq!(cleanup.removed, 0);
    restarted
        .verify_otp(&token, "", &code, false, later)
        .await
        .unwrap();
    let cleanup = restarted
        .cleanup_otp_codec_states(None, later + Duration::seconds(31))
        .await
        .unwrap();
    assert_eq!(cleanup.removed, 1);
    assert!(
        restarted
            .verify_otp(&token, "", &code, false, later)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn custom_otp_codec_fresh_authority_expiry_and_no_hook_pending() {
    for action in ["signout", "expiry", "factor-disable", "no-hook"] {
        let store = Arc::new(MemoryAuthStore::default());
        let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        let token = seed(store.as_ref(), now, "authority-codec-user").await;
        let codec = Arc::new(Codec::default());
        let delivery = Arc::new(otp::Delivery::default());
        let factor = Arc::new(service(store.clone(), codec.clone(), delivery.clone()));
        factor.send_otp(&token, "", now).await.unwrap();
        let code = delivery.last();
        codec.mode.store(1, Ordering::SeqCst);
        let clock = Arc::new(std::sync::atomic::AtomicI64::new(now.timestamp_millis()));
        let pending = {
            let factor = factor.clone();
            let token = token.clone();
            let code = code.clone();
            let clock = clock.clone();
            tokio::spawn(async move {
                factor
                    .verify_otp_with_clock(&token, "", &code, false, &move || {
                        DateTime::from_timestamp_millis(clock.load(Ordering::SeqCst)).unwrap()
                    })
                    .await
            })
        };
        codec.entered.notified().await;
        if action == "signout" {
            let mut tx = store.begin(true).await.unwrap();
            let session = tx
                .find_unique(Model::Session, "token", &token)
                .await
                .unwrap()
                .unwrap();
            tx.delete(Model::Session, text(&session, "id").unwrap())
                .await
                .unwrap();
            tx.commit().await.unwrap();
        } else if action == "factor-disable" {
            let mut tx = store.begin(true).await.unwrap();
            let mut user = tx
                .get(Model::User, "authority-codec-user")
                .await
                .unwrap()
                .unwrap();
            user["twoFactorEnabled"] = json!(false);
            tx.replace(Model::User, user).await.unwrap();
            tx.commit().await.unwrap();
        } else if action == "expiry" {
            clock.store(now.timestamp_millis() + 180_001, Ordering::SeqCst);
        } else {
            let plain = TwoFactorService::new(store.clone(), vec![42; 32], "OTP".into()).unwrap();
            assert_eq!(
                plain
                    .verify_otp(&token, "", &code, false, now)
                    .await
                    .err()
                    .unwrap()
                    .code,
                "TWO_FACTOR_CUSTOM_CODEC_FAILED"
            );
        }
        codec.released.notify_one();
        let result = pending.await.unwrap();
        assert_eq!(result.is_ok(), action == "no-hook");
        assert_eq!(
            rows(store.as_ref(), Model::Session).await.len(),
            usize::from(action != "signout")
        );
    }
}

struct Reentrant(Arc<MemoryAuthStore>);
impl OtpHashCodec for Reentrant {
    fn hash(&self, input: OtpCodecText) -> OtpCodecFuture {
        let store = self.0.clone();
        Box::pin(async move {
            let mut tx = store.begin(true).await?;
            let _ = tx.list(Model::Verification).await?;
            tx.commit().await?;
            OtpCodecText::new(format!("hash{}", input.as_str()))
        })
    }
}
#[tokio::test]
async fn custom_otp_codec_reentry_timeout_and_delimiter_failure() {
    let store = Arc::new(MemoryAuthStore::default());
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let token = seed(store.as_ref(), now, "reentry-codec-user").await;
    let delivery = Arc::new(otp::Delivery::default());
    let factor = TwoFactorService::new(store.clone(), vec![42; 32], "OTP".into())
        .unwrap()
        .with_otp_delivery(delivery.clone())
        .with_otp_hash_codec(
            Arc::new(Reentrant(store.clone())),
            OtpCodecOptions::new(std::time::Duration::from_secs(1)).unwrap(),
        );
    factor.send_otp(&token, "", now).await.unwrap();
    let code = delivery.last();
    factor
        .verify_otp(&token, "", &code, false, now)
        .await
        .unwrap();
    let codec = Arc::new(Codec::default());
    codec.mode.store(1, Ordering::SeqCst);
    let factor = service(store.clone(), codec.clone(), delivery).with_otp_hash_codec(
        codec.clone(),
        OtpCodecOptions::new(std::time::Duration::from_millis(1)).unwrap(),
    );
    assert_eq!(
        factor
            .send_otp_with_clock(&token, "", &move || now)
            .await
            .err()
            .unwrap()
            .code,
        "TWO_FACTOR_CUSTOM_CODEC_FAILED"
    );
    assert_eq!(codec.calls.load(Ordering::SeqCst), 1);
    struct Colon;
    impl OtpHashCodec for Colon {
        fn hash(&self, _: OtpCodecText) -> OtpCodecFuture {
            Box::pin(async { OtpCodecText::new("invalid:format".into()) })
        }
    }
    let factor = factor.with_otp_hash_codec(Arc::new(Colon), OtpCodecOptions::default());
    assert_eq!(
        factor.send_otp(&token, "", now).await.err().unwrap().code,
        "TWO_FACTOR_CUSTOM_CODEC_FAILED"
    );
}

struct Encryption;
impl OtpEncryptionCodec for Encryption {
    fn encrypt(&self, input: OtpCodecText) -> OtpCodecFuture {
        Box::pin(async move { OtpCodecText::new(format!("🔐{}", input.as_str())) })
    }
    fn decrypt(&self, input: OtpCodecText) -> OtpCodecFuture {
        Box::pin(async move {
            OtpCodecText::new(
                input
                    .as_str()
                    .strip_prefix('🔐')
                    .ok_or_else(|| error("private", "private decrypt failure"))?
                    .to_owned(),
            )
        })
    }
}
#[tokio::test]
async fn custom_otp_codec_encryption_unicode_and_empty_hash() {
    let store = Arc::new(MemoryAuthStore::default());
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let token = seed(store.as_ref(), now, "encryption-codec-user").await;
    let delivery = Arc::new(otp::Delivery::default());
    let factor = TwoFactorService::new(store, vec![42; 32], "OTP".into())
        .unwrap()
        .with_otp_delivery(delivery.clone())
        .with_otp_encryption_codec(Arc::new(Encryption), OtpCodecOptions::default());
    factor.send_otp(&token, "", now).await.unwrap();
    let code = delivery.last();
    assert!(
        factor
            .verify_otp(&token, "", "é", false, now)
            .await
            .is_err()
    );
    factor
        .verify_otp(&token, "", &code, false, now)
        .await
        .unwrap();
    struct Empty;
    impl OtpHashCodec for Empty {
        fn hash(&self, _: OtpCodecText) -> OtpCodecFuture {
            Box::pin(async { OtpCodecText::new(String::new()) })
        }
    }
    let factor = factor.with_otp_hash_codec(Arc::new(Empty), OtpCodecOptions::default());
    factor.send_otp(&token, "", now).await.unwrap();
    factor.verify_otp(&token, "", "", false, now).await.unwrap();
}

#[tokio::test]
async fn custom_otp_codec_final_commit_rollback_and_current_pending_lockout() {
    rollback(Arc::new(MemoryAuthStore::default())).await;
}
#[allow(clippy::too_many_lines)]
pub(super) async fn rollback<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let token = seed(store.as_ref(), now, "rollback-codec-user").await;
    let codec = Arc::new(Codec::default());
    let delivery = Arc::new(otp::Delivery::default());
    let commit = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let wrapped = Arc::new(super::otp_commit_store::FaultStore {
        store: store.clone(),
        read: None,
        page: None,
        delete: None,
        insert: None,
        begin: false,
        commit: commit.clone(),
        advance_on_session: None,
        advance_on_verification: None,
        advance_on_commit: None,
    });
    let factor = Arc::new(service(wrapped, codec.clone(), delivery.clone()));
    let mut tx = store.begin(false).await.unwrap();
    let user = tx
        .get(Model::User, "rollback-codec-user")
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = store.begin(true).await.unwrap();
    let gate = factor
        .gate(&mut tx, &user, None, true, "", "", now)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let signin::SigninGate::Challenge(challenge) = gate else {
        panic!("challenge required")
    };
    factor
        .send_otp("", challenge.identifier(), now)
        .await
        .unwrap();
    let code = delivery.last();
    codec.mode.store(1, Ordering::SeqCst);
    let identifier = challenge.identifier().to_owned();
    let pending = {
        let factor = factor.clone();
        let identifier = identifier.clone();
        tokio::spawn(async move { factor.verify_otp("", &identifier, &code, true, now).await })
    };
    codec.entered.notified().await;
    commit.store(true, Ordering::SeqCst);
    codec.released.notify_one();
    assert!(pending.await.unwrap().is_err());
    commit.store(false, Ordering::SeqCst);
    assert_eq!(
        rows(store.as_ref(), Model::Session)
            .await
            .iter()
            .filter(|row| row["userId"] == "rollback-codec-user")
            .count(),
        1
    );
    assert!(
        rows(store.as_ref(), Model::Verification)
            .await
            .iter()
            .any(|row| row["identifier"] == identifier)
    );
    assert!(
        !rows(store.as_ref(), Model::Verification)
            .await
            .iter()
            .any(|row| row["value"] == "rollback-codec-user"
                && row["identifier"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("trust-device-"))
    );
    codec.mode.store(0, Ordering::SeqCst);
    factor.send_otp("", &identifier, now).await.unwrap();
    let code = delivery.last();
    codec.mode.store(1, Ordering::SeqCst);
    let pending = {
        let factor = factor.clone();
        let identifier = identifier.clone();
        tokio::spawn(async move { factor.verify_otp("", &identifier, &code, true, now).await })
    };
    codec.entered.notified().await;
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::TwoFactor,json!({"id":"codec-locked-factor","userId":"rollback-codec-user","secret":"unused","backupCodes":[],"failedVerificationCount":10,"lockedUntil":timestamp(now+Duration::seconds(60))})).await.unwrap();
    tx.commit().await.unwrap();
    codec.released.notify_one();
    assert_eq!(
        pending.await.unwrap().err().unwrap().code,
        "ACCOUNT_TEMPORARILY_LOCKED"
    );
    assert_eq!(
        rows(store.as_ref(), Model::Session)
            .await
            .iter()
            .filter(|row| row["userId"] == "rollback-codec-user")
            .count(),
        1
    );
    // Explicit authority revocation after encode began cannot publish or deliver a new proof.
    codec.mode.store(1, Ordering::SeqCst);
    let before = delivery.codes.lock().unwrap().len();
    let pending = {
        let factor = factor.clone();
        let token = token.clone();
        tokio::spawn(async move { factor.send_otp(&token, "", now).await })
    };
    codec.entered.notified().await;
    let mut tx = store.begin(true).await.unwrap();
    let session = tx
        .find_unique(Model::Session, "token", &token)
        .await
        .unwrap()
        .unwrap();
    tx.delete(Model::Session, text(&session, "id").unwrap())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    codec.released.notify_one();
    assert!(pending.await.unwrap().is_err());
    assert_eq!(delivery.codes.lock().unwrap().len(), before);
}

#[tokio::test]
async fn custom_otp_codec_samples_clock_after_waiting_for_store_lock() {
    let store = Arc::new(MemoryAuthStore::default());
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let token = seed(store.as_ref(), now, "clock-lock-user").await;
    let codec = Arc::new(Codec::default());
    let delivery = Arc::new(otp::Delivery::default());
    let factor = Arc::new(service(store.clone(), codec.clone(), delivery.clone()));
    factor.send_otp(&token, "", now).await.unwrap();
    let code = delivery.last();
    codec.mode.store(1, Ordering::SeqCst);
    let clock = Arc::new(std::sync::atomic::AtomicI64::new(now.timestamp_millis()));
    let pending = {
        let factor = factor.clone();
        let token = token.clone();
        let clock = clock.clone();
        tokio::spawn(async move {
            factor
                .verify_otp_with_clock(&token, "", &code, false, &move || {
                    DateTime::from_timestamp_millis(clock.load(Ordering::SeqCst)).unwrap()
                })
                .await
        })
    };
    codec.entered.notified().await;
    let tx = store.begin(true).await.unwrap();
    codec.released.notify_one();
    // Yield while the final transaction is blocked behind this writer.
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    clock.store(now.timestamp_millis() + 30_001, Ordering::SeqCst);
    tx.commit().await.unwrap();
    assert_eq!(
        pending.await.unwrap().err().unwrap().code,
        "OTP_HAS_EXPIRED"
    );
}

#[tokio::test]
async fn custom_otp_codec_expiry_during_session_write_rolls_back() {
    late_commit(Arc::new(MemoryAuthStore::default())).await;
}
pub(super) async fn late_commit<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let _ = seed(store.as_ref(), now, "late-codec-user").await;
    let clock = Arc::new(std::sync::atomic::AtomicI64::new(now.timestamp_millis()));
    let wrapped = Arc::new(super::otp_commit_store::FaultStore {
        store: store.clone(),
        read: None,
        page: None,
        delete: None,
        insert: None,
        begin: false,
        commit: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        advance_on_session: Some((clock.clone(), now.timestamp_millis() + 30_001)),
        advance_on_verification: None,
        advance_on_commit: None,
    });
    let codec = Arc::new(Codec::default());
    let delivery = Arc::new(otp::Delivery::default());
    let factor = service(wrapped, codec, delivery.clone());
    let mut tx = store.begin(true).await.unwrap();
    let user = tx
        .get(Model::User, "late-codec-user")
        .await
        .unwrap()
        .unwrap();
    let gate = factor
        .gate(&mut tx, &user, None, true, "", "", now)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let signin::SigninGate::Challenge(challenge) = gate else {
        panic!("challenge required")
    };
    factor
        .send_otp("", challenge.identifier(), now)
        .await
        .unwrap();
    let result = factor
        .verify_otp_with_clock(
            "",
            challenge.identifier(),
            &delivery.last(),
            true,
            &move || DateTime::from_timestamp_millis(clock.load(Ordering::SeqCst)).unwrap(),
        )
        .await;
    assert_eq!(result.err().unwrap().code, "OTP_HAS_EXPIRED");
    assert_eq!(
        rows(store.as_ref(), Model::Session)
            .await
            .iter()
            .filter(|row| row["userId"] == "late-codec-user")
            .count(),
        1
    );
    let records = rows(store.as_ref(), Model::Verification).await;
    assert!(
        records
            .iter()
            .any(|row| row["identifier"] == challenge.identifier())
    );
    assert!(!records.iter().any(|row| {
        row["value"] == "late-codec-user"
            && row["identifier"]
                .as_str()
                .unwrap_or("")
                .starts_with("trust-device-")
    }));
}

#[tokio::test]
async fn custom_otp_codec_superseded_send_and_cancelled_verifier_stay_consumed() {
    let store = Arc::new(MemoryAuthStore::default());
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let token = seed(store.as_ref(), now, "cancelled-verifier-user").await;
    let codec = Arc::new(Codec::default());
    let delivery = Arc::new(otp::Delivery::default());
    let factor = Arc::new(service(store.clone(), codec.clone(), delivery.clone()));
    codec.mode.store(1, Ordering::SeqCst);
    let old = {
        let factor = factor.clone();
        let token = token.clone();
        tokio::spawn(async move { factor.send_otp(&token, "", now).await })
    };
    codec.entered.notified().await;
    codec.mode.store(0, Ordering::SeqCst);
    factor.send_otp(&token, "", now).await.unwrap();
    let code = delivery.last();
    factor
        .verify_otp(&token, "", &code, false, now)
        .await
        .unwrap();
    codec.released.notify_one();
    assert!(old.await.unwrap().is_err());
    assert_eq!(delivery.codes.lock().unwrap().len(), 1);
    factor.send_otp(&token, "", now).await.unwrap();
    let code = delivery.last();
    codec.mode.store(1, Ordering::SeqCst);
    let pending = {
        let factor = factor.clone();
        let token = token.clone();
        let code = code.clone();
        tokio::spawn(async move { factor.verify_otp(&token, "", &code, false, now).await })
    };
    codec.entered.notified().await;
    pending.abort();
    let _ = pending.await;
    let restarted = service(store, Arc::new(Codec::default()), delivery);
    assert_eq!(
        restarted
            .cleanup_otp_codec_states(None, now + Duration::seconds(31))
            .await
            .unwrap()
            .removed,
        1
    );
    assert_eq!(
        restarted
            .verify_otp(&token, "", &code, false, now + Duration::seconds(31))
            .await
            .err()
            .unwrap()
            .code,
        "OTP_HAS_EXPIRED"
    );
}

#[tokio::test]
async fn custom_otp_codec_initial_contention_never_calls_expired_authority_callback() {
    for verify in [false, true] {
        let store = Arc::new(MemoryAuthStore::default());
        let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        let token = seed(store.as_ref(), now, "initial-clock-user").await;
        let codec = Arc::new(Codec::default());
        let delivery = Arc::new(otp::Delivery::default());
        let factor = Arc::new(service(store.clone(), codec.clone(), delivery.clone()));
        factor.send_otp(&token, "", now).await.unwrap();
        let code = delivery.last();
        codec.calls.store(0, Ordering::SeqCst);
        let clock = Arc::new(std::sync::atomic::AtomicI64::new(now.timestamp_millis()));
        let tx = store.begin(true).await.unwrap();
        let pending = {
            let factor = factor.clone();
            let token = token.clone();
            let clock = clock.clone();
            tokio::spawn(async move {
                let clock =
                    move || DateTime::from_timestamp_millis(clock.load(Ordering::SeqCst)).unwrap();
                if verify {
                    factor
                        .verify_otp_with_clock(&token, "", &code, false, &clock)
                        .await
                } else {
                    factor.send_otp_with_clock(&token, "", &clock).await
                }
            })
        };
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        clock.store(
            now.timestamp_millis() + Duration::days(2).num_milliseconds(),
            Ordering::SeqCst,
        );
        tx.commit().await.unwrap();
        assert!(pending.await.unwrap().is_err());
        assert_eq!(codec.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn custom_otp_codec_reservation_write_and_commit_expiry_never_call_codec() {
    for after_commit in [false, true] {
        for verify in [false, true] {
            let store = Arc::new(MemoryAuthStore::default());
            let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
            let token = seed(store.as_ref(), now, "reservation-clock-user").await;
            let codec = Arc::new(Codec::default());
            let delivery = Arc::new(otp::Delivery::default());
            let factor = service(store.clone(), codec.clone(), delivery.clone());
            factor.send_otp(&token, "", now).await.unwrap();
            let code = delivery.last();
            codec.calls.store(0, Ordering::SeqCst);
            let clock = Arc::new(std::sync::atomic::AtomicI64::new(now.timestamp_millis()));
            let advance = Some((clock.clone(), now.timestamp_millis() + 30_001));
            let wrapped = Arc::new(super::otp_commit_store::FaultStore {
                store: store.clone(),
                read: None,
                page: None,
                delete: None,
                insert: None,
                begin: false,
                commit: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                advance_on_session: None,
                advance_on_verification: if after_commit { None } else { advance.clone() },
                advance_on_commit: if after_commit { advance } else { None },
            });
            let factor = service(wrapped, codec.clone(), delivery);
            let clock =
                move || DateTime::from_timestamp_millis(clock.load(Ordering::SeqCst)).unwrap();
            let result = if verify {
                factor
                    .verify_otp_with_clock(&token, "", &code, false, &clock)
                    .await
            } else {
                factor.send_otp_with_clock(&token, "", &clock).await
            };
            assert_eq!(result.err().unwrap().code, "OTP_HAS_EXPIRED");
            assert_eq!(codec.calls.load(Ordering::SeqCst), 0);
        }
    }
}
