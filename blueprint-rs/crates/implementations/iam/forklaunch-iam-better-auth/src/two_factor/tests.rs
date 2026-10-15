use super::*;
use crate::{
    accounts::{SigninInput, SignupInput},
    store::{AuthCursor, AuthPage, AuthWindow, memory::MemoryAuthStore},
};
// Reuse the fault adapter while permitting this domain suite without the HTTP feature.
mod backup_options;
#[allow(clippy::duplicate_mod)]
#[path = "../http/observer/fault_store.rs"]
mod fault_store;
#[cfg(feature = "http")]
mod http;
mod lifecycle_options;
mod lifetime_options;
mod lockout_options;
mod lockout_store;
mod otp;
mod otp_encryption;
mod otp_options;
mod otp_storage;
#[cfg(feature = "postgres")]
mod postgres;
mod totp_options;
const PASSWORD: &str = "owned-totp-password";
fn signin() -> SigninInput {
    SigninInput {
        email: "factor@example.test".into(),
        password: PASSWORD.into(),
        callback_url: None,
        remember_me: true,
    }
}
async fn rows<S: AuthStore>(store: &S, model: Model) -> Vec<Value> {
    let mut tx = store.begin(false).await.unwrap();
    let result = tx.list(model).await.unwrap();
    tx.commit().await.unwrap();
    result
}
async fn factor_code<S: AuthStore>(store: &S, now: DateTime<Utc>) -> String {
    let row = rows(store, Model::TwoFactor).await.remove(0);
    crypto::code(
        &secret(&row).unwrap(),
        u64::try_from(now.timestamp() / 30).unwrap(),
        6,
    )
}
async fn challenge<S: AuthStore>(
    service: &CredentialService<S>,
    factor: &TwoFactorService<S>,
    now: DateTime<Utc>,
) -> TwoFactorChallenge {
    match service
        .sign_in_with_two_factor(signin(), factor, None, "", "", now)
        .await
        .unwrap()
    {
        CredentialSigninOutcome::Challenge(value) => value,
        CredentialSigninOutcome::Authenticated { .. } => {
            panic!("factor must prevent session issuance")
        }
    }
}
#[allow(clippy::too_many_lines)]
async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let service = CredentialService::new(store.clone());
    let signup = service
        .sign_up(
            SignupInput {
                name: "Factor".into(),
                email: "factor@example.test".into(),
                password: PASSWORD.into(),
                first_name: "Factor".into(),
                last_name: "User".into(),
                image: None,
                phone_number: None,
                remember_me: true,
            },
            "",
            "",
            now,
        )
        .await
        .unwrap();
    assert!(signup.user.get("twoFactorEnabled").is_none());
    let token = signup.session["token"].as_str().unwrap();
    let factor = TwoFactorService::new(store.clone(), vec![42; 32], "Better Auth".into()).unwrap();
    assert!(
        factor
            .enable(token, "wrong".into(), None, now)
            .await
            .is_err()
    );
    let enrollment = factor
        .enable(token, PASSWORD.into(), None, now)
        .await
        .unwrap();
    assert_eq!(
        enrollment.body()["backupCodes"].as_array().unwrap().len(),
        10
    );
    assert!(
        enrollment.body()["totpURI"]
            .as_str()
            .unwrap()
            .starts_with("otpauth://totp/")
    );
    let code = factor_code(store.as_ref(), now).await;
    let old_other = service.sign_in(signin(), "", "", now).await.unwrap();
    let enabled = factor
        .verify(token, "", &code, false, false, now)
        .await
        .unwrap();
    assert_eq!(enabled.body()["token"], token);
    assert_eq!(enabled.body()["user"]["twoFactorEnabled"], false);
    let active = enabled.replacement_session_token().unwrap().to_owned();
    assert_ne!(active, token);
    assert_eq!(rows(store.as_ref(), Model::Session).await.len(), 2);
    assert!(
        rows(store.as_ref(), Model::Session)
            .await
            .iter()
            .all(|row| row["token"] != token)
    );
    assert!(
        rows(store.as_ref(), Model::Session)
            .await
            .iter()
            .any(|row| row["token"] == old_other.session["token"])
    );
    assert_eq!(
        service
            .sign_in(signin(), "", "", now)
            .await
            .err()
            .unwrap()
            .code,
        "TWO_FACTOR_REQUIRED"
    );
    let pending = challenge(&service, &factor, now).await;
    assert_eq!(rows(store.as_ref(), Model::Session).await.len(), 2);
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
    .unwrap();
    assert!(
        failed
            .verify("", pending.identifier(), &code, false, true, now)
            .await
            .is_err()
    );
    assert_eq!(rows(store.as_ref(), Model::Session).await.len(), 2);
    assert!(
        failed
            .verify("", pending.identifier(), "invalid", false, false, now)
            .await
            .is_err()
    );
    assert_eq!(
        rows(store.as_ref(), Model::TwoFactor).await[0]["failedVerificationCount"],
        0
    );
    for (insert, delete) in [
        (Some(Model::Session), None),
        (None, Some(Model::Verification)),
    ] {
        let failing = TwoFactorService::new(
            Arc::new(fault_store::FaultStore {
                store: store.clone(),
                read: None,
                page: None,
                insert,
                delete,
                begin: false,
                commit: false,
            }),
            vec![42; 32],
            "Better Auth".into(),
        )
        .unwrap();
        assert!(
            failing
                .verify("", pending.identifier(), &code, false, true, now)
                .await
                .is_err()
        );
        assert_eq!(rows(store.as_ref(), Model::Session).await.len(), 2);
        let mut tx = store.begin(false).await.unwrap();
        assert!(
            verification_row(&mut tx, pending.identifier())
                .await
                .unwrap()
                .is_some()
        );
        tx.commit().await.unwrap();
    }
    let (a, b) = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        tokio::join!(
            factor.verify("", pending.identifier(), &code, false, true, now),
            factor.verify("", pending.identifier(), &code, false, true, now)
        )
    })
    .await
    .expect("concurrent verification completes without a lock leak");
    assert_ne!(a.is_ok(), b.is_ok());
    let accepted = a.or(b).unwrap();
    let trust = accepted.trusted_device_value().unwrap();
    let trusted = service
        .sign_in_with_two_factor(signin(), &factor, Some(trust), "", "", now)
        .await
        .unwrap();
    let trusted_token = match trusted {
        CredentialSigninOutcome::Authenticated {
            grant,
            trust: replacement,
        } => {
            assert_ne!(replacement.as_deref(), Some(trust));
            grant.session["token"].as_str().unwrap().to_owned()
        }
        CredentialSigninOutcome::Challenge(_) => panic!("valid trusted device bypasses challenge"),
    };
    assert!(matches!(
        service
            .sign_in_with_two_factor(signin(), &factor, Some(trust), "", "", now)
            .await
            .unwrap(),
        CredentialSigninOutcome::Challenge(_)
    ));
    let backup = enrollment.body()["backupCodes"][0].as_str().unwrap();
    assert!(
        failed
            .verify(&trusted_token, "", backup, true, false, now)
            .await
            .is_err()
    );
    factor
        .verify(&trusted_token, "", backup, true, false, now)
        .await
        .unwrap();
    assert_eq!(
        factor
            .verify(&trusted_token, "", backup, true, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_BACKUP_CODE"
    );
    let skip = BackupVerificationOptions {
        disable_session: true,
        trust_device: true,
    };
    let authenticated_skip = factor
        .verify_backup_with_options(
            &trusted_token,
            "",
            enrollment.body()["backupCodes"][1].as_str().unwrap(),
            skip,
            now,
        )
        .await
        .unwrap();
    assert_eq!(authenticated_skip.body()["token"], trusted_token);
    assert!(authenticated_skip.session.is_none());
    assert!(authenticated_skip.trust.is_none());
    let pending_skip = challenge(&service, &factor, now).await;
    let skip_code = enrollment.body()["backupCodes"][2].as_str().unwrap();
    let before_sessions = rows(store.as_ref(), Model::Session).await;
    assert!(
        failed
            .verify_backup_with_options("", pending_skip.identifier(), skip_code, skip, now)
            .await
            .is_err()
    );
    let skipped = factor
        .verify_backup_with_options("", pending_skip.identifier(), skip_code, skip, now)
        .await
        .unwrap();
    assert!(skipped.body().get("token").is_none());
    assert!(skipped.session.is_none());
    assert!(skipped.trust.is_none());
    assert!(!skipped.clear_challenge);
    assert_eq!(rows(store.as_ref(), Model::Session).await, before_sessions);
    assert_eq!(
        factor
            .verify(
                "",
                pending_skip.identifier(),
                &factor_code(store.as_ref(), now).await,
                false,
                true,
                now
            )
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_TWO_FACTOR_COOKIE"
    );
    assert_eq!(
        factor
            .verify_backup_with_options(&trusted_token, "", skip_code, skip, now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_BACKUP_CODE"
    );
    for round in 0..2 {
        let pending = challenge(&service, &factor, now).await;
        for _ in 0..5 {
            assert_eq!(
                factor
                    .verify("", pending.identifier(), "invalid", false, false, now)
                    .await
                    .err()
                    .unwrap()
                    .code,
                "INVALID_CODE"
            );
        }
        assert_eq!(
            factor
                .verify("", pending.identifier(), &code, false, false, now)
                .await
                .err()
                .unwrap()
                .code,
            if round == 0 {
                "TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE"
            } else {
                "ACCOUNT_TEMPORARILY_LOCKED"
            }
        );
    }
    let later = now + Duration::seconds(901);
    let pending = challenge(&service, &factor, later).await;
    let code = factor_code(store.as_ref(), later).await;
    let unlocked = factor
        .verify("", pending.identifier(), &code, false, false, later)
        .await
        .unwrap();
    assert_eq!(
        rows(store.as_ref(), Model::TwoFactor).await[0]["failedVerificationCount"],
        0
    );
    let current = unlocked.replacement_session_token().unwrap();
    let new_codes = factor
        .regenerate_backup_codes(current, PASSWORD.into(), later)
        .await
        .unwrap();
    assert_eq!(
        new_codes.body()["backupCodes"].as_array().unwrap().len(),
        10
    );
    let sessions_before = rows(store.as_ref(), Model::Session).await.len();
    factor
        .disable(current, PASSWORD.into(), None, later)
        .await
        .unwrap();
    assert!(rows(store.as_ref(), Model::TwoFactor).await.is_empty());
    assert_eq!(
        rows(store.as_ref(), Model::Session).await.len(),
        sessions_before
    );
    service.sign_in(signin(), "", "", later).await.unwrap();
}
#[tokio::test]
async fn default_totp_lifecycle_and_atomic_failures() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}

#[tokio::test]
async fn pending_factor_expiry_disabled_user_and_backup_race() {
    let store = Arc::new(MemoryAuthStore::default());
    let factor = TwoFactorService::new(store.clone(), vec![42; 32], "Better Auth".into()).unwrap();
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let user =
        json!({"id":"boundary-user","email":"boundary@example.test","twoFactorEnabled":true});
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User, user.clone()).await.unwrap();
    tx.insert(Model::TwoFactor,json!({"id":"boundary-factor","userId":"boundary-user","secret":STANDARD.encode([7u8;32]),"backupCodes":["ABCDE-FGHIJ"],"verified":true,"failedVerificationCount":0,"lockedUntil":null})).await.unwrap();
    let mut challenges = vec![];
    for _ in 0..3 {
        match factor
            .gate(&mut tx, &user, None, true, "", "", now)
            .await
            .unwrap()
        {
            signin::SigninGate::Challenge(challenge) => challenges.push(challenge),
            signin::SigninGate::Trusted(_) => panic!("no trust credential supplied"),
        }
    }
    tx.commit().await.unwrap();
    assert_eq!(
        factor
            .verify(
                "",
                challenges[0].identifier(),
                "ABCDE-FGHIJ",
                true,
                false,
                now + Duration::seconds(600) + Duration::milliseconds(1)
            )
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_TWO_FACTOR_COOKIE"
    );
    let foreign = TwoFactorService::new(
        Arc::new(MemoryAuthStore::default()),
        vec![42; 32],
        "Better Auth".into(),
    )
    .unwrap();
    assert_eq!(
        foreign
            .verify(
                "",
                challenges[0].identifier(),
                "ABCDE-FGHIJ",
                true,
                false,
                now
            )
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_TWO_FACTOR_COOKIE"
    );
    let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        tokio::join!(
            factor.verify(
                "",
                challenges[0].identifier(),
                "ABCDE-FGHIJ",
                true,
                false,
                now
            ),
            factor.verify(
                "",
                challenges[1].identifier(),
                "ABCDE-FGHIJ",
                true,
                false,
                now
            )
        )
    })
    .await
    .unwrap();
    assert_ne!(first.is_ok(), second.is_ok());
    assert_eq!(rows(store.as_ref(), Model::Session).await.len(), 1);
    let mut tx = store.begin(true).await.unwrap();
    let mut disabled = user;
    disabled["twoFactorEnabled"] = json!(false);
    tx.replace(Model::User, disabled).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        factor
            .verify("", challenges[2].identifier(), "000000", false, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_TWO_FACTOR_COOKIE"
    );
    assert_eq!(rows(store.as_ref(), Model::Session).await.len(), 1);
}

#[tokio::test]
async fn rotation_after_elapsed_time_resets_configured_session_dates() {
    let store = MemoryAuthStore::default();
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let old = new_session("rotation-user", "", "", now).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Session, old.clone()).await.unwrap();
    let later = now + Duration::minutes(5);
    let rotated = enrollment::rotate(&mut tx, &json!({"id":"rotation-user"}), &old, later)
        .await
        .unwrap();
    assert_eq!(time(&rotated, "createdAt").unwrap(), later);
    assert_eq!(
        time(&rotated, "expiresAt").unwrap(),
        later + Duration::days(1)
    );
    assert_ne!(rotated["expiresAt"], old["expiresAt"]);
    assert!(
        tx.get(Model::Session, text(&old, "id").unwrap())
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
}
