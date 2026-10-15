use super::*;

#[test]
fn otp_hash_vectors_and_persisted_representation_bounds() {
    assert_eq!(OtpStorage::default(), OtpStorage::Plain);
    for (input, expected) in [
        ("", "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU"),
        ("123456", "jZae727K08KaOmKSgOaGzww_XVqGr_PKEgIMkjrcbJI"),
    ] {
        assert_eq!(OtpStorage::Hashed.encode(input).as_str(), expected);
        assert!(super::super::otp_storage::valid_stored(expected));
    }
    for value in [
        "",
        "abc",
        "12345678901234567890123456789012345",
        "///////////////////////////////////////////",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAB",
    ] {
        assert!(!super::super::otp_storage::valid_stored(value));
    }
}

#[tokio::test]
async fn otp_storage_modes_policy_changes_race_and_rollback() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}

#[allow(clippy::too_many_lines)]
pub(super) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let user =
        json!({"id":"otp-storage-user","email":"storage@example.test","twoFactorEnabled":true});
    let session = new_session("otp-storage-user", "", "", now).unwrap();
    let token = text(&session, "token").unwrap().to_owned();
    let existing_ids: Vec<Value> = rows(store.as_ref(), Model::Verification)
        .await
        .into_iter()
        .map(|row| row["id"].clone())
        .collect();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User, user.clone()).await.unwrap();
    tx.insert(Model::Session, session).await.unwrap();
    tx.commit().await.unwrap();
    let delivery = Arc::new(otp::Delivery::default());
    for storage in [OtpStorage::Plain, OtpStorage::Hashed] {
        for digits in [1, 32] {
            let factor = Arc::new(
                TwoFactorService::new(store.clone(), vec![42; 32], "OTP".into())
                    .unwrap()
                    .with_otp_delivery(delivery.clone())
                    .with_otp_storage(storage)
                    .with_otp_options(
                        OtpOptions::new(digits, std::time::Duration::from_secs(180), 5).unwrap(),
                    ),
            );
            factor.send_otp(&token, "", now).await.unwrap();
            let code = delivery.last();
            assert_eq!(code.len(), usize::from(digits));
            let saved = rows(store.as_ref(), Model::Verification)
                .await
                .into_iter()
                .find(|row| !existing_ids.contains(&row["id"]))
                .unwrap();
            assert_eq!(saved["value"], json!(storage.encode(&code).as_str()));
            let other = if storage == OtpStorage::Plain {
                OtpStorage::Hashed
            } else {
                OtpStorage::Plain
            };
            let changed = TwoFactorService::new(store.clone(), vec![42; 32], "OTP".into())
                .unwrap()
                .with_otp_storage(other);
            assert_eq!(
                changed
                    .verify_otp(&token, "", &code, false, now)
                    .await
                    .err()
                    .unwrap()
                    .code,
                "INVALID_CODE"
            );
            let mut tx = store.begin(false).await.unwrap();
            let current = tx
                .get(Model::Verification, text(&saved, "id").unwrap())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(current["value"], saved["value"]);
            assert_eq!(current["attempts"], 1);
            tx.commit().await.unwrap();
            // Restoration with different configured digits must not infer width from hash length.
            let restored = TwoFactorService::new(store.clone(), vec![42; 32], "OTP".into())
                .unwrap()
                .with_otp_storage(storage);
            for invalid in ["", "é🔑"] {
                assert_eq!(
                    restored
                        .verify_otp(&token, "", invalid, false, now)
                        .await
                        .err()
                        .unwrap()
                        .code,
                    "INVALID_CODE"
                );
            }
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
                "OTP".into(),
            )
            .unwrap()
            .with_otp_storage(storage);
            assert!(
                failed
                    .verify_otp(&token, "", &code, false, now)
                    .await
                    .is_err()
            );
            restored
                .verify_otp(&token, "", &code, false, now)
                .await
                .unwrap();
            assert_eq!(
                restored
                    .verify_otp(&token, "", &code, false, now)
                    .await
                    .err()
                    .unwrap()
                    .code,
                "OTP_HAS_EXPIRED"
            );
            factor.send_otp(&token, "", now).await.unwrap();
            let code = delivery.last();
            let barrier = Arc::new(tokio::sync::Barrier::new(3));
            let mut handles = Vec::new();
            for _ in 0..2 {
                let (factor, barrier, code, token) =
                    (factor.clone(), barrier.clone(), code.clone(), token.clone());
                handles.push(tokio::spawn(async move {
                    barrier.wait().await;
                    factor.verify_otp(&token, "", &code, false, now).await
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
                    Err(e) => assert_eq!(e.code, "OTP_HAS_EXPIRED"),
                }
            }
            assert_eq!(successes, 1);
            factor.send_otp(&token, "", now).await.unwrap();
            factor
                .verify_otp(
                    &token,
                    "",
                    &delivery.last(),
                    false,
                    now + Duration::seconds(180),
                )
                .await
                .unwrap();
            factor.send_otp(&token, "", now).await.unwrap();
            assert_eq!(
                factor
                    .verify_otp(
                        &token,
                        "",
                        &delivery.last(),
                        false,
                        now + Duration::seconds(180) + Duration::milliseconds(1)
                    )
                    .await
                    .err()
                    .unwrap()
                    .code,
                "OTP_HAS_EXPIRED"
            );
        }
    }
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::TwoFactor,json!({"id":"otp-storage-factor","userId":"otp-storage-user","secret":STANDARD.encode([7u8;32]),"backupCodes":[],"verified":true,"failedVerificationCount":0,"lockedUntil":null})).await.unwrap();
    tx.commit().await.unwrap();
    let factor = TwoFactorService::new(store.clone(), vec![42; 32], "OTP".into())
        .unwrap()
        .with_otp_storage(OtpStorage::Hashed)
        .with_otp_delivery(delivery.clone())
        .with_account_lockout_options(
            AccountLockoutOptions::new(true, 1, std::time::Duration::from_secs(1)).unwrap(),
        );
    let mut tx = store.begin(true).await.unwrap();
    let signin::SigninGate::Challenge(challenge) = factor
        .gate(&mut tx, &user, None, true, "", "", now)
        .await
        .unwrap()
    else {
        panic!("challenge")
    };
    tx.commit().await.unwrap();
    factor
        .send_otp("", challenge.identifier(), now)
        .await
        .unwrap();
    assert_eq!(
        factor
            .verify_otp("", challenge.identifier(), "bad", false, now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_CODE"
    );
    assert_eq!(
        factor
            .verify_otp("", challenge.identifier(), &delivery.last(), false, now)
            .await
            .err()
            .unwrap()
            .code,
        "ACCOUNT_TEMPORARILY_LOCKED"
    );
    factor
        .verify_otp(
            "",
            challenge.identifier(),
            &delivery.last(),
            false,
            now + Duration::seconds(1),
        )
        .await
        .unwrap();
}
