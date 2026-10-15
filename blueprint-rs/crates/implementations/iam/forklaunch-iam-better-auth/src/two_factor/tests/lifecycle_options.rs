use super::*;

#[test]
fn lifecycle_issuer_bounds_and_passwordless_inheritance() {
    assert!(
        TwoFactorLifecycleOptions::default()
            .with_issuers(Some("x".repeat(257)), None)
            .is_err()
    );
    let policy = TwoFactorLifecycleOptions::default()
        .with_passwordless(true)
        .with_uri_passwordless(Some(false));
    assert!(policy.password_optional("/two-factor/enable"));
    assert!(!policy.password_optional("/two-factor/get-totp-uri"));
    assert!(policy.password_optional("/two-factor/generate-backup-codes"));
    let policy = policy
        .with_issuers(Some(String::new()), Some(String::new()))
        .unwrap();
    assert_eq!(
        policy.enrollment_issuer(Some(""), "Application"),
        "Application"
    );
    assert_eq!(policy.uri_issuer("Application"), "Application");
}

#[allow(clippy::too_many_lines)]
pub(super) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let id = "lifecycle-options-user";
    let user = json!({"id":id,"email":"lifecycle@example.test","twoFactorEnabled":false});
    let initial = new_session(id, "", "", now).unwrap();
    let other = new_session(id, "", "", now).unwrap();
    let token = text(&initial, "token").unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User, user.clone()).await.unwrap();
    tx.insert(Model::Session, initial.clone()).await.unwrap();
    tx.insert(Model::Session, other.clone()).await.unwrap();
    tx.commit().await.unwrap();
    let policy = TwoFactorLifecycleOptions::default()
        .with_passwordless(true)
        .with_skip_verification_on_enable(true)
        .with_issuers(Some("Enroll issuer".into()), Some("URI issuer".into()))
        .unwrap();
    let factor = TwoFactorService::new(store.clone(), vec![42; 32], "Application".into())
        .unwrap()
        .with_lifecycle_options(policy.clone());
    // A real second writer adds a credential after the absence snapshot commits.
    let snapshot = factor
        .password_snapshot(token, String::new(), now, PasswordAction::General)
        .await
        .unwrap();
    let writer = store.clone();
    tokio::spawn(async move {
        let mut tx=writer.begin(true).await.unwrap();tx.insert(Model::Account,json!({"id":"new-password-account","userId":id,"accountId":id,"providerId":"credential","password":"new-nonempty-credential-hash"})).await.unwrap();tx.commit().await.unwrap();
    }).await.unwrap();
    let mut tx = store.begin(true).await.unwrap();
    assert_eq!(
        snapshot.check(&mut tx, &user).await.err().unwrap().code,
        "INVALID_PASSWORD"
    );
    drop(tx);
    assert!(
        factor
            .enable(token, String::new(), None, now)
            .await
            .is_err()
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.delete(Model::Account, "new-password-account")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    for password in [Value::Null, json!("")] {
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(Model::Account,json!({"id":"empty-password-account","userId":id,"accountId":id,"providerId":"credential","password":password})).await.unwrap();
        tx.commit().await.unwrap();
        let absent = factor
            .password_snapshot(token, String::new(), now, PasswordAction::General)
            .await
            .unwrap();
        let mut tx = store.begin(true).await.unwrap();
        absent.check(&mut tx, &user).await.unwrap();
        tx.delete(Model::Account, "empty-password-account")
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    // Failure after factor+user mutation but before rotation commit must roll all back.
    let failed = TwoFactorService::new(
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
        "Application".into(),
    )
    .unwrap()
    .with_lifecycle_options(policy.clone());
    assert!(
        failed
            .enable(token, String::new(), None, now)
            .await
            .is_err()
    );
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(tx.get(Model::User, id).await.unwrap().unwrap(), user);
    assert!(
        tx.find_unique(Model::TwoFactor, "userId", id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tx.get(Model::Session, text(&initial, "id").unwrap())
            .await
            .unwrap()
            .is_some()
    );
    tx.commit().await.unwrap();
    let enrollment = factor
        .enable(token, String::new(), Some(""), now)
        .await
        .unwrap();
    assert!(
        enrollment.body["totpURI"]
            .as_str()
            .unwrap()
            .contains("Enroll+issuer")
    );
    let replacement = enrollment.session.as_ref().unwrap();
    let next_token = text(replacement, "token").unwrap();
    assert_ne!(next_token, token);
    let mut tx = store.begin(false).await.unwrap();
    let enabled_user = tx.get(Model::User, id).await.unwrap().unwrap();
    assert_eq!(enabled_user["twoFactorEnabled"], true);
    assert_eq!(
        tx.find_unique(Model::TwoFactor, "userId", id)
            .await
            .unwrap()
            .unwrap()["verified"],
        true
    );
    assert!(
        tx.get(Model::Session, text(&initial, "id").unwrap())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tx.get(Model::Session, text(&other, "id").unwrap())
            .await
            .unwrap()
            .is_some()
    );
    tx.commit().await.unwrap();
    assert!(
        factor
            .get_uri(next_token, String::new(), now)
            .await
            .unwrap()
            .body["totpURI"]
            .as_str()
            .unwrap()
            .contains("URI+issuer")
    );
    let strict = TwoFactorService::new(store.clone(), vec![42; 32], "Application".into())
        .unwrap()
        .with_lifecycle_options(
            policy
                .clone()
                .with_uri_passwordless(Some(false))
                .with_backup_passwordless(Some(false)),
        );
    assert_eq!(
        strict
            .get_uri(next_token, String::new(), now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_PASSWORD"
    );
    assert_eq!(
        strict
            .regenerate_backup_codes(next_token, String::new(), now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_PASSWORD"
    );
    assert_eq!(
        strict
            .get_uri(next_token, "x".repeat(129), now)
            .await
            .err()
            .unwrap()
            .code,
        "PASSWORD_TOO_LONG"
    );
    assert!(
        factor
            .get_uri(next_token, "x".repeat(129), now)
            .await
            .is_ok()
    );
    let disabled = factor.with_lifecycle_options(policy.with_totp_disabled(true));
    assert_eq!(
        disabled
            .verify("", "", "123456", false, false, now)
            .await
            .err()
            .unwrap()
            .code,
        "TOTP_NOT_CONFIGURED"
    );
    assert_eq!(
        disabled
            .get_uri("", String::new(), now)
            .await
            .err()
            .unwrap()
            .code,
        "UNAUTHORIZED"
    );
    assert_eq!(
        disabled
            .get_uri(next_token, String::new(), now)
            .await
            .err()
            .unwrap()
            .code,
        "TOTP_NOT_CONFIGURED"
    );
    let mut tx = store.begin(true).await.unwrap();
    let trusted = disabled
        .create_trust(&mut tx, &enabled_user, now)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = store.begin(true).await.unwrap();
    let renewed = match disabled
        .gate(&mut tx, &enabled_user, Some(&trusted), true, "", "", now)
        .await
        .unwrap()
    {
        signin::SigninGate::Trusted(value) => value,
        signin::SigninGate::Challenge(_) => panic!("existing trust remains valid"),
    };
    tx.commit().await.unwrap();
    let mut tx = store.begin(true).await.unwrap();
    let pending = match disabled
        .gate(&mut tx, &enabled_user, None, true, "", "", now)
        .await
        .unwrap()
    {
        signin::SigninGate::Challenge(value) => value,
        signin::SigninGate::Trusted(_) => panic!("no trust supplied"),
    };
    assert!(pending.methods().is_empty());
    tx.commit().await.unwrap();
    let recovered = disabled
        .verify(
            "",
            pending.identifier(),
            enrollment.body["backupCodes"][0].as_str().unwrap(),
            true,
            false,
            now,
        )
        .await
        .unwrap();
    assert!(recovered.session.is_some());
    assert!(recovered.trust.is_none());
    let response = disabled
        .disable(
            text(recovered.session.as_ref().unwrap(), "token").unwrap(),
            String::new(),
            Some(&renewed),
            now,
        )
        .await
        .unwrap();
    assert!(response.clear_trust);
    assert!(response.session.is_some());
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::User, id).await.unwrap().unwrap()["twoFactorEnabled"],
        false
    );
    assert!(
        tx.find_unique(Model::TwoFactor, "userId", id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        verification_row(&mut tx, renewed.split_once('!').unwrap().1)
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn lifecycle_policy_atomic_enrollment_password_race_and_recovery() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
