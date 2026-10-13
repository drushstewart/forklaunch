use super::*;
use crate::store::{AuthCursor, AuthPage};
use serde_json::json;
#[tokio::test]
async fn classified_export_explicit_audit_and_scoped_erasure() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}
pub(crate) async fn exercise<S: AuthStore>(store: Arc<S>) {
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"compliance-owner","email":"compliance-owner@native.test"}),
    )
    .await
    .unwrap();
    tx.insert(Model::Session,json!({"id":"compliance-session","userId":"compliance-owner","token":"native-compliance-session"})).await.unwrap();
    tx.insert(Model::Account,json!({"id":"compliance-credential","userId":"compliance-owner","providerId":"credential","accountId":"compliance-owner","password":"native-compliance-password","accessToken":null})).await.unwrap();
    tx.insert(Model::Account,json!({"id":"compliance-provider","userId":"compliance-owner","providerId":"fixture","accountId":"subject","accessToken":"native-compliance-access","refreshToken":"native-compliance-refresh","idToken":"native-compliance-id"})).await.unwrap();
    tx.insert(Model::Account,json!({"id":"compliance-foreign","userId":"compliance-other","providerId":"fixture","accountId":"foreign","password":"native-compliance-foreign-private"})).await.unwrap();
    tx.insert(Model::Jwks,json!({"id":"compliance-owner","privateKey":"native-compliance-key","publicKey":"opaque-public"})).await.unwrap();
    tx.commit().await.unwrap();
    let service = ComplianceService::new(store.clone());
    let result = service.export("compliance-owner").await.unwrap();
    let encoded = serde_json::to_value(&result).unwrap();
    assert_eq!(encoded["entities"]["Account"].as_array().unwrap().len(), 2);
    assert_eq!(encoded["entities"]["Jwks"][0]["privateKey"], json!({}));
    assert!(result.failures.is_empty());
    assert!(!encoded.to_string().contains("native-compliance"));
    let mut audit = vec![];
    let field = result.entities["Account"]
        .iter()
        .find(|row| row.id == "compliance-provider")
        .unwrap()
        .field("accessToken")
        .unwrap();
    assert_eq!(
        field.deanon(|event| {
            assert_eq!(event.level, "pci");
            audit.push(event.field.to_owned());
        }),
        "native-compliance-access"
    );
    assert_eq!(audit, ["Account.accessToken"]);
    let erased = service.erase("compliance-owner").await.unwrap();
    assert_eq!(
        erased,
        EraseResult {
            entities_affected: vec!["Jwks".into(), "Account".into()],
            records_deleted: 3,
            failures: vec![]
        }
    );
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.get(Model::User, "compliance-owner")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        tx.get(Model::Session, "compliance-session")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        tx.get(Model::Account, "compliance-foreign")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        tx.get(Model::Jwks, "compliance-owner")
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
    assert!(
        service
            .export("compliance-owner")
            .await
            .unwrap()
            .entities
            .is_empty()
    );
    assert_eq!(
        service
            .erase("compliance-owner")
            .await
            .unwrap()
            .records_deleted,
        0
    );
}

#[path = "../../examples/http_parity/fault_store.rs"]
mod fault_store;
#[tokio::test]
async fn partial_reads_report_failures_and_write_failure_rolls_back_all_models() {
    let store = Arc::new(crate::store::memory::MemoryAuthStore::default());
    exercise_failures(store).await;
}
pub(crate) async fn exercise_failures<S: AuthStore>(store: Arc<S>) {
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::Jwks,
        json!({"id":"compliance-fault-owner","privateKey":"native-fault-private-key"}),
    )
    .await
    .unwrap();
    tx.insert(Model::Account,json!({"id":"compliance-fault-account","userId":"compliance-fault-owner","password":"native-fault-private-password"})).await.unwrap();
    tx.commit().await.unwrap();
    let faulty = Arc::new(fault_store::FaultStore {
        store: store.clone(),
        read: Some(Model::Account),
        delete: None,
    });
    let exported = ComplianceService::new(faulty)
        .with_tenant_label("trusted-fixture".into())
        .export("compliance-fault-owner")
        .await
        .unwrap();
    assert_eq!(exported.entities.len(), 1);
    assert!(exported.entities.contains_key("Jwks"));
    assert_eq!(
        exported.failures,
        [ComplianceFailure {
            entity_name: "Account".into(),
            reason: "Error: owned compliance read failure".into(),
            tenant_id: Some("trusted-fixture".into())
        }]
    );
    let faulty = Arc::new(fault_store::FaultStore {
        store: store.clone(),
        read: None,
        delete: Some(Model::Account),
    });
    assert!(
        ComplianceService::new(faulty)
            .erase("compliance-fault-owner")
            .await
            .is_err()
    );
    let normal = ComplianceService::new(store.clone());
    let retained = normal.export("compliance-fault-owner").await.unwrap();
    assert_eq!(retained.entities.len(), 2);
    assert!(retained.failures.is_empty());
    let faulty = Arc::new(fault_store::FaultStore {
        store: store.clone(),
        read: Some(Model::Jwks),
        delete: None,
    });
    let partial = ComplianceService::new(faulty)
        .erase("compliance-fault-owner")
        .await
        .unwrap();
    assert_eq!(partial.records_deleted, 1);
    assert_eq!(partial.entities_affected, ["Account"]);
    assert_eq!(partial.failures.len(), 1);
    assert_eq!(
        normal
            .export("compliance-fault-owner")
            .await
            .unwrap()
            .entities
            .len(),
        1
    );
    assert_eq!(
        normal
            .erase("compliance-fault-owner")
            .await
            .unwrap()
            .records_deleted,
        1
    );
}
