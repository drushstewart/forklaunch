use super::*;
use std::time::Duration as StdDuration;

#[test]
fn bounded_otp_options_validate_without_side_effects() {
    let defaults = OtpOptions::default();
    assert_eq!(defaults.digits(), 6);
    assert_eq!(defaults.validity(), StdDuration::from_secs(180));
    assert_eq!(defaults.allowed_attempts(), 5);
    for (digits, validity, attempts) in [
        (0, StdDuration::from_secs(1), 1),
        (33, StdDuration::from_secs(1), 1),
        (6, StdDuration::ZERO, 1),
        (6, StdDuration::from_nanos(1_000_001), 1),
        (6, StdDuration::MAX, 1),
        (6, StdDuration::from_secs(1), 0),
        (6, StdDuration::from_secs(1), 101),
    ] {
        assert_eq!(
            OtpOptions::new(digits, validity, attempts)
                .unwrap_err()
                .code,
            "better_auth_invalid_config"
        );
    }
    assert!(OtpOptions::new(1, StdDuration::from_millis(1), 1).is_ok());
    assert!(OtpOptions::new(32, StdDuration::from_millis(i64::MAX as u64), 100).is_ok());
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn configured_otp_lengths_expiry_current_attempts_resend_and_rollback() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for (digits, millis, attempts) in [(1, 1, 1), (4, 1500, 1), (8, 3000, 3), (32, 5000, 100)] {
        let store = Arc::new(MemoryAuthStore::default());
        let user = json!({"id":"options-user","email":"otp@example.test","twoFactorEnabled":true});
        let session = new_session("options-user", "", "", now).unwrap();
        let token_owned = text(&session, "token").unwrap().to_owned();
        let token = token_owned.as_str();
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(Model::User, user).await.unwrap();
        tx.insert(Model::Session, session).await.unwrap();
        tx.commit().await.unwrap();
        let delivery = Arc::new(otp::Delivery::default());
        let factor = TwoFactorService::new(store.clone(), vec![42; 32], "OTP".into())
            .unwrap()
            .with_otp_delivery(delivery.clone())
            .with_otp_options(
                OtpOptions::new(digits, StdDuration::from_millis(millis), attempts).unwrap(),
            );
        factor.send_otp(token, "", now).await.unwrap();
        let code = delivery.last();
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
        .with_otp_options(
            OtpOptions::new(digits, StdDuration::from_millis(millis), attempts).unwrap(),
        );
        assert!(
            failed
                .verify_otp(token, "", &code, false, now)
                .await
                .is_err()
        );
        assert_eq!(rows(store.as_ref(), Model::Verification).await.len(), 1);
        assert_eq!(code.len(), usize::from(digits));
        let saved = rows(store.as_ref(), Model::Verification).await.remove(0);
        assert_eq!(
            time(&saved, "expiresAt").unwrap(),
            now + Duration::milliseconds(i64::try_from(millis).unwrap())
        );
        // Replacement is one durable row, with reset attempts and a fresh expiry.
        factor
            .verify_otp(token, "", "invalid", false, now)
            .await
            .err()
            .unwrap();
        factor
            .send_otp(token, "", now + Duration::milliseconds(2))
            .await
            .unwrap();
        let saved = rows(store.as_ref(), Model::Verification).await;
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0]["attempts"], 0);
        assert_eq!(
            time(&saved[0], "expiresAt").unwrap(),
            now + Duration::milliseconds(i64::try_from(millis).unwrap() + 2)
        );
        for _ in 0..attempts {
            assert_eq!(
                factor
                    .verify_otp(token, "", "invalid", false, now)
                    .await
                    .err()
                    .unwrap()
                    .code,
                "INVALID_CODE"
            );
        }
        assert_eq!(
            factor
                .verify_otp(token, "", &delivery.last(), false, now)
                .await
                .err()
                .unwrap()
                .code,
            "TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE"
        );
        factor.send_otp(token, "", now).await.unwrap();
        let code = delivery.last();
        // Changed validity/digits do not reinterpret an issued code or its expiry.
        let factor =
            factor.with_otp_options(OtpOptions::new(6, StdDuration::from_secs(1), 1).unwrap());
        factor
            .verify_otp(
                token,
                "",
                &code,
                false,
                now + Duration::milliseconds(i64::try_from(millis).unwrap()),
            )
            .await
            .unwrap();
        factor.send_otp(token, "", now).await.unwrap();
        assert_eq!(
            factor
                .verify_otp(
                    token,
                    "",
                    &delivery.last(),
                    false,
                    now + Duration::milliseconds(1001)
                )
                .await
                .err()
                .unwrap()
                .code,
            "OTP_HAS_EXPIRED"
        );
        factor.send_otp(token, "", now).await.unwrap();
        factor
            .verify_otp(token, "", "invalid", false, now)
            .await
            .err()
            .unwrap();
        let factor =
            factor.with_otp_options(OtpOptions::new(8, StdDuration::from_secs(2), 2).unwrap());
        factor
            .verify_otp(token, "", &delivery.last(), false, now)
            .await
            .unwrap();
        let factor = factor.with_otp_options(
            OtpOptions::new(6, StdDuration::from_millis(i64::MAX as u64), 5).unwrap(),
        );
        assert_eq!(
            factor.send_otp(token, "", now).await.err().unwrap().code,
            "better_auth_invalid_config"
        );
        assert!(rows(store.as_ref(), Model::Verification).await.is_empty());
    }
}
