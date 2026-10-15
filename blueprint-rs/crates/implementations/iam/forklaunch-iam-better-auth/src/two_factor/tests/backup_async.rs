use super::*;
use std::sync::atomic::{AtomicI64, AtomicU8, AtomicUsize, Ordering};
use tokio::sync::Notify;

#[derive(Default)]
struct Control {
    encrypt: AtomicU8,
    decrypt: AtomicU8,
    calls: AtomicUsize,
    entered: Notify,
    release: Notify,
}
struct Codec(Arc<Control>);
impl BackupCodeEncryptionCodec for Codec {
    fn encrypt(&self, input: BackupCodeCodecText) -> BackupCodeCodecFuture {
        work(self.0.clone(), input, true)
    }
    fn decrypt(&self, input: BackupCodeCodecText) -> BackupCodeCodecFuture {
        work(self.0.clone(), input, false)
    }
}
fn work(control: Arc<Control>, input: BackupCodeCodecText, encrypt: bool) -> BackupCodeCodecFuture {
    Box::pin(async move {
        control.calls.fetch_add(1, Ordering::SeqCst);
        let mode = if encrypt {
            control.encrypt.load(Ordering::SeqCst)
        } else {
            control.decrypt.load(Ordering::SeqCst)
        };
        if mode == 1 {
            return Err(error("private-backup-key", "secret callback diagnostics"));
        }
        if mode == 2 {
            control.entered.notify_one();
            control.release.notified().await;
        }
        if mode == 3 {
            return BackupCodeCodecText::new("not json".into());
        }
        if mode == 4 {
            return BackupCodeCodecText::new("{}".into());
        }
        BackupCodeCodecText::new(if encrypt {
            format!("X{}", input.as_str())
        } else {
            input
                .as_str()
                .strip_prefix('X')
                .ok_or_else(|| error("private-decode", "secret"))?
                .into()
        })
    })
}
fn service<S: AuthStore>(store: Arc<S>, control: Arc<Control>) -> TwoFactorService<S> {
    TwoFactorService::new(store, vec![42; 32], "Async backup".into())
        .unwrap()
        .with_lifecycle_options(
            TwoFactorLifecycleOptions::default()
                .with_passwordless(true)
                .with_skip_verification_on_enable(true),
        )
        .with_backup_code_generator(Arc::new(|| {
            Ok(vec![
                "one".into(),
                "one".into(),
                "two".into(),
                String::new(),
                "é🔐".into(),
            ])
        }))
        .with_backup_code_encryption_codec(
            Arc::new(Codec(control)),
            BackupCodeCodecOptions::default(),
        )
}
async fn setup<S: AuthStore + 'static>(
    store: Arc<S>,
    id: &str,
) -> (
    Arc<TwoFactorService<S>>,
    Arc<Control>,
    String,
    DateTime<Utc>,
) {
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let session = new_session(id, "", "", now).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":id,"email":format!("{id}@example.test"),"twoFactorEnabled":false}),
    )
    .await
    .unwrap();
    tx.insert(Model::Session, session.clone()).await.unwrap();
    tx.commit().await.unwrap();
    let control = Arc::new(Control::default());
    let factor = Arc::new(service(store, control.clone()));
    let enabled = factor
        .enable_with_backup_clock(
            text(&session, "token").unwrap(),
            String::new(),
            None,
            &move || now,
        )
        .await
        .unwrap();
    (
        factor,
        control,
        enabled.replacement_session_token().unwrap().into(),
        now,
    )
}
async fn by_user<S: AuthStore>(store: &S, id: &str) -> Value {
    let mut tx = store.begin(false).await.unwrap();
    let row = tx
        .find_unique(Model::TwoFactor, "userId", id)
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    row
}
async fn pending<S: AuthStore>(
    factor: &TwoFactorService<S>,
    id: &str,
    now: DateTime<Utc>,
) -> String {
    let mut tx = factor.store.begin(true).await.unwrap();
    let user = tx.get(Model::User, id).await.unwrap().unwrap();
    let signin::SigninGate::Challenge(p) = factor
        .gate(&mut tx, &user, None, true, "", "", now)
        .await
        .unwrap()
    else {
        panic!("challenge")
    };
    tx.commit().await.unwrap();
    p.identifier().into()
}
async fn wait(control: &Control) {
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        control.entered.notified(),
    )
    .await
    .unwrap();
}
fn options() -> BackupVerificationOptions {
    BackupVerificationOptions {
        disable_session: false,
        trust_device: true,
    }
}
#[test]
fn async_backup_codec_bounds_and_opaque_format() {
    assert!(BackupCodeCodecText::new("x".repeat(154_330)).is_ok());
    assert!(BackupCodeCodecText::new("x".repeat(154_331)).is_err());
    for value in ["", "é:🔐"] {
        assert!(BackupCodeCodecText::new(value.into()).is_ok());
    }
    for duration in [
        std::time::Duration::ZERO,
        std::time::Duration::from_nanos(1),
        std::time::Duration::from_secs(31),
    ] {
        assert!(BackupCodeCodecOptions::new(duration).is_err());
    }
}
#[tokio::test]
async fn async_backup_lifecycle_error_attempts_and_policy_migration() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
#[allow(clippy::too_many_lines)]
pub(super) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let id = "async-backup-lifecycle";
    let (factor, control, token, now) = setup(store.clone(), id).await;
    let original = by_user(store.as_ref(), id).await;
    assert_eq!(original["backupCodesFormat"], 2);
    assert_eq!(
        original["backupCodes"],
        "X[\"one\",\"one\",\"two\",\"\",\"é🔐\"]"
    );
    let builtin = TwoFactorService::new(store.clone(), vec![42; 32], "Builtin".into()).unwrap();
    assert_eq!(
        builtin
            .verify(&token, "", "one", true, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "TWO_FACTOR_BACKUP_STORAGE_FAILED"
    );
    let challenge = pending(&factor, id, now).await;
    for mode in [1, 4] {
        control.decrypt.store(mode, Ordering::SeqCst);
        let before = rows(store.as_ref(), Model::Verification).await;
        assert_eq!(
            factor
                .verify_backup_with_clock("", &challenge, "one", options(), &move || now)
                .await
                .err()
                .unwrap()
                .code,
            "TWO_FACTOR_BACKUP_CODEC_FAILED"
        );
        assert_eq!(rows(store.as_ref(), Model::Verification).await, before);
        assert_eq!(by_user(store.as_ref(), id).await, original);
    }
    control.decrypt.store(3, Ordering::SeqCst);
    assert_eq!(
        factor
            .verify_backup_with_clock("", &challenge, "one", options(), &move || now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_BACKUP_CODE"
    );
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        verification_row(&mut tx, &challenge)
            .await
            .unwrap()
            .unwrap()["attempts"],
        1
    );
    tx.commit().await.unwrap();
    control.decrypt.store(0, Ordering::SeqCst);
    control.encrypt.store(1, Ordering::SeqCst);
    let before = by_user(store.as_ref(), id).await;
    assert_eq!(
        factor
            .verify_backup_with_clock("", &challenge, "one", options(), &move || now)
            .await
            .err()
            .unwrap()
            .code,
        "TWO_FACTOR_BACKUP_CODEC_FAILED"
    );
    assert_eq!(by_user(store.as_ref(), id).await, before);
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        verification_row(&mut tx, &challenge)
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
    control.encrypt.store(0, Ordering::SeqCst);
    assert!(
        factor
            .verify_backup_with_clock("", &challenge, "one", options(), &move || now)
            .await
            .is_err()
    );
    factor
        .verify_backup_with_clock(&token, "", "one", options(), &move || now)
        .await
        .unwrap();
    assert_eq!(
        by_user(store.as_ref(), id).await["backupCodes"],
        "X[\"two\",\"\",\"é🔐\"]"
    );
    factor
        .verify_backup_with_clock(&token, "", "", options(), &move || now)
        .await
        .unwrap();
    factor
        .verify_backup_with_clock(&token, "", "é🔐", options(), &move || now)
        .await
        .unwrap();
    // A default writer migrates to legacy; custom verification can consume legacy
    // without invoking decrypt and publishes its remaining proofs in format2.
    builtin
        .with_lifecycle_options(TwoFactorLifecycleOptions::default().with_passwordless(true))
        .with_backup_code_generator(Arc::new(|| Ok(vec!["legacy".into(), "keep".into()])))
        .regenerate_backup_codes(&token, String::new(), now)
        .await
        .unwrap();
    control.decrypt.store(1, Ordering::SeqCst);
    factor
        .verify_backup_with_clock(&token, "", "legacy", options(), &move || now)
        .await
        .unwrap();
    assert_eq!(
        by_user(store.as_ref(), id).await["backupCodes"],
        "X[\"keep\"]"
    );
    control.decrypt.store(0, Ordering::SeqCst);
    // Per-user reservation rejects a second overlapping callback before decrypt.
    control.decrypt.store(2, Ordering::SeqCst);
    let cloned = factor.clone();
    let token2 = token.clone();
    let task = tokio::spawn(async move {
        cloned
            .verify_backup_with_clock(&token2, "", "keep", options(), &move || now)
            .await
    });
    wait(&control).await;
    let calls = control.calls.load(Ordering::SeqCst);
    assert_eq!(
        factor
            .verify_backup_with_clock(&token, "", "keep", options(), &move || now)
            .await
            .err()
            .unwrap()
            .code,
        "TWO_FACTOR_BACKUP_CODEC_CONFLICT"
    );
    assert_eq!(control.calls.load(Ordering::SeqCst), calls);
    control.release.notify_one();
    task.await.unwrap().unwrap();
    assert_eq!(by_user(store.as_ref(), id).await["backupCodes"], "X[]");
}
#[tokio::test]
async fn async_backup_stale_authority_aba_lockout_and_missing_owner() {
    for action in [
        "signout",
        "expiry",
        "disable",
        "aba",
        "lock",
        "delete-lease",
        "owner",
    ] {
        let store = Arc::new(MemoryAuthStore::default());
        let (factor, control, token, now) = setup(store.clone(), "race-user").await;
        let challenge = if action == "lock" {
            pending(&factor, "race-user", now).await
        } else {
            String::new()
        };
        control.decrypt.store(2, Ordering::SeqCst);
        let f = factor.clone();
        let t = if action == "lock" {
            String::new()
        } else {
            token.clone()
        };
        let task = tokio::spawn(async move {
            f.verify_backup_with_clock(&t, &challenge, "one", options(), &move || now)
                .await
        });
        wait(&control).await;
        let before = by_user(store.as_ref(), "race-user").await;
        let mut tx = store.begin(true).await.unwrap();
        match action {
            "signout" | "expiry" => {
                let mut session = tx
                    .find_unique(Model::Session, "token", &token)
                    .await
                    .unwrap()
                    .unwrap();
                if action == "signout" {
                    tx.delete(Model::Session, text(&session, "id").unwrap())
                        .await
                        .unwrap();
                } else {
                    session["expiresAt"] = json!(timestamp(now));
                    tx.replace(Model::Session, session).await.unwrap();
                }
            }
            "disable" => {
                let mut user = tx.get(Model::User, "race-user").await.unwrap().unwrap();
                user["twoFactorEnabled"] = json!(false);
                tx.replace(Model::User, user).await.unwrap();
            }
            "aba" | "lock" => {
                let mut row = before.clone();
                if action == "aba" {
                    row["backupCodesRevision"] = json!(uuid::Uuid::new_v4().to_string());
                } else {
                    row["lockedUntil"] = json!(timestamp(now + Duration::seconds(60)));
                }
                tx.replace(Model::TwoFactor, row).await.unwrap();
            }
            _ => {
                let mut row = verification_row(
                    &mut tx,
                    &super::super::backup_codec_state::identifier("race-user"),
                )
                .await
                .unwrap()
                .unwrap();
                if action == "delete-lease" {
                    tx.delete(Model::Verification, text(&row, "id").unwrap())
                        .await
                        .unwrap();
                } else {
                    row["backupCodecLease"]["owner"] = json!(uuid::Uuid::new_v4().to_string());
                    tx.replace(Model::Verification, row).await.unwrap();
                }
            }
        }
        tx.commit().await.unwrap();
        control.release.notify_one();
        assert!(task.await.unwrap().is_err());
        assert_eq!(
            by_user(store.as_ref(), "race-user").await["backupCodes"],
            before["backupCodes"]
        );
    }
}
#[tokio::test]
async fn async_backup_cancel_restart_lazy_recovery_and_generic_cleanup() {
    for phase in ["decode", "reencode"] {
        for generic in [false, true] {
            let store = Arc::new(MemoryAuthStore::default());
            let (factor, control, _, now) = setup(store.clone(), "cancel-user").await;
            let challenge = pending(&factor, "cancel-user", now).await;
            if phase == "decode" {
                control.decrypt.store(2, Ordering::SeqCst);
            } else {
                control.encrypt.store(2, Ordering::SeqCst);
            }
            let f = factor.clone();
            let c = challenge.clone();
            let task = tokio::spawn(async move {
                f.verify_backup_with_clock("", &c, "one", options(), &move || now)
                    .await
            });
            wait(&control).await;
            task.abort();
            assert!(task.await.err().unwrap().is_cancelled());
            let before = by_user(store.as_ref(), "cancel-user").await;
            let later = now + Duration::seconds(31);
            control.encrypt.store(0, Ordering::SeqCst);
            control.decrypt.store(0, Ordering::SeqCst);
            if generic {
                let oauth =
                    crate::oauth::state::StateService::new(store.clone(), vec![42; 32], false)
                        .unwrap();
                assert!(
                    oauth
                        .consume("owned-nonexistent-state", None, later)
                        .await
                        .is_err()
                );
            }
            let restarted = service(store.clone(), control.clone());
            let result = restarted
                .verify_backup_with_clock("", &challenge, "one", options(), &move || later)
                .await;
            if phase == "decode" {
                result.unwrap();
            } else {
                assert!(result.is_err());
                assert_eq!(by_user(store.as_ref(), "cancel-user").await, before);
                let mut tx = store.begin(false).await.unwrap();
                assert!(
                    verification_row(&mut tx, &challenge)
                        .await
                        .unwrap()
                        .is_none()
                );
                assert!(
                    verification_row(
                        &mut tx,
                        &super::super::backup_codec_state::identifier("cancel-user")
                    )
                    .await
                    .unwrap()
                    .is_none()
                );
                tx.commit().await.unwrap();
            }
        }
    }
}
#[tokio::test]
async fn async_backup_enroll_regenerate_cancellation_and_password_change() {
    for enroll in [false, true] {
        for action in ["password", "cancel", "signout"] {
            let store = Arc::new(MemoryAuthStore::default());
            let control = Arc::new(Control::default());
            let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
            let session = new_session("mutation-user", "", "", now).unwrap();
            let mut tx = store.begin(true).await.unwrap();
            tx.insert(Model::User,json!({"id":"mutation-user","email":"mutation@example.test","twoFactorEnabled":false})).await.unwrap();
            tx.insert(Model::Session, session.clone()).await.unwrap();
            tx.commit().await.unwrap();
            let factor = Arc::new(service(store.clone(), control.clone()));
            let mut token = text(&session, "token").unwrap().to_owned();
            if !enroll {
                token = factor
                    .enable_with_backup_clock(&token, String::new(), None, &move || now)
                    .await
                    .unwrap()
                    .replacement_session_token()
                    .unwrap()
                    .into();
            }
            let factors = rows(store.as_ref(), Model::TwoFactor).await;
            control.encrypt.store(2, Ordering::SeqCst);
            let f = factor.clone();
            let t = token.clone();
            let task = tokio::spawn(async move {
                if enroll {
                    f.enable_with_backup_clock(&t, String::new(), None, &move || now)
                        .await
                } else {
                    f.regenerate_backup_codes_with_clock(&t, String::new(), &move || now)
                        .await
                }
            });
            wait(&control).await;
            if action == "cancel" {
                task.abort();
                assert!(task.await.err().unwrap().is_cancelled());
            } else {
                let mut tx = store.begin(true).await.unwrap();
                if action == "password" {
                    tx.insert(Model::Account,json!({"id":"added-credential","userId":"mutation-user","providerId":"credential","accountId":"mutation-user","password":"new-password-hash"})).await.unwrap();
                } else {
                    let row = tx
                        .find_unique(Model::Session, "token", &token)
                        .await
                        .unwrap()
                        .unwrap();
                    tx.delete(Model::Session, text(&row, "id").unwrap())
                        .await
                        .unwrap();
                }
                tx.commit().await.unwrap();
                control.release.notify_one();
                assert!(task.await.unwrap().is_err());
            }
            assert_eq!(rows(store.as_ref(), Model::TwoFactor).await, factors);
            let default =
                TwoFactorService::new(store.clone(), vec![42; 32], "Maintenance".into()).unwrap();
            assert_eq!(
                default
                    .cleanup_backup_codec_states(None, now + Duration::seconds(31))
                    .await
                    .unwrap()
                    .removed,
                1
            );
        }
    }
}
#[tokio::test]
async fn async_backup_total_lease_and_reentrant_callbacks() {
    struct Slow;
    impl BackupCodeEncryptionCodec for Slow {
        fn encrypt(&self, input: BackupCodeCodecText) -> BackupCodeCodecFuture {
            Box::pin(async move {
                tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                BackupCodeCodecText::new(format!("X{}", input.as_str()))
            })
        }
        fn decrypt(&self, input: BackupCodeCodecText) -> BackupCodeCodecFuture {
            Box::pin(async move {
                tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                BackupCodeCodecText::new(input.as_str()[1..].into())
            })
        }
    }
    struct Reentrant<S>(Arc<S>);
    impl<S: AuthStore + 'static> BackupCodeEncryptionCodec for Reentrant<S> {
        fn encrypt(&self, input: BackupCodeCodecText) -> BackupCodeCodecFuture {
            let store = self.0.clone();
            Box::pin(async move {
                let tx = store.begin(true).await?;
                tx.commit().await?;
                BackupCodeCodecText::new(format!("X{}", input.as_str()))
            })
        }
        fn decrypt(&self, input: BackupCodeCodecText) -> BackupCodeCodecFuture {
            let store = self.0.clone();
            Box::pin(async move {
                let tx = store.begin(true).await?;
                tx.commit().await?;
                BackupCodeCodecText::new(input.as_str()[1..].into())
            })
        }
    }
    let store = Arc::new(MemoryAuthStore::default());
    let (_, _, token, now) = setup(store.clone(), "reentry-user").await;
    let factor = service(store.clone(), Arc::new(Control::default()))
        .with_backup_code_encryption_codec(
            Arc::new(Reentrant(store.clone())),
            BackupCodeCodecOptions::default(),
        );
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        factor.verify_backup_with_clock(&token, "", "one", options(), &move || now),
    )
    .await
    .unwrap()
    .unwrap();
    let timed = factor.with_backup_code_encryption_codec(
        Arc::new(Slow),
        BackupCodeCodecOptions::new(std::time::Duration::from_millis(60)).unwrap(),
    );
    let before = by_user(store.as_ref(), "reentry-user").await;
    let challenge = pending(&timed, "reentry-user", now).await;
    assert_eq!(
        timed
            .verify_backup_with_clock("", &challenge, "two", options(), &move || now)
            .await
            .err()
            .unwrap()
            .code,
        "TWO_FACTOR_BACKUP_CODEC_FAILED"
    );
    assert_eq!(by_user(store.as_ref(), "reentry-user").await, before);
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        verification_row(&mut tx, &challenge)
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn async_backup_phase_commit_expiry_final_write_clock_and_rollback() {
    rollback(Arc::new(MemoryAuthStore::default())).await;
}
pub(super) async fn rollback<S: AuthStore + 'static>(store: Arc<S>) {
    for mode in 0..3 {
        let id = format!("backup-commit-{mode}");
        let (original, control, _, now) = setup(store.clone(), &id).await;
        let challenge = pending(&original, &id, now).await;
        let before = by_user(store.as_ref(), &id).await;
        let sessions = rows(store.as_ref(), Model::Session).await;
        let clock = Arc::new(AtomicI64::new(now.timestamp_millis()));
        let tick = clock.clone();
        let wrapped = Arc::new(backup_commit_store::FaultStore {
            store: store.clone(),
            commits: Arc::new(AtomicUsize::new(0)),
            fail_commit: if mode == 0 { 3 } else { 0 },
            advance_commit: if mode == 1 { 2 } else { 0 },
            clock: clock.clone(),
            late: (now + Duration::seconds(31)).timestamp_millis(),
            advance_session: mode == 2,
        });
        let factor = service(wrapped, control.clone());
        let calls = control.calls.load(Ordering::SeqCst);
        assert!(
            factor
                .verify_backup_with_clock("", &challenge, "one", options(), &move || {
                    DateTime::from_timestamp_millis(tick.load(Ordering::SeqCst)).unwrap()
                })
                .await
                .is_err()
        );
        assert_eq!(
            control.calls.load(Ordering::SeqCst) - calls,
            if mode == 1 { 1 } else { 2 }
        );
        assert_eq!(by_user(store.as_ref(), &id).await, before);
        assert_eq!(rows(store.as_ref(), Model::Session).await, sessions);
        let mut tx = store.begin(false).await.unwrap();
        assert!(
            verification_row(&mut tx, &challenge)
                .await
                .unwrap()
                .is_some()
        );
        tx.commit().await.unwrap();
        let cleanup = TwoFactorService::new(store.clone(), vec![42; 32], "Cleanup".into()).unwrap();
        cleanup
            .cleanup_backup_codec_states(None, now + Duration::seconds(31))
            .await
            .unwrap();
        let mut tx = store.begin(false).await.unwrap();
        assert!(
            verification_row(&mut tx, &challenge)
                .await
                .unwrap()
                .is_none()
        );
        tx.commit().await.unwrap();
    }
}
#[tokio::test]
async fn async_backup_initial_contention_expiry_prevents_callback() {
    let store = Arc::new(MemoryAuthStore::default());
    let (factor, control, token, now) = setup(store.clone(), "contended-user").await;
    let mut tx = store.begin(true).await.unwrap();
    let mut row = tx
        .find_unique(Model::Session, "token", &token)
        .await
        .unwrap()
        .unwrap();
    row["expiresAt"] = json!(timestamp(now + Duration::seconds(1)));
    tx.replace(Model::Session, row).await.unwrap();
    tx.commit().await.unwrap();
    let hold = store.begin(true).await.unwrap();
    let clock = Arc::new(AtomicI64::new(now.timestamp_millis()));
    let tick = clock.clone();
    let calls = control.calls.load(Ordering::SeqCst);
    let f = factor.clone();
    let task = tokio::spawn(async move {
        f.verify_backup_with_clock(&token, "", "one", options(), &move || {
            DateTime::from_timestamp_millis(tick.load(Ordering::SeqCst)).unwrap()
        })
        .await
    });
    tokio::task::yield_now().await;
    clock.store(
        (now + Duration::seconds(2)).timestamp_millis(),
        Ordering::SeqCst,
    );
    hold.commit().await.unwrap();
    assert!(task.await.unwrap().is_err());
    assert_eq!(control.calls.load(Ordering::SeqCst), calls);
}

#[tokio::test]
async fn async_backup_reencoding_cleanup_preserves_changed_challenge() {
    let store = Arc::new(MemoryAuthStore::default());
    let (factor, control, _, now) = setup(store.clone(), "changed-challenge").await;
    let challenge = pending(&factor, "changed-challenge", now).await;
    control.encrypt.store(2, Ordering::SeqCst);
    let f = factor.clone();
    let c = challenge.clone();
    let task = tokio::spawn(async move {
        f.verify_backup_with_clock("", &c, "one", options(), &move || now)
            .await
    });
    wait(&control).await;
    let mut tx = store.begin(true).await.unwrap();
    let mut changed = verification_row(&mut tx, &challenge)
        .await
        .unwrap()
        .unwrap();
    changed["attempts"] = json!(1);
    tx.replace(Model::Verification, changed.clone())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    task.abort();
    assert!(task.await.err().unwrap().is_cancelled());
    factor
        .cleanup_backup_codec_states(None, now + Duration::seconds(31))
        .await
        .unwrap();
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        verification_row(&mut tx, &challenge).await.unwrap(),
        Some(changed)
    );
    tx.commit().await.unwrap();
    assert_eq!(
        by_user(store.as_ref(), "changed-challenge").await["backupCodes"],
        "X[\"one\",\"one\",\"two\",\"\",\"é🔐\"]"
    );
}
#[tokio::test]
async fn async_backup_regeneration_cannot_overwrite_competing_consumption() {
    let store = Arc::new(MemoryAuthStore::default());
    let (factor, control, token, now) = setup(store.clone(), "regen-race").await;
    // Start with a legacy array, admissible to both the custom callback and builtin reader.
    let builtin = TwoFactorService::new(store.clone(), vec![42; 32], "Builtin".into())
        .unwrap()
        .with_lifecycle_options(TwoFactorLifecycleOptions::default().with_passwordless(true))
        .with_backup_code_generator(Arc::new(|| Ok(vec!["one".into(), "two".into()])));
    builtin
        .regenerate_backup_codes(&token, String::new(), now)
        .await
        .unwrap();
    control.encrypt.store(2, Ordering::SeqCst);
    let f = factor.clone();
    let t = token.clone();
    let task = tokio::spawn(async move {
        f.regenerate_backup_codes_with_clock(&t, String::new(), &move || now)
            .await
    });
    wait(&control).await;
    builtin
        .verify(&token, "", "one", true, false, now)
        .await
        .unwrap();
    let winner = by_user(store.as_ref(), "regen-race").await;
    control.release.notify_one();
    assert_eq!(
        task.await.unwrap().err().unwrap().code,
        "TWO_FACTOR_BACKUP_CODEC_CONFLICT"
    );
    assert_eq!(by_user(store.as_ref(), "regen-race").await, winner);
    assert_eq!(winner["backupCodes"], json!(["two"]));
}
