use super::*;
use std::time::Duration as StdDuration;

#[test]
fn totp_options_bounds_and_actual_sha1_counter_vectors() {
    assert_eq!(TotpOptions::default().digits(), TotpDigits::Six);
    assert_eq!(TotpOptions::default().period(), StdDuration::from_secs(30));
    for duration in [
        StdDuration::ZERO,
        StdDuration::from_nanos(1_000_001),
        StdDuration::from_millis(86_400_001),
        StdDuration::MAX,
    ] {
        assert_eq!(
            TotpOptions::new(TotpDigits::Eight, duration)
                .unwrap_err()
                .code,
            "better_auth_invalid_config"
        );
    }
    let vectors: Vec<Value> = serde_json::from_str(include_str!("totp_vectors.json")).unwrap();
    assert_eq!(vectors.len(), 96);
    for row in vectors {
        let digits = if row["digits"] == 6 {
            TotpDigits::Six
        } else {
            TotpDigits::Eight
        };
        let options = TotpOptions::new(
            digits,
            StdDuration::from_millis(row["periodMs"].as_u64().unwrap()),
        )
        .unwrap();
        let counter = options.counter(row["millis"].as_i64().unwrap()).unwrap();
        assert_eq!(counter, row["counter"].as_u64().unwrap(), "{row}");
        assert_eq!(
            crypto::code(b"12345678901234567890", counter, digits.count()),
            row["code"],
            "{row}"
        );
        assert!(crypto::matches_with_options(
            b"12345678901234567890",
            row["code"].as_str().unwrap(),
            row["millis"].as_i64().unwrap(),
            options
        ));
    }
    let options = TotpOptions::default();
    assert_eq!(options.counter(-1), None);
    assert_eq!(options.counter(i64::MAX), None);
    assert_eq!(options.counter(8_640_000_000_000_001), None);
    let secret = b"12345678901234567890";
    assert!(crypto::matches_with_options(
        secret,
        &crypto::code(secret, 0, 6),
        0,
        options
    ));
    assert!(crypto::matches_with_options(
        secret,
        &crypto::code(secret, 1, 6),
        0,
        options
    ));
    assert!(!crypto::matches_with_options(
        secret,
        &crypto::code(secret, u64::MAX, 6),
        0,
        options
    ));
    assert!(!crypto::matches_with_options(
        secret,
        &crypto::code(secret, 0, 6),
        -1,
        options
    ));
    let fractional = TotpOptions::new(TotpDigits::Eight, StdDuration::from_millis(2007)).unwrap();
    assert_eq!(fractional.counter(200_700), Some(99)); // JS binary64 boundary, not 100.
}

#[tokio::test]
async fn totp_options_apply_to_existing_secret_without_rewriting_it() {
    let now = DateTime::<Utc>::from_timestamp_millis(1_800_000_000_123).unwrap();
    let store = Arc::new(MemoryAuthStore::default());
    let session = new_session("totp-options-user", "", "", now).unwrap();
    let token = text(&session, "token").unwrap().to_owned();
    let secret_bytes = b"12345678901234567890123456789012";
    let row = json!({"id":"totp-options-factor","userId":"totp-options-user","secret":STANDARD.encode(secret_bytes),"backupCodes":[],"verified":true,"failedVerificationCount":0,"lockedUntil":null});
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User,json!({"id":"totp-options-user","email":"totp-options@example.test","twoFactorEnabled":true})).await.unwrap();
    tx.insert(Model::Session, session).await.unwrap();
    tx.insert(Model::TwoFactor, row.clone()).await.unwrap();
    tx.commit().await.unwrap();
    let options = TotpOptions::new(TotpDigits::Eight, StdDuration::from_millis(1500)).unwrap();
    let service = TwoFactorService::new(store.clone(), vec![42; 32], "OTP".into())
        .unwrap()
        .with_totp_options(options);
    let counter = options.counter(now.timestamp_millis()).unwrap();
    let code = crypto::code(secret_bytes, counter, 8);
    service
        .verify(&token, "", &code, false, false, now)
        .await
        .unwrap();
    let service = service.with_totp_options(TotpOptions::default());
    assert_eq!(
        service
            .verify(&token, "", &code, false, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_CODE"
    );
    let code = crypto::code(
        secret_bytes,
        TotpOptions::default()
            .counter(now.timestamp_millis())
            .unwrap(),
        6,
    );
    service
        .verify(&token, "", &code, false, false, now)
        .await
        .unwrap();
    assert_eq!(rows(store.as_ref(), Model::TwoFactor).await, vec![row]);
}
