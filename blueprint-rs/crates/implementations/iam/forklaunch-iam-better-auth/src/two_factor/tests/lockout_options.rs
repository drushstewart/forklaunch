use super::*;
use std::time::Duration as StdDuration;

fn policy(enabled: bool, threshold: u8, millis: u64) -> AccountLockoutOptions {
    AccountLockoutOptions::new(enabled, threshold, StdDuration::from_millis(millis)).unwrap()
}
#[test]
fn lockout_options_bounds_corrupt_counters_and_disabled_preservation() {
    assert_eq!(
        AccountLockoutOptions::default().duration(),
        StdDuration::from_secs(900)
    );
    assert_eq!(AccountLockoutOptions::default().max_failed_attempts(), 10);
    for (n, d) in [
        (0, StdDuration::ZERO),
        (101, StdDuration::ZERO),
        (1, StdDuration::from_nanos(1)),
        (1, StdDuration::from_secs(31_536_001)),
        (1, StdDuration::MAX),
    ] {
        assert!(AccountLockoutOptions::new(true, n, d).is_err());
    }
    let factor = TwoFactorService::new(
        Arc::new(MemoryAuthStore::default()),
        vec![42; 32],
        "Lockout".into(),
    )
    .unwrap();
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for count in [json!(-1), json!(1.5), json!("2"), json!(u64::MAX)] {
        let mut row = json!({"failedVerificationCount":count});
        let before = row.clone();
        assert!(factor.record_account_failure(&mut row, now).is_err());
        assert_eq!(row, before);
    }
    let fractional = TwoFactorService::new(
        Arc::new(MemoryAuthStore::default()),
        vec![42; 32],
        "Clock".into(),
    )
    .unwrap()
    .with_account_lockout_options(policy(true, 1, 1001));
    for (clock, expected) in [
        (0, 1000),
        (-1, 999),
        (-1001, 0),
        (1_800_000_000_000, 1_800_000_001_001),
    ] {
        let mut row = json!({});
        fractional
            .record_account_failure(
                &mut row,
                DateTime::<Utc>::from_timestamp_millis(clock).unwrap(),
            )
            .unwrap();
        assert_eq!(
            time(&row, "lockedUntil").unwrap().timestamp_millis(),
            expected
        );
    }
    let mut row = json!({});
    let before = row.clone();
    assert!(
        fractional
            .record_account_failure(&mut row, DateTime::<Utc>::MAX_UTC)
            .is_err()
    );
    assert_eq!(row, before);
    let disabled = factor.with_account_lockout_options(policy(false, 1, 0));
    let mut row = json!({"failedVerificationCount":"malformed","lockedUntil":"malformed"});
    let before = row.clone();
    assert!(!disabled.check_account_lock(&mut row, now).unwrap());
    assert!(!disabled.record_account_failure(&mut row, now).unwrap());
    disabled.reset_account_failures(&mut row);
    assert_eq!(row, before);
}
async fn challenge_for<S: AuthStore>(
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
async fn lockout_mixed_factors_policy_changes_and_concurrent_failure_budget() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
#[allow(clippy::too_many_lines)]
pub(super) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let id = "lockout-options-user";
    let mut user = json!({"id":id,"email":"lockout@example.test","twoFactorEnabled":false});
    let session = new_session(id, "", "", now).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User, user.clone()).await.unwrap();
    tx.insert(Model::Session, session.clone()).await.unwrap();
    tx.commit().await.unwrap();
    let delivery = Arc::new(otp::Delivery::default());
    let build = |options| {
        TwoFactorService::new(store.clone(), vec![42; 32], "Lockout".into())
            .unwrap()
            .with_otp_delivery(delivery.clone())
            .with_account_lockout_options(options)
            .with_lifecycle_options(
                TwoFactorLifecycleOptions::default()
                    .with_passwordless(true)
                    .with_skip_verification_on_enable(true),
            )
    };
    let factor = build(policy(true, 2, 1500));
    let enrollment = factor
        .enable(text(&session, "token").unwrap(), String::new(), None, now)
        .await
        .unwrap();
    let codes = enrollment.body()["backupCodes"].as_array().unwrap().clone();
    let token = enrollment.replacement_session_token().unwrap();
    user["twoFactorEnabled"] = json!(true);
    let challenge = challenge_for(&factor, &user, now).await;
    assert_eq!(
        factor
            .verify("", &challenge, "invalid", false, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_CODE"
    );
    assert_eq!(
        factor
            .verify("", &challenge, "invalid", true, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_BACKUP_CODE"
    );
    let row = saved(store.as_ref(), id).await;
    assert_eq!(row["failedVerificationCount"], 2);
    assert_eq!(
        time(&row, "lockedUntil").unwrap(),
        now + Duration::milliseconds(1500)
    );
    factor.send_otp("", &challenge, now).await.unwrap();
    let code = delivery.last();
    assert_eq!(
        factor
            .verify_otp("", &challenge, &code, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "ACCOUNT_TEMPORARILY_LOCKED"
    );
    factor
        .verify(token, "", codes[0].as_str().unwrap(), true, false, now)
        .await
        .unwrap();
    assert_eq!(
        saved(store.as_ref(), id).await["failedVerificationCount"],
        2
    );
    let disabled = build(policy(false, 1, 10_000));
    assert_eq!(
        disabled
            .verify("", &challenge, "invalid", true, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_BACKUP_CODE"
    );
    disabled
        .verify_otp("", &challenge, &code, false, now)
        .await
        .unwrap();
    assert_eq!(
        saved(store.as_ref(), id).await["failedVerificationCount"],
        2
    );
    let changed = build(policy(true, 5, 20_000));
    let challenge = challenge_for(&changed, &user, now).await;
    assert_eq!(
        changed
            .verify(
                "",
                &challenge,
                "invalid",
                true,
                false,
                now + Duration::milliseconds(1499)
            )
            .await
            .err()
            .unwrap()
            .code,
        "ACCOUNT_TEMPORARILY_LOCKED"
    );
    let at = now + Duration::milliseconds(1500);
    assert_eq!(
        changed
            .verify("", &challenge, "invalid", true, false, at)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_BACKUP_CODE"
    );
    let row = saved(store.as_ref(), id).await;
    assert_eq!(row["failedVerificationCount"], 1);
    assert!(row["lockedUntil"].is_null());
    let zero = build(policy(true, 1, 0));
    zero.verify("", &challenge, "invalid", true, false, at)
        .await
        .err()
        .unwrap();
    assert_eq!(
        time(&saved(store.as_ref(), id).await, "lockedUntil").unwrap(),
        at
    );
    zero.verify("", &challenge, codes[1].as_str().unwrap(), true, false, at)
        .await
        .unwrap();
    assert_eq!(
        saved(store.as_ref(), id).await["failedVerificationCount"],
        0
    );
    // Concurrent independent challenges serialize their shared account increment.
    let concurrent = Arc::new(build(policy(true, 2, 1500)));
    let a = challenge_for(&concurrent, &user, at).await;
    let b = challenge_for(&concurrent, &user, at).await;
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let mut handles = Vec::new();
    for challenge in [a, b] {
        let factor = concurrent.clone();
        let barrier = barrier.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            factor
                .verify("", &challenge, "invalid", false, false, at)
                .await
                .err()
                .unwrap()
                .code
        }));
    }
    barrier.wait().await;
    for task in handles {
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(10), task)
                .await
                .unwrap()
                .unwrap(),
            "INVALID_CODE"
        );
    }
    assert_eq!(
        saved(store.as_ref(), id).await["failedVerificationCount"],
        2
    );
    // Expired-lock clearing must survive the deliberate exhausted-challenge error.
    let challenge = challenge_for(&disabled, &user, at).await;
    for _ in 0..5 {
        disabled
            .verify("", &challenge, "invalid", true, false, at)
            .await
            .err()
            .unwrap();
    }
    let after = at + Duration::milliseconds(1500);
    assert_eq!(
        factor
            .verify("", &challenge, "invalid", true, false, after)
            .await
            .err()
            .unwrap()
            .code,
        "TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE"
    );
    let row = saved(store.as_ref(), id).await;
    assert_eq!(row["failedVerificationCount"], 0);
    assert!(row["lockedUntil"].is_null());
    // Commit failure must not publish either the account increment or proof attempt.
    let challenge = challenge_for(&factor, &user, after).await;
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
        "Lockout".into(),
    )
    .unwrap()
    .with_account_lockout_options(policy(true, 1, 1500));
    assert!(
        failed
            .verify("", &challenge, "invalid", true, false, after)
            .await
            .is_err()
    );
    assert_eq!(
        saved(store.as_ref(), id).await["failedVerificationCount"],
        0
    );
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        verification_row(&mut tx, &challenge)
            .await
            .unwrap()
            .unwrap()["attempts"],
        0
    );
    tx.commit().await.unwrap();
    // OTP-only identities have no factor row/account counter, even at threshold1.
    let otp_user =
        json!({"id":"lockout-otp-only-user","email":"only@example.test","twoFactorEnabled":true});
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User, otp_user.clone()).await.unwrap();
    tx.commit().await.unwrap();
    let otp_only = build(policy(true, 1, 1500));
    let challenge = challenge_for(&otp_only, &otp_user, after).await;
    otp_only.send_otp("", &challenge, after).await.unwrap();
    for _ in 0..2 {
        assert_eq!(
            otp_only
                .verify_otp("", &challenge, "invalid", false, after)
                .await
                .err()
                .unwrap()
                .code,
            "INVALID_CODE"
        );
    }
    otp_only
        .verify_otp("", &challenge, &delivery.last(), false, after)
        .await
        .unwrap();
    // Disabled successful TOTP/OTP must work with factor UPDATE denied. Recovery
    // consumption still requires UPDATE and must roll back its challenge/session.
    let read_only = TwoFactorService::new(
        Arc::new(lockout_store::FactorReadOnly {
            store: store.clone(),
        }),
        vec![42; 32],
        "Read only".into(),
    )
    .unwrap()
    .with_otp_delivery(delivery.clone())
    .with_account_lockout_options(policy(false, 1, 1500));
    let before = saved(store.as_ref(), id).await;
    let challenge = challenge_for(&disabled, &user, after).await;
    let code = crypto::code(
        &secret(&before).unwrap(),
        u64::try_from(after.timestamp() / 30).unwrap(),
        6,
    );
    read_only
        .verify("", &challenge, &code, false, false, after)
        .await
        .unwrap();
    let challenge = challenge_for(&disabled, &user, after).await;
    read_only.send_otp("", &challenge, after).await.unwrap();
    read_only
        .verify_otp("", &challenge, &delivery.last(), false, after)
        .await
        .unwrap();
    assert_eq!(saved(store.as_ref(), id).await, before);
    let challenge = challenge_for(&disabled, &user, after).await;
    let sessions = rows(store.as_ref(), Model::Session).await.len();
    assert_eq!(
        read_only
            .verify(
                "",
                &challenge,
                codes[2].as_str().unwrap(),
                true,
                false,
                after
            )
            .await
            .err()
            .unwrap()
            .code,
        "better_auth_store_failure"
    );
    assert_eq!(rows(store.as_ref(), Model::Session).await.len(), sessions);
    assert_eq!(saved(store.as_ref(), id).await, before);
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        verification_row(&mut tx, &challenge)
            .await
            .unwrap()
            .is_some()
    );
    tx.commit().await.unwrap();
}
