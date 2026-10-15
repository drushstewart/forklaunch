use super::*;

fn service<S: AuthStore>(store: Arc<S>, policy: BackupCodeStorage) -> TwoFactorService<S> {
    TwoFactorService::new(store, vec![42; 32], "Backup codecs".into())
        .unwrap()
        .with_backup_code_storage(policy)
        .with_lifecycle_options(
            TwoFactorLifecycleOptions::default()
                .with_passwordless(true)
                .with_skip_verification_on_enable(true),
        )
        .with_backup_code_generator(Arc::new(|| {
            Ok(vec![
                "same".into(),
                "same".into(),
                String::new(),
                "é🔐".into(),
                "race".into(),
                "rollback".into(),
            ])
        }))
}
#[test]
fn backup_storage_vectors_limits_and_version_errors() {
    let nonce: Vec<u8> = (0..24).collect();
    let vectors: Vec<Value> =
        serde_json::from_str(include_str!("../backup_encryption_vectors.json")).unwrap();
    for vector in vectors {
        let keys =
            BackupCodeEncryptionKeys::legacy(vector["secret"].as_str().unwrap().into()).unwrap();
        let plain = serde_json::to_string(&vector["codes"]).unwrap();
        let ciphertext = keys.encrypt_nonce(&plain, &nonce).unwrap();
        assert_eq!(ciphertext.as_str(), vector["ciphertext"].as_str().unwrap());
        assert_eq!(keys.decrypt(&ciphertext).unwrap().as_str(), plain);
        assert_eq!(
            keys.decrypt(&ciphertext.to_uppercase()).unwrap().as_str(),
            plain
        );
    }
    let keys =
        BackupCodeEncryptionKeys::versioned(u32::MAX, vec![(u32::MAX, "x".repeat(32))], None)
            .unwrap();
    let plain = serde_json::to_string(&vec!["\0".repeat(128); 100]).unwrap();
    assert_eq!(plain.len(), 77_101);
    let cipher = keys.encrypt_nonce(&plain, &nonce).unwrap();
    assert_eq!(cipher.len(), 154_297);
    assert_eq!(keys.decrypt(&cipher).unwrap().as_str(), plain);
    let permissive = cipher.replace("$4294967295$", &format!("$ +4294967295{}$", "j".repeat(31)));
    assert_eq!(keys.decrypt(&permissive).unwrap().as_str(), plain);
    let excessive = cipher.replace("$4294967295$", &format!("$4294967295{}$", "j".repeat(40)));
    assert!(keys.decrypt(&excessive).is_err());
    assert!(keys.encrypt_nonce(&"x".repeat(77_102), &nonce).is_err());
    for bad in [
        cipher.replace("$4294967295$", "$1$"),
        cipher.replace("$4294967295$", "$-1$"),
        format!(
            "{}{}",
            &cipher[..cipher.len() - 1],
            if cipher.ends_with('0') { '1' } else { '0' }
        ),
    ] {
        assert_eq!(
            keys.decrypt(&bad).err().unwrap().code,
            "TWO_FACTOR_BACKUP_STORAGE_FAILED"
        );
    }
    for secret in [String::new(), "x".repeat(31), "x".repeat(4097)] {
        assert!(BackupCodeEncryptionKeys::legacy(secret).is_err());
    }
    assert!(BackupCodeEncryptionKeys::versioned(1, vec![], None).is_err());
    assert!(BackupCodeEncryptionKeys::versioned(2, vec![(1, "x".repeat(32))], None).is_err());
    assert!(
        BackupCodeEncryptionKeys::versioned(
            1,
            vec![(1, "x".repeat(32)), (1, "y".repeat(32))],
            None
        )
        .is_err()
    );
}
#[test]
fn backup_storage_typed_shape_current_policy_and_unknown_marker() {
    let store = Arc::new(MemoryAuthStore::default());
    for mode in [
        BackupCodeStorage::LegacyArray,
        BackupCodeStorage::PlainJson,
        BackupCodeStorage::Encrypted,
    ] {
        let factor = service(store.clone(), mode);
        let codes = vec![String::new(), "é🔐".into()];
        let mut row = json!({"backupCodes":codes});
        assert_eq!(factor.read_backup_codes(&row).unwrap().unwrap(), codes);
        factor.write_backup_codes(&mut row, &codes).unwrap();
        assert_eq!(factor.read_backup_codes(&row).unwrap().unwrap(), codes);
        row["backupCodesFormat"] = json!(2);
        assert!(factor.read_backup_codes(&row).is_err());
    }
    let factor = service(store.clone(), BackupCodeStorage::PlainJson);
    for field in ["not json", "null"] {
        assert!(
            factor
                .read_backup_codes(&json!({"backupCodesFormat":1,"backupCodes":field}))
                .unwrap()
                .is_none()
        );
    }
    for field in ["{}", "[1]"] {
        assert!(
            factor
                .read_backup_codes(&json!({"backupCodesFormat":1,"backupCodes":field}))
                .is_err()
        );
    }
    assert!(
        factor
            .read_backup_codes(&json!({"backupCodes":"[]"}))
            .is_err()
    );
    let mut row = json!({});
    assert!(
        factor
            .write_backup_codes(&mut row, &vec!["x".into(); 101])
            .is_err()
    );
    assert!(
        factor
            .write_backup_codes(&mut row, &["é".repeat(65)])
            .is_err()
    );
    let encrypted = service(store, BackupCodeStorage::Encrypted);
    encrypted
        .write_backup_codes(&mut row, &vec!["\0".repeat(128); 100])
        .unwrap();
    assert!(factor.read_backup_codes(&row).unwrap().is_none());
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
async fn challenge<S: AuthStore>(
    factor: &TwoFactorService<S>,
    id: &str,
    now: DateTime<Utc>,
) -> String {
    let mut tx = factor.store.begin(true).await.unwrap();
    let user = tx.get(Model::User, id).await.unwrap().unwrap();
    let signin::SigninGate::Challenge(proof) = factor
        .gate(&mut tx, &user, None, true, "", "", now)
        .await
        .unwrap()
    else {
        panic!("challenge")
    };
    tx.commit().await.unwrap();
    proof.identifier().into()
}
#[tokio::test]
async fn backup_storage_lifecycle_migration_failures_races_and_rollback() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
#[allow(clippy::too_many_lines)]
pub(super) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for (index, mode) in [
        BackupCodeStorage::LegacyArray,
        BackupCodeStorage::PlainJson,
        BackupCodeStorage::Encrypted,
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("backup-codec-user-{index}");
        let session = new_session(&id, "", "", now).unwrap();
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(
            Model::User,
            json!({"id":id,"email":format!("{id}@example.test"),"twoFactorEnabled":false}),
        )
        .await
        .unwrap();
        tx.insert(Model::Session, session.clone()).await.unwrap();
        tx.commit().await.unwrap();
        let factor = Arc::new(service(store.clone(), mode));
        let enrollment = factor
            .enable(text(&session, "token").unwrap(), String::new(), None, now)
            .await
            .unwrap();
        let token = enrollment.replacement_session_token().unwrap();
        let before = saved(store.as_ref(), &id).await;
        assert_eq!(
            before["backupCodes"].is_array(),
            mode == BackupCodeStorage::LegacyArray
        );
        if mode != BackupCodeStorage::LegacyArray {
            let switched = service(
                store.clone(),
                if mode == BackupCodeStorage::PlainJson {
                    BackupCodeStorage::Encrypted
                } else {
                    BackupCodeStorage::PlainJson
                },
            );
            let pending = challenge(&factor, &id, now).await;
            let fail = switched
                .verify("", &pending, "same", true, true, now)
                .await
                .err()
                .unwrap();
            assert_eq!(
                fail.code,
                if mode == BackupCodeStorage::PlainJson {
                    "TWO_FACTOR_BACKUP_STORAGE_FAILED"
                } else {
                    "INVALID_BACKUP_CODE"
                }
            );
            assert_eq!(
                saved(store.as_ref(), &id).await["backupCodes"],
                before["backupCodes"]
            );
            let mut tx = store.begin(false).await.unwrap();
            let proof = verification_row(&mut tx, &pending).await.unwrap().unwrap();
            assert_eq!(
                proof["attempts"],
                i32::from(mode != BackupCodeStorage::PlainJson)
            );
            tx.commit().await.unwrap();
        }
        factor
            .verify(token, "", "same", true, false, now)
            .await
            .unwrap();
        let row = saved(store.as_ref(), &id).await;
        assert!(
            !factor
                .read_backup_codes(&row)
                .unwrap()
                .unwrap()
                .contains(&"same".into())
        );
        factor
            .verify(token, "", "", true, false, now)
            .await
            .unwrap();
        factor
            .verify(token, "", "é🔐", true, false, now)
            .await
            .unwrap();
        let a = challenge(&factor, &id, now).await;
        let b = challenge(&factor, &id, now).await;
        let (first, second) = tokio::join!(
            factor.verify("", &a, "race", true, true, now),
            factor.verify("", &b, "race", true, true, now)
        );
        assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
        let pending = challenge(&factor, &id, now).await;
        let before = saved(store.as_ref(), &id).await;
        let sessions = rows(store.as_ref(), Model::Session).await;
        let proofs = rows(store.as_ref(), Model::Verification).await;
        let faulty = service(
            Arc::new(fault_store::FaultStore {
                store: store.clone(),
                read: None,
                page: None,
                delete: None,
                insert: None,
                begin: false,
                commit: true,
            }),
            mode,
        );
        assert!(
            faulty
                .verify("", &pending, "rollback", true, true, now)
                .await
                .is_err()
        );
        assert_eq!(saved(store.as_ref(), &id).await, before);
        assert_eq!(rows(store.as_ref(), Model::Session).await, sessions);
        assert_eq!(rows(store.as_ref(), Model::Verification).await, proofs);
        factor
            .verify("", &pending, "rollback", true, true, now)
            .await
            .unwrap();
        let migrated = service(store.clone(), BackupCodeStorage::Encrypted);
        // Regeneration is authorized by the original, still-current session.
        migrated
            .regenerate_backup_codes(token, String::new(), now)
            .await
            .unwrap();
        let regenerated = saved(store.as_ref(), &id).await;
        assert!(regenerated["backupCodes"].is_string());
        assert_eq!(regenerated["backupCodesFormat"], 1);
        if mode == BackupCodeStorage::LegacyArray {
            // Explicit legacy rewrite, then successful consumption under encrypted policy migrates it.
            factor
                .regenerate_backup_codes(token, String::new(), now)
                .await
                .unwrap();
            assert!(saved(store.as_ref(), &id).await["backupCodes"].is_array());
            migrated
                .verify(token, "", "same", true, false, now)
                .await
                .unwrap();
            assert!(saved(store.as_ref(), &id).await["backupCodes"].is_string());
        }
        let original = saved(store.as_ref(), &id).await;
        for value in ["not hex".to_owned(), "0".repeat(80)] {
            let mut damaged = original.clone();
            damaged["backupCodes"] = json!(value);
            let mut tx = store.begin(true).await.unwrap();
            tx.replace(Model::TwoFactor, damaged.clone()).await.unwrap();
            tx.commit().await.unwrap();
            assert_eq!(
                migrated
                    .verify(token, "", "race", true, false, now)
                    .await
                    .err()
                    .unwrap()
                    .code,
                "TWO_FACTOR_BACKUP_STORAGE_FAILED"
            );
            assert_eq!(saved(store.as_ref(), &id).await, damaged);
        }
        let mut tx = store.begin(true).await.unwrap();
        tx.replace(Model::TwoFactor, original).await.unwrap();
        tx.commit().await.unwrap();
        migrated
            .verify(token, "", "race", true, false, now)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn backup_storage_key_rotation_preserves_failed_proof_then_reencodes_current() {
    let store = Arc::new(MemoryAuthStore::default());
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let session = new_session("backup-keys-user", "", "", now).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User,json!({"id":"backup-keys-user","email":"backup-keys@example.test","twoFactorEnabled":false})).await.unwrap();
    tx.insert(Model::Session, session.clone()).await.unwrap();
    tx.commit().await.unwrap();
    let factor = service(store.clone(), BackupCodeStorage::Encrypted)
        .with_backup_code_encryption_keys(Arc::new(
            BackupCodeEncryptionKeys::versioned(7, vec![(7, "x".repeat(32))], None).unwrap(),
        ));
    let enrolled = factor
        .enable(text(&session, "token").unwrap(), String::new(), None, now)
        .await
        .unwrap();
    let token = enrolled.replacement_session_token().unwrap();
    let before = saved(store.as_ref(), "backup-keys-user").await;
    let pending = challenge(&factor, "backup-keys-user", now).await;
    let proofs = rows(store.as_ref(), Model::Verification).await;
    for keys in [
        BackupCodeEncryptionKeys::legacy("y".repeat(32)).unwrap(),
        BackupCodeEncryptionKeys::versioned(7, vec![(7, "y".repeat(32))], None).unwrap(),
        BackupCodeEncryptionKeys::versioned(8, vec![(8, "z".repeat(32))], None).unwrap(),
    ] {
        let wrong = service(store.clone(), BackupCodeStorage::Encrypted)
            .with_backup_code_encryption_keys(Arc::new(keys));
        assert_eq!(
            wrong
                .verify("", &pending, "same", true, true, now)
                .await
                .err()
                .unwrap()
                .code,
            "TWO_FACTOR_BACKUP_STORAGE_FAILED"
        );
        assert_eq!(saved(store.as_ref(), "backup-keys-user").await, before);
        assert_eq!(rows(store.as_ref(), Model::Verification).await, proofs);
    }
    let rotated = service(store.clone(), BackupCodeStorage::Encrypted)
        .with_backup_code_encryption_keys(Arc::new(
            BackupCodeEncryptionKeys::versioned(
                8,
                vec![(8, "z".repeat(32)), (7, "x".repeat(32))],
                None,
            )
            .unwrap(),
        ));
    rotated
        .verify(token, "", "same", true, false, now)
        .await
        .unwrap();
    let after = saved(store.as_ref(), "backup-keys-user").await;
    assert!(after["backupCodes"].as_str().unwrap().starts_with("$ba$8$"));
    assert!(
        !rotated
            .read_backup_codes(&after)
            .unwrap()
            .unwrap()
            .contains(&"same".into())
    );
}
