use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

async fn seed<S: AuthStore>(store: &S, id: &str, codes: Value, format: Option<u8>) {
    let mut tx = store.begin(true).await.unwrap();
    let mut row =
        json!({"id":id,"userId":id,"secret":"unused-secret","verified":false,"backupCodes":codes});
    if let Some(format) = format {
        row["backupCodesFormat"] = json!(format);
    }
    tx.insert(Model::TwoFactor, row).await.unwrap();
    tx.commit().await.unwrap();
}
async fn rows<S: AuthStore>(store: &S, model: Model) -> Vec<Value> {
    let mut tx = store.begin(false).await.unwrap();
    let rows = tx.list(model).await.unwrap();
    tx.commit().await.unwrap();
    rows
}
pub(super) async fn readonly<S: AuthStore + 'static>(store: Arc<S>) {
    let service = TwoFactorService::new(store.clone(), vec![42; 32], "Read".into()).unwrap();
    seed(
        store.as_ref(),
        "view-legacy",
        json!(["one", "one", "", "é"]),
        None,
    )
    .await;
    let before = rows(store.as_ref(), Model::TwoFactor).await;
    let result = service
        .view_backup_codes_for_user("view-legacy")
        .await
        .unwrap();
    assert_eq!(result.codes().collect::<Vec<_>>(), ["one", "one", "", "é"]);
    assert_eq!(result.len(), 4);
    assert!(!result.is_empty());
    assert_eq!(rows(store.as_ref(), Model::TwoFactor).await, before);
    assert_eq!(
        service
            .view_backup_codes_for_user("missing")
            .await
            .err()
            .unwrap()
            .code,
        "BACKUP_CODES_NOT_ENABLED"
    );
    for (id, storage) in [
        ("view-plain", BackupCodeStorage::PlainJson),
        ("view-encrypted", BackupCodeStorage::Encrypted),
    ] {
        let service = TwoFactorService::new(store.clone(), vec![42; 32], "Read".into())
            .unwrap()
            .with_backup_code_storage(storage);
        let mut row = json!({"id":id,"userId":id,"secret":"unused","verified":false});
        service
            .write_backup_codes(&mut row, &["code".into()])
            .unwrap();
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(Model::TwoFactor, row.clone()).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(
            service
                .view_backup_codes_for_user(id)
                .await
                .unwrap()
                .codes()
                .collect::<Vec<_>>(),
            ["code"]
        );
        let mut tx = store.begin(false).await.unwrap();
        assert_eq!(tx.get(Model::TwoFactor, id).await.unwrap(), Some(row));
        tx.commit().await.unwrap();
    }
    seed(store.as_ref(), "view-invalid", json!("null"), Some(1)).await;
    assert_eq!(
        service
            .view_backup_codes_for_user("view-invalid")
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_BACKUP_CODE"
    );
    for deadline in [0, 9_000_000_000_000_i64] {
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(Model::Verification,json!({"id":"view-lease","identifier":backup_codec_state::identifier("view-legacy"),"expiresAt":deadline,"value":"recognized-but-malformed"})).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(
            service
                .view_backup_codes_for_user("view-legacy")
                .await
                .err()
                .unwrap()
                .code,
            "TWO_FACTOR_BACKUP_CODEC_CONFLICT"
        );
        let mut tx = store.begin(true).await.unwrap();
        assert!(
            tx.get(Model::Verification, "view-lease")
                .await
                .unwrap()
                .is_some()
        );
        tx.delete(Model::Verification, "view-lease").await.unwrap();
        tx.commit().await.unwrap();
    }
}
#[tokio::test]
async fn server_only_backup_view_builtin_readonly_profiles() {
    readonly(Arc::new(MemoryAuthStore::default())).await;
}

#[derive(Default)]
struct Gate {
    calls: AtomicUsize,
    entered: Notify,
    release: Notify,
}
struct Decode(Arc<Gate>);
impl BackupCodeEncryptionCodec for Decode {
    fn encrypt(&self, _: BackupCodeCodecText) -> BackupCodeCodecFuture {
        panic!("read must not encode")
    }
    fn decrypt(&self, _: BackupCodeCodecText) -> BackupCodeCodecFuture {
        let gate = self.0.clone();
        Box::pin(async move {
            gate.calls.fetch_add(1, Ordering::SeqCst);
            gate.entered.notify_one();
            gate.release.notified().await;
            BackupCodeCodecText::new("[\"owned\"]".into())
        })
    }
}
#[tokio::test]
async fn server_only_backup_view_callback_rechecks_generation_and_reservation() {
    for action in ["same", "delete", "regenerate", "lease", "cancel"] {
        let store = Arc::new(MemoryAuthStore::default());
        seed(store.as_ref(), "target", json!("cipher"), Some(2)).await;
        let gate = Arc::new(Gate::default());
        let service = TwoFactorService::new(store.clone(), vec![42; 32], "Read".into())
            .unwrap()
            .with_backup_code_encryption_codec(
                Arc::new(Decode(gate.clone())),
                BackupCodeCodecOptions::default(),
            );
        let task = tokio::spawn(async move { service.view_backup_codes_for_user("target").await });
        gate.entered.notified().await;
        // This writer acquires while the callback is pending: no transaction crosses await.
        let mut tx = store.begin(true).await.unwrap();
        match action {
            "delete" => {
                tx.delete(Model::TwoFactor, "target").await.unwrap();
            }
            "regenerate" => {
                let mut row = tx.get(Model::TwoFactor, "target").await.unwrap().unwrap();
                row["backupCodesRevision"] = json!("new-generation-same-cipher");
                tx.replace(Model::TwoFactor, row).await.unwrap();
            }
            "lease" => {
                tx.insert(Model::Verification,json!({"id":"lease","identifier":backup_codec_state::identifier("target"),"value":"reservation"})).await.unwrap();
            }
            _ => {}
        }
        tx.commit().await.unwrap();
        let before = rows(store.as_ref(), Model::TwoFactor).await;
        if action == "cancel" {
            task.abort();
            assert!(task.await.err().unwrap().is_cancelled());
        } else {
            gate.release.notify_one();
            let result = task.await.unwrap();
            if action == "same" {
                assert_eq!(result.unwrap().codes().collect::<Vec<_>>(), ["owned"]);
            } else {
                assert_eq!(
                    result.err().unwrap().code,
                    "TWO_FACTOR_BACKUP_CODEC_CONFLICT"
                );
            }
        }
        assert_eq!(rows(store.as_ref(), Model::TwoFactor).await, before);
        assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn server_only_backup_view_pending_timeout_has_no_writes() {
    let store = Arc::new(MemoryAuthStore::default());
    seed(store.as_ref(), "target", json!("cipher"), Some(2)).await;
    let gate = Arc::new(Gate::default());
    let service = TwoFactorService::new(store.clone(), vec![42; 32], "Read".into())
        .unwrap()
        .with_backup_code_encryption_codec(
            Arc::new(Decode(gate)),
            BackupCodeCodecOptions::new(std::time::Duration::from_millis(50)).unwrap(),
        );
    let before = rows(store.as_ref(), Model::TwoFactor).await;
    assert_eq!(
        service
            .view_backup_codes_for_user("target")
            .await
            .err()
            .unwrap()
            .code,
        "TWO_FACTOR_BACKUP_CODEC_FAILED"
    );
    assert_eq!(rows(store.as_ref(), Model::TwoFactor).await, before);
    assert!(rows(store.as_ref(), Model::Verification).await.is_empty());
}

struct Immediate(&'static str);
impl BackupCodeEncryptionCodec for Immediate {
    fn encrypt(&self, _: BackupCodeCodecText) -> BackupCodeCodecFuture {
        panic!("read must not encode")
    }
    fn decrypt(&self, _: BackupCodeCodecText) -> BackupCodeCodecFuture {
        let value = self.0;
        Box::pin(async move {
            if value == "error" {
                return Err(error("private-error", "secret callback data"));
            }
            BackupCodeCodecText::new(value.into())
        })
    }
}
#[tokio::test]
async fn server_only_backup_view_errors_and_preexisting_lease_never_mutate() {
    let store = Arc::new(MemoryAuthStore::default());
    seed(store.as_ref(), "target", json!("cipher"), Some(2)).await;
    let before = rows(store.as_ref(), Model::TwoFactor).await;
    for (output, expected) in [
        ("error", "TWO_FACTOR_BACKUP_CODEC_FAILED"),
        ("null", "INVALID_BACKUP_CODE"),
        ("invalid", "INVALID_BACKUP_CODE"),
        ("{}", "TWO_FACTOR_BACKUP_STORAGE_FAILED"),
    ] {
        let service = TwoFactorService::new(store.clone(), vec![42; 32], "Read".into())
            .unwrap()
            .with_backup_code_encryption_codec(
                Arc::new(Immediate(output)),
                BackupCodeCodecOptions::default(),
            );
        assert_eq!(
            service
                .view_backup_codes_for_user("target")
                .await
                .err()
                .unwrap()
                .code,
            expected
        );
        assert_eq!(rows(store.as_ref(), Model::TwoFactor).await, before);
        assert!(rows(store.as_ref(), Model::Verification).await.is_empty());
    }
    let gate = Arc::new(Gate::default());
    let service = TwoFactorService::new(store.clone(), vec![42; 32], "Read".into())
        .unwrap()
        .with_backup_code_encryption_codec(
            Arc::new(Decode(gate.clone())),
            BackupCodeCodecOptions::default(),
        );
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::Verification,
        json!({"id":"lease","identifier":backup_codec_state::identifier("target"),"expiresAt":0}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        service
            .view_backup_codes_for_user("target")
            .await
            .err()
            .unwrap()
            .code,
        "TWO_FACTOR_BACKUP_CODEC_CONFLICT"
    );
    assert_eq!(gate.calls.load(Ordering::SeqCst), 0);
    assert_eq!(rows(store.as_ref(), Model::Verification).await.len(), 1);
}

#[tokio::test]
async fn server_only_backup_view_final_clock_rejects_without_side_effects() {
    use std::sync::atomic::AtomicI64;
    let store = Arc::new(MemoryAuthStore::default());
    seed(store.as_ref(), "target", json!("cipher"), Some(2)).await;
    let now = Arc::new(AtomicI64::new(1_800_000_000_000));
    let gate = Arc::new(Gate::default());
    let service = TwoFactorService::new(store.clone(), vec![42; 32], "Read".into())
        .unwrap()
        .with_backup_code_encryption_codec(
            Arc::new(Decode(gate.clone())),
            BackupCodeCodecOptions::default(),
        );
    let time = now.clone();
    let task = tokio::spawn(async move {
        service
            .view_backup_codes_for_user_with_clock("target", &move || {
                DateTime::from_timestamp_millis(time.load(Ordering::SeqCst)).unwrap()
            })
            .await
    });
    gate.entered.notified().await;
    now.fetch_add(30_000, Ordering::SeqCst);
    gate.release.notify_one();
    assert_eq!(
        task.await.unwrap().err().unwrap().code,
        "TWO_FACTOR_BACKUP_CODEC_CONFLICT"
    );
    assert!(rows(store.as_ref(), Model::Verification).await.is_empty());
    assert_eq!(
        rows(store.as_ref(), Model::TwoFactor).await[0]["backupCodes"],
        json!("cipher")
    );
}

#[tokio::test]
async fn server_only_backup_view_falsy_values_and_final_read_generation() {
    let store = Arc::new(MemoryAuthStore::default());
    let plain = TwoFactorService::new(store.clone(), vec![42; 32], "Read".into()).unwrap();
    for (index, text) in ["false", "0", "\"\"", "null"].into_iter().enumerate() {
        let id = format!("falsy-{index}");
        seed(store.as_ref(), &id, json!(text), Some(1)).await;
        assert_eq!(
            plain
                .view_backup_codes_for_user(&id)
                .await
                .err()
                .unwrap()
                .code,
            "INVALID_BACKUP_CODE"
        );
        let custom = TwoFactorService::new(store.clone(), vec![42; 32], "Read".into())
            .unwrap()
            .with_backup_code_encryption_codec(
                Arc::new(Immediate(text)),
                BackupCodeCodecOptions::default(),
            );
        assert_eq!(
            custom
                .view_backup_codes_for_user(&id)
                .await
                .err()
                .unwrap()
                .code,
            "INVALID_BACKUP_CODE"
        );
    }
    seed(store.as_ref(), "generation", json!(["old"]), None).await;
    let mut tx = store.begin(true).await.unwrap();
    let mut row = tx
        .get(Model::TwoFactor, "generation")
        .await
        .unwrap()
        .unwrap();
    let generation = backup_codec_state::factor_digest(&row).unwrap();
    row["backupCodes"] = json!(["new"]);
    tx.replace(Model::TwoFactor, row).await.unwrap();
    assert_eq!(
        super::super::backup_view::recheck(&mut tx, "generation", &generation)
            .await
            .err()
            .unwrap()
            .code,
        "TWO_FACTOR_BACKUP_CODEC_CONFLICT"
    );
    tx.commit().await.unwrap();
    let gate = Arc::new(Gate::default());
    let custom = TwoFactorService::new(store.clone(), vec![42; 32], "Read".into())
        .unwrap()
        .with_backup_code_encryption_codec(
            Arc::new(Decode(gate.clone())),
            BackupCodeCodecOptions::default(),
        );
    assert_eq!(
        custom
            .view_backup_codes_for_user("generation")
            .await
            .unwrap()
            .codes()
            .collect::<Vec<_>>(),
        ["new"]
    );
    assert_eq!(gate.calls.load(Ordering::SeqCst), 0);
}
