use super::*;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

#[test]
fn backup_generation_bounds_format_and_custom_output() {
    for (amount, length) in [(101, 10), (1, 0), (1, 128)] {
        assert!(BackupCodeOptions::new(amount, length).is_err());
    }
    for (amount, length) in [(0, 1), (1, 1), (3, 5), (2, 6), (100, 127), (10, 10)] {
        let factor = TwoFactorService::new(
            Arc::new(MemoryAuthStore::default()),
            vec![42; 32],
            "Backups".into(),
        )
        .unwrap()
        .with_backup_code_options(BackupCodeOptions::new(amount, length).unwrap());
        let codes = factor.generate_backup_codes().unwrap();
        assert_eq!(codes.len(), usize::from(amount));
        for code in codes {
            assert_eq!(code.len(), usize::from(length) + 1);
            assert_eq!(code.as_bytes()[5.min(usize::from(length))], b'-');
            assert!(code.bytes().all(|b| b == b'-' || b.is_ascii_alphanumeric()));
        }
    }
    for codes in [Vec::new(), vec![String::new(), "é".repeat(64)]] {
        let expected = codes.clone();
        let factor = TwoFactorService::new(
            Arc::new(MemoryAuthStore::default()),
            vec![42; 32],
            "Backups".into(),
        )
        .unwrap()
        .with_backup_code_generator(Arc::new(move || Ok(codes.clone())));
        assert_eq!(factor.generate_backup_codes().unwrap(), expected);
    }
    for codes in [vec!["x".to_owned(); 101], vec!["é".repeat(65)]] {
        let factor = TwoFactorService::new(
            Arc::new(MemoryAuthStore::default()),
            vec![42; 32],
            "Backups".into(),
        )
        .unwrap()
        .with_backup_code_generator(Arc::new(move || Ok(codes.clone())));
        assert_eq!(
            factor.generate_backup_codes().unwrap_err().code,
            "TWO_FACTOR_GENERATION_FAILED"
        );
    }
}
async fn pending<S: AuthStore>(
    factor: &TwoFactorService<S>,
    user: &Value,
    now: DateTime<Utc>,
) -> String {
    let mut tx = factor.store.begin(true).await.unwrap();
    let signin::SigninGate::Challenge(c) = factor
        .gate(&mut tx, user, None, true, "", "", now)
        .await
        .unwrap()
    else {
        panic!("challenge")
    };
    tx.commit().await.unwrap();
    c.identifier().to_owned()
}
async fn saved<S: AuthStore>(store: &S, id: &str) -> Value {
    let mut tx = store.begin(false).await.unwrap();
    let row = tx
        .find_unique(Model::TwoFactor, "userId", id)
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    row
}
#[tokio::test]
async fn backup_generator_ordering_old_codes_duplicates_race_and_rollback() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
#[allow(clippy::too_many_lines)]
pub(super) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let id = "backup-options-user";
    let mut user = json!({"id":id,"email":"backup@example.test","twoFactorEnabled":false});
    let session = new_session(id, "", "", now).unwrap();
    let token = text(&session, "token").unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User, user.clone()).await.unwrap();
    tx.insert(Model::Session, session.clone()).await.unwrap();
    tx.commit().await.unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let mode = Arc::new(AtomicU8::new(0));
    let count = calls.clone();
    let state = mode.clone();
    let generator: Arc<dyn BackupCodeGenerator> = Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
        match state.load(Ordering::SeqCst) {
            1 => Err(error(
                "SECRET_CALLBACK_ERROR",
                "private generator credential",
            )),
            2 => Ok(vec!["x".into(); 101]),
            _ => Ok(vec![
                "duplicate".into(),
                "duplicate".into(),
                String::new(),
                "é🔑".into(),
                "race".into(),
                "keep".into(),
            ]),
        }
    });
    let factor = TwoFactorService::new(store.clone(), vec![42; 32], "Backups".into())
        .unwrap()
        .with_backup_code_options(BackupCodeOptions::new(1, 1).unwrap())
        .with_backup_code_generator(generator.clone())
        .with_lifecycle_options(
            TwoFactorLifecycleOptions::default()
                .with_passwordless(true)
                .with_skip_verification_on_enable(true),
        );
    assert!(factor.enable("", String::new(), None, now).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    mode.store(1, Ordering::SeqCst);
    let failure = factor
        .enable(token, String::new(), None, now)
        .await
        .err()
        .unwrap();
    assert_eq!(failure.code, "TWO_FACTOR_GENERATION_FAILED");
    assert!(!failure.message.contains("private"));
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.find_unique(Model::TwoFactor, "userId", id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(tx.get(Model::User, id).await.unwrap().unwrap(), user);
    assert!(
        tx.get(Model::Session, text(&session, "id").unwrap())
            .await
            .unwrap()
            .is_some()
    );
    tx.commit().await.unwrap();
    mode.store(0, Ordering::SeqCst);
    let enrolled = factor
        .enable(token, String::new(), None, now)
        .await
        .unwrap();
    let active = enrolled.replacement_session_token().unwrap();
    user["twoFactorEnabled"] = json!(true);
    assert_eq!(
        enrolled.body()["backupCodes"],
        json!(["duplicate", "duplicate", "", "é🔑", "race", "keep"])
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        factor
            .enable(active, String::new(), None, now)
            .await
            .err()
            .unwrap()
            .code,
        "TOTP_ALREADY_ENABLED"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let changed = Arc::new(
        TwoFactorService::new(store.clone(), vec![42; 32], "Backups".into())
            .unwrap()
            .with_backup_code_options(BackupCodeOptions::new(0, 1).unwrap())
            .with_lifecycle_options(TwoFactorLifecycleOptions::default().with_passwordless(true)),
    );
    changed
        .verify(active, "", "duplicate", true, false, now)
        .await
        .unwrap();
    assert_eq!(
        changed
            .verify(active, "", "duplicate", true, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_BACKUP_CODE"
    );
    changed
        .verify(active, "", "", true, false, now)
        .await
        .unwrap();
    changed
        .verify(active, "", "é🔑", true, false, now)
        .await
        .unwrap();
    assert_eq!(
        saved(store.as_ref(), id).await["backupCodes"],
        json!(["race", "keep"])
    );
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let challenge = pending(&changed, &user, now).await;
        let factor = changed.clone();
        let barrier = barrier.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            factor
                .verify("", &challenge, "race", true, false, now)
                .await
        }));
    }
    barrier.wait().await;
    let mut successes = 0;
    for handle in handles {
        match tokio::time::timeout(std::time::Duration::from_secs(10), handle)
            .await
            .unwrap()
            .unwrap()
        {
            Ok(_) => successes += 1,
            Err(e) => assert_eq!(e.code, "INVALID_BACKUP_CODE"),
        }
    }
    assert_eq!(successes, 1);
    assert_eq!(
        saved(store.as_ref(), id).await["backupCodes"],
        json!(["keep"])
    );
    let before = saved(store.as_ref(), id).await;
    let sessions = rows(store.as_ref(), Model::Session).await;
    for bad in [1, 2] {
        mode.store(bad, Ordering::SeqCst);
        assert_eq!(
            factor
                .regenerate_backup_codes(active, String::new(), now)
                .await
                .err()
                .unwrap()
                .code,
            "TWO_FACTOR_GENERATION_FAILED"
        );
        assert_eq!(saved(store.as_ref(), id).await, before);
        assert_eq!(rows(store.as_ref(), Model::Session).await, sessions);
    }
    mode.store(0, Ordering::SeqCst);
    let rollback_id = "backup-rollback-user";
    let rollback_user =
        json!({"id":rollback_id,"email":"rollback@example.test","twoFactorEnabled":false});
    let rollback_session = new_session(rollback_id, "", "", now).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User, rollback_user.clone()).await.unwrap();
    tx.insert(Model::Session, rollback_session.clone())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let sessions_before = rows(store.as_ref(), Model::Session).await;
    let calls_before = calls.load(Ordering::SeqCst);
    let failure = TwoFactorService::new(
        Arc::new(fault_store::FaultStore {
            store: store.clone(),
            read: None,
            page: None,
            delete: None,
            insert: Some(Model::Session),
            begin: false,
            commit: false,
        }),
        vec![42; 32],
        "Backups".into(),
    )
    .unwrap()
    .with_backup_code_generator(generator.clone())
    .with_lifecycle_options(
        TwoFactorLifecycleOptions::default()
            .with_passwordless(true)
            .with_skip_verification_on_enable(true),
    );
    // Rotation fails after factor/user mutation: the entire enrollment rolls back.
    assert!(
        failure
            .enable(
                text(&rollback_session, "token").unwrap(),
                String::new(),
                None,
                now
            )
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), calls_before + 1);
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::User, rollback_id).await.unwrap(),
        Some(rollback_user)
    );
    assert!(
        tx.find_unique(Model::TwoFactor, "userId", rollback_id)
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
    assert_eq!(rows(store.as_ref(), Model::Session).await, sessions_before);
    let denied = TwoFactorService::new(
        Arc::new(lockout_store::FactorReadOnly {
            store: store.clone(),
        }),
        vec![42; 32],
        "Backups".into(),
    )
    .unwrap()
    .with_lifecycle_options(TwoFactorLifecycleOptions::default().with_passwordless(true));
    assert!(
        denied
            .regenerate_backup_codes(active, String::new(), now)
            .await
            .is_err()
    );
    assert_eq!(saved(store.as_ref(), id).await, before);
    let numeric = TwoFactorService::new(store.clone(), vec![42; 32], "Backups".into())
        .unwrap()
        .with_backup_code_options(BackupCodeOptions::new(3, 6).unwrap())
        .with_lifecycle_options(TwoFactorLifecycleOptions::default().with_passwordless(true));
    let replacement = numeric
        .regenerate_backup_codes(active, String::new(), now)
        .await
        .unwrap();
    let codes = replacement.body()["backupCodes"].as_array().unwrap();
    assert_eq!(codes.len(), 3);
    assert!(codes.iter().all(|c| c.as_str().unwrap().len() == 7));
    assert_eq!(
        numeric
            .verify(active, "", "keep", true, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_BACKUP_CODE"
    );
    numeric
        .verify(active, "", codes[0].as_str().unwrap(), true, false, now)
        .await
        .unwrap();
}
