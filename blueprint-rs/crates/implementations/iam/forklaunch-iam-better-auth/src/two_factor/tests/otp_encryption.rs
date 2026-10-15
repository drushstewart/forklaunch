use super::*;

#[tokio::test]
async fn encrypted_otp_policy_keys_consumption_race_and_rollback() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
async fn otp_row<S: AuthStore>(store: &S, baseline: &[Value]) -> Value {
    rows(store, Model::Verification)
        .await
        .into_iter()
        .find(|row| !baseline.contains(&row["id"]))
        .unwrap()
}
#[allow(clippy::too_many_lines)]
pub(super) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let user =
        json!({"id":"encrypted-otp-user","email":"encrypted@example.test","twoFactorEnabled":true});
    let session = new_session("encrypted-otp-user", "", "", now).unwrap();
    let token = text(&session, "token").unwrap().to_owned();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User, user).await.unwrap();
    tx.insert(Model::Session, session).await.unwrap();
    tx.commit().await.unwrap();
    let baseline: Vec<Value> = rows(store.as_ref(), Model::Verification)
        .await
        .into_iter()
        .map(|r| r["id"].clone())
        .collect();
    let delivery = Arc::new(otp::Delivery::default());
    let keys = Arc::new(OtpEncryptionKeys::versioned(7, vec![(7, "x".repeat(32))], None).unwrap());
    let encrypted = Arc::new(
        TwoFactorService::new(store.clone(), vec![42; 32], "OTP".into())
            .unwrap()
            .with_otp_delivery(delivery.clone())
            .with_otp_encryption_keys(keys.clone()),
    );
    for storage in [OtpStorage::Plain, OtpStorage::Hashed] {
        encrypted.send_otp(&token, "", now).await.unwrap();
        let code = delivery.last();
        let saved = otp_row(store.as_ref(), &baseline).await;
        assert!(text(&saved, "value").unwrap().starts_with("$ba$7$"));
        let changed = TwoFactorService::new(store.clone(), vec![42; 32], "OTP".into())
            .unwrap()
            .with_otp_storage(storage)
            .with_otp_delivery(delivery.clone());
        assert_eq!(
            changed
                .verify_otp(&token, "", &code, false, now)
                .await
                .err()
                .unwrap()
                .code,
            "INVALID_CODE"
        );
        let retained = otp_row(store.as_ref(), &baseline).await;
        assert_eq!(retained["value"], saved["value"]);
        assert_eq!(retained["attempts"], 1);
        encrypted
            .verify_otp(&token, "", &code, false, now)
            .await
            .unwrap();
        changed.send_otp(&token, "", now).await.unwrap();
        let code = delivery.last();
        assert_eq!(
            encrypted
                .verify_otp(&token, "", &code, false, now)
                .await
                .err()
                .unwrap()
                .code,
            "TWO_FACTOR_STORAGE_DECODE_FAILED"
        );
        assert_eq!(
            encrypted
                .verify_otp(&token, "", &code, false, now)
                .await
                .err()
                .unwrap()
                .code,
            "OTP_HAS_EXPIRED"
        );
    }
    let rotated = TwoFactorService::new(store.clone(), vec![42; 32], "OTP".into())
        .unwrap()
        .with_otp_encryption_keys(Arc::new(
            OtpEncryptionKeys::versioned(8, vec![(8, "y".repeat(32)), (7, "x".repeat(32))], None)
                .unwrap(),
        ));
    encrypted.send_otp(&token, "", now).await.unwrap();
    rotated
        .verify_otp(&token, "", &delivery.last(), false, now)
        .await
        .unwrap();
    let retired = TwoFactorService::new(store.clone(), vec![42; 32], "OTP".into())
        .unwrap()
        .with_otp_encryption_keys(Arc::new(
            OtpEncryptionKeys::versioned(8, vec![(8, "y".repeat(32))], None).unwrap(),
        ));
    encrypted.send_otp(&token, "", now).await.unwrap();
    assert_eq!(
        retired
            .verify_otp(&token, "", &delivery.last(), false, now)
            .await
            .err()
            .unwrap()
            .code,
        "TWO_FACTOR_STORAGE_DECODE_FAILED"
    );
    assert_eq!(
        encrypted
            .verify_otp(&token, "", &delivery.last(), false, now)
            .await
            .err()
            .unwrap()
            .code,
        "OTP_HAS_EXPIRED"
    );
    // Failed commit restores a malformed proof instead of falsely reporting consumption.
    encrypted.send_otp(&token, "", now).await.unwrap();
    let mut saved = otp_row(store.as_ref(), &baseline).await;
    saved["value"] = json!("$ba$7$not-hex");
    let mut tx = store.begin(true).await.unwrap();
    tx.replace(Model::Verification, saved.clone())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let failing = TwoFactorService::new(
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
        "OTP".into(),
    )
    .unwrap()
    .with_otp_encryption_keys(keys);
    let failed = failing
        .verify_otp(&token, "", &delivery.last(), false, now)
        .await
        .err()
        .unwrap();
    assert_ne!(failed.code, "TWO_FACTOR_STORAGE_DECODE_FAILED");
    assert_eq!(otp_row(store.as_ref(), &baseline).await, saved);
    assert_eq!(
        encrypted
            .verify_otp(&token, "", &delivery.last(), false, now)
            .await
            .err()
            .unwrap()
            .code,
        "TWO_FACTOR_STORAGE_DECODE_FAILED"
    );
    encrypted.send_otp(&token, "", now).await.unwrap();
    let code = delivery.last();
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let (factor, barrier, token, code) = (
            encrypted.clone(),
            barrier.clone(),
            token.clone(),
            code.clone(),
        );
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            factor.verify_otp(&token, "", &code, false, now).await
        }));
    }
    barrier.wait().await;
    let mut successes = 0;
    for h in handles {
        match tokio::time::timeout(std::time::Duration::from_secs(10), h)
            .await
            .unwrap()
            .unwrap()
        {
            Ok(_) => successes += 1,
            Err(e) => assert_eq!(e.code, "OTP_HAS_EXPIRED"),
        }
    }
    assert_eq!(successes, 1);
    let wide = TwoFactorService::new(store.clone(), vec![42; 32], "OTP".into())
        .unwrap()
        .with_otp_storage(OtpStorage::Encrypted)
        .with_otp_delivery(delivery.clone())
        .with_otp_options(OtpOptions::new(32, std::time::Duration::from_secs(180), 5).unwrap());
    wide.send_otp(&token, "", now).await.unwrap();
    let saved = otp_row(store.as_ref(), &baseline).await;
    let ciphertext = text(&saved, "value").unwrap();
    assert_eq!(ciphertext.len(), 144);
    let plain = TwoFactorService::new(store.clone(), vec![42; 32], "OTP".into()).unwrap();
    assert_eq!(
        plain
            .verify_otp(&token, "", ciphertext, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_CODE"
    );
    assert_eq!(otp_row(store.as_ref(), &baseline).await, saved);
    wide.verify_otp(&token, "", &delivery.last(), false, now)
        .await
        .unwrap();
    // Expired lock clearing precedes decode and commits with destructive failure.
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::TwoFactor,json!({"id":"encrypted-otp-factor","userId":"encrypted-otp-user","secret":STANDARD.encode([7u8;32]),"backupCodes":[],"verified":true,"failedVerificationCount":5,"lockedUntil":super::timestamp(now-Duration::seconds(1))})).await.unwrap();
    let user = tx
        .get(Model::User, "encrypted-otp-user")
        .await
        .unwrap()
        .unwrap();
    let signin::SigninGate::Challenge(challenge) = encrypted
        .gate(&mut tx, &user, None, true, "", "", now)
        .await
        .unwrap()
    else {
        panic!("challenge")
    };
    tx.commit().await.unwrap();
    encrypted
        .send_otp("", challenge.identifier(), now)
        .await
        .unwrap();
    assert_eq!(
        retired
            .verify_otp("", challenge.identifier(), &delivery.last(), true, now)
            .await
            .err()
            .unwrap()
            .code,
        "TWO_FACTOR_STORAGE_DECODE_FAILED"
    );
    let mut tx = store.begin(false).await.unwrap();
    let factor = tx
        .get(Model::TwoFactor, "encrypted-otp-factor")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(factor["failedVerificationCount"], 0);
    assert!(factor["lockedUntil"].is_null());
    assert!(
        verification_row(&mut tx, challenge.identifier())
            .await
            .unwrap()
            .is_some()
    );
    tx.commit().await.unwrap();
}
