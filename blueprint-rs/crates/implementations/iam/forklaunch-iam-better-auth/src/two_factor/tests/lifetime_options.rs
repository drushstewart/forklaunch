use super::*;
use std::time::Duration as StdDuration;

#[test]
fn lifetime_options_bounds_and_defaults() {
    let defaults = TwoFactorLifetimeOptions::default();
    assert_eq!(defaults.challenge().as_secs(), 600);
    assert_eq!(defaults.trusted_device().as_secs(), 2_592_000);
    for invalid in [
        StdDuration::from_millis(1),
        StdDuration::from_secs(31_536_001),
        StdDuration::MAX,
    ] {
        assert!(TwoFactorLifetimeOptions::new(invalid, StdDuration::ZERO).is_err());
        assert!(TwoFactorLifetimeOptions::new(StdDuration::ZERO, invalid).is_err());
    }
    assert!(
        TwoFactorLifetimeOptions::new(StdDuration::ZERO, StdDuration::from_secs(31_536_000))
            .is_ok()
    );
}

#[tokio::test]
async fn lifetime_challenge_and_trust_boundaries_policy_change_and_one_use() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}

#[allow(clippy::too_many_lines)]
pub(super) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let id = "lifetime-options-user";
    let user = json!({"id":id,"email":"lifetime@example.test","twoFactorEnabled":false});
    let session = new_session(id, "", "", now).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User, user.clone()).await.unwrap();
    tx.insert(Model::Session, session.clone()).await.unwrap();
    tx.commit().await.unwrap();
    let base = TwoFactorService::new(store.clone(), vec![42; 32], "Lifetime".into())
        .unwrap()
        .with_lifecycle_options(
            TwoFactorLifecycleOptions::default()
                .with_passwordless(true)
                .with_skip_verification_on_enable(true),
        );
    let enrolled = base
        .enable(text(&session, "token").unwrap(), String::new(), None, now)
        .await
        .unwrap();
    let backups = enrolled.body()["backupCodes"].as_array().unwrap().clone();
    let mut user = user;
    user["twoFactorEnabled"] = json!(true);
    let mut index = 0;
    for seconds in [0_u64, 2, 600] {
        let factor = base.copy_for_test(
            TwoFactorLifetimeOptions::new(
                StdDuration::from_secs(seconds),
                StdDuration::from_secs(seconds),
            )
            .unwrap(),
        );
        for offset in [-1_i64, 0, 1] {
            let mut tx = store.begin(true).await.unwrap();
            let signin::SigninGate::Challenge(challenge) = factor
                .gate(&mut tx, &user, None, true, "", "", now)
                .await
                .unwrap()
            else {
                panic!("challenge expected")
            };
            tx.commit().await.unwrap();
            let expiry = now + Duration::seconds(i64::try_from(seconds).unwrap());
            let at = expiry + Duration::milliseconds(offset);
            let result = factor
                .verify(
                    "",
                    challenge.identifier(),
                    backups[index].as_str().unwrap(),
                    true,
                    true,
                    at,
                )
                .await;
            if offset > 0 {
                assert_eq!(result.err().unwrap().code, "INVALID_TWO_FACTOR_COOKIE");
                continue;
            }
            index += 1;
            let proof = result.unwrap();
            assert!(proof.replacement_session_token().is_some());
            assert!(
                factor
                    .verify(
                        "",
                        challenge.identifier(),
                        backups[index].as_str().unwrap(),
                        true,
                        false,
                        at
                    )
                    .await
                    .is_err()
            );
            let trust = proof.trust.as_deref().unwrap();
            let trust_expiry = at + Duration::seconds(i64::try_from(seconds).unwrap());
            let changed = base.copy_for_test(
                TwoFactorLifetimeOptions::new(
                    StdDuration::from_secs(11),
                    StdDuration::from_secs(7),
                )
                .unwrap(),
            );
            // Existing trust expiry, not changed policy, determines admission.
            let mut tx = store.begin(true).await.unwrap();
            assert!(matches!(
                changed
                    .gate(&mut tx, &user, Some(trust), true, "", "", trust_expiry)
                    .await
                    .unwrap(),
                signin::SigninGate::Challenge(_)
            ));
            tx.commit().await.unwrap();
            // Renewal deletes the old proof before inserting its replacement. A
            // failed replacement must roll back that deletion, preserving retry.
            let failing = TwoFactorService::new(
                Arc::new(fault_store::FaultStore {
                    store: store.clone(),
                    read: None,
                    page: None,
                    delete: None,
                    insert: Some(Model::Verification),
                    begin: false,
                    commit: false,
                }),
                vec![42; 32],
                "Lifetime".into(),
            )
            .unwrap()
            .with_lifetime_options(changed.lifetime_options());
            {
                let mut tx = failing.store.begin(true).await.unwrap();
                assert!(
                    failing
                        .gate(
                            &mut tx,
                            &user,
                            Some(trust),
                            true,
                            "",
                            "",
                            trust_expiry - Duration::milliseconds(1)
                        )
                        .await
                        .is_err()
                );
            }
            let mut tx = store.begin(true).await.unwrap();
            let signin::SigninGate::Trusted(renewed) = changed
                .gate(
                    &mut tx,
                    &user,
                    Some(trust),
                    true,
                    "",
                    "",
                    trust_expiry - Duration::milliseconds(1),
                )
                .await
                .unwrap()
            else {
                panic!("unexpired trust")
            };
            let (_, identifier) = renewed.split_once('!').unwrap();
            let row = verification_row(&mut tx, identifier)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                time(&row, "expiresAt").unwrap(),
                trust_expiry - Duration::milliseconds(1) + Duration::seconds(7)
            );
            tx.commit().await.unwrap();
        }
    }
    // TOTP and delivered OTP use the same inclusive challenge boundary, separately
    // from their own code windows/expiry. Exercise both entrypoints, not only backup.
    let at = now + Duration::hours(1);
    let delivery = Arc::new(otp::Delivery::default());
    let factor = base
        .copy_for_test(
            TwoFactorLifetimeOptions::new(StdDuration::from_secs(2), StdDuration::ZERO).unwrap(),
        )
        .with_otp_delivery(delivery.clone());
    for otp in [false, true] {
        for offset in [0_i64, 1] {
            let mut tx = store.begin(true).await.unwrap();
            let signin::SigninGate::Challenge(challenge) = factor
                .gate(&mut tx, &user, None, true, "", "", at)
                .await
                .unwrap()
            else {
                panic!("challenge")
            };
            tx.commit().await.unwrap();
            let expiry = at + Duration::seconds(2) + Duration::milliseconds(offset);
            let result = if otp {
                factor
                    .send_otp("", challenge.identifier(), at)
                    .await
                    .unwrap();
                factor
                    .verify_otp("", challenge.identifier(), &delivery.last(), false, expiry)
                    .await
            } else {
                let mut tx = store.begin(false).await.unwrap();
                let row = tx
                    .find_unique(Model::TwoFactor, "userId", id)
                    .await
                    .unwrap()
                    .unwrap();
                tx.commit().await.unwrap();
                let code = crypto::code(
                    &secret(&row).unwrap(),
                    u64::try_from(expiry.timestamp() / 30).unwrap(),
                    6,
                );
                factor
                    .verify("", challenge.identifier(), &code, false, false, expiry)
                    .await
            };
            if offset == 0 {
                assert!(result.is_ok());
            } else {
                assert_eq!(result.err().unwrap().code, "INVALID_TWO_FACTOR_COOKIE");
            }
        }
    }
}
impl<S> TwoFactorService<S> {
    fn copy_for_test(&self, options: TwoFactorLifetimeOptions) -> Self {
        Self {
            credential_policy: self.credential_policy,
            time_source: self.time_source.clone(),
            session_policy: self.session_policy,
            store: self.store.clone(),
            secret: self.secret.clone(),
            issuer: self.issuer.clone(),
            otp_delivery: self.otp_delivery.clone(),
            otp_background_dispatcher: self.otp_background_dispatcher.clone(),
            otp_options: self.otp_options,
            otp_storage: self.otp_storage,
            otp_encryption_keys: self.otp_encryption_keys.clone(),
            otp_codec: self.otp_codec.clone(),
            totp_options: self.totp_options,
            lifecycle_options: self.lifecycle_options.clone(),
            lifetime_options: options,
            account_lockout_options: self.account_lockout_options,
            backup_code_options: self.backup_code_options,
            backup_code_generator: self.backup_code_generator.clone(),
            backup_code_storage: self.backup_code_storage,
            backup_code_encryption_keys: self.backup_code_encryption_keys.clone(),
            backup_codec: self.backup_codec.clone(),
        }
    }
}
