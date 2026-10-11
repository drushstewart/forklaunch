use super::{
    DirectoryKind, DirectoryStore, DirectoryTransaction, memory::MemoryDirectoryStore,
    service::BaseIamDirectory,
};
use forklaunch_iam_interface::services::{CreateUserDto, UpdateUserDto, UserService};
use serde_json::json;
use std::sync::Arc;

fn user(email: &str) -> CreateUserDto {
    serde_json::from_value(json!({"email":email,"password":"never-stored","firstName":"Private","lastName":"Member","roles":[]})).unwrap()
}
#[tokio::test]
async fn failed_batches_roll_back_writes_and_unique_claims() {
    let store = Arc::new(MemoryDirectoryStore::default());
    let service = BaseIamDirectory::new(store.clone());
    assert!(
        service
            .create_batch_users(vec![user("same@fixture.test"), user("same@fixture.test")])
            .await
            .is_err()
    );
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.list(DirectoryKind::User, None, None)
            .await
            .unwrap()
            .is_empty()
    );
    tx.commit().await.unwrap();
    let first = service
        .create_user(user("same@fixture.test"))
        .await
        .unwrap();
    let update: UpdateUserDto =
        serde_json::from_value(json!({"id":first.id,"firstName":"Must roll back"})).unwrap();
    let missing: UpdateUserDto =
        serde_json::from_value(json!({"id":"missing","lastName":"Missing"})).unwrap();
    assert!(
        service
            .update_batch_users(vec![update, missing])
            .await
            .is_err()
    );
    assert_eq!(
        service.get_user(&first.id).await.unwrap().first_name,
        "Private"
    );
}
#[tokio::test]
async fn dropped_transactions_and_concurrent_unique_inserts_are_atomic() {
    let store = Arc::new(MemoryDirectoryStore::default());
    {
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(
            DirectoryKind::Permission,
            json!({"id":"dropped","slug":"read"}),
        )
        .await
        .unwrap();
    }
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.get(DirectoryKind::Permission, "dropped")
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
    let first = BaseIamDirectory::new(store.clone());
    let second = BaseIamDirectory::new(store);
    let (a, b) = tokio::join!(
        first.create_user(user("race@fixture.test")),
        second.create_user(user("race@fixture.test"))
    );
    assert_ne!(a.is_ok(), b.is_ok());
}
