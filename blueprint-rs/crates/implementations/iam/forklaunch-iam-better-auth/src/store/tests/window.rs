use super::*;
use crate::store::postgres::PostgresAuthStore;

pub(super) async fn exercise_window(
    owner: &sqlx::PgPool,
    tenant: &str,
    store: &PostgresAuthStore,
    other: &PostgresAuthStore,
) {
    let mut tx = store.begin(true).await.unwrap();
    for index in 0..5 {
        tx.insert(
            Model::Account,
            json!({"id":format!("window-{index}"),"userId":"window-owner"}),
        )
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
    let ciphertext: String = sqlx::query_scalar("SELECT ciphertext FROM fl_better_auth.records WHERE tenant_id=$1 AND model='account' AND id='window-4'").bind(tenant).fetch_one(owner).await.unwrap();
    sqlx::query("UPDATE fl_better_auth.records SET ciphertext='corrupt-owned-fixture' WHERE tenant_id=$1 AND model='account' AND id='window-4'").bind(tenant).execute(owner).await.unwrap();
    let mut tx = store.begin(false).await.unwrap();
    let first = tx
        .find_window(Model::Account, "userId", "window-owner", 0, 2)
        .await
        .unwrap();
    assert_eq!(first.total, 5);
    assert_eq!(
        first.records.len(),
        2,
        "off-page ciphertext must not be decrypted"
    );
    assert!(
        tx.find_window(Model::Account, "userId", "window-owner", 4, 1)
            .await
            .is_err(),
        "returned ciphertext must still authenticate"
    );
    tx.commit().await.unwrap();
    sqlx::query("UPDATE fl_better_auth.records SET ciphertext=$2 WHERE tenant_id=$1 AND model='account' AND id='window-4'").bind(tenant).bind(ciphertext).execute(owner).await.unwrap();
    let mut foreign = other.begin(false).await.unwrap();
    let window = foreign
        .find_window(Model::Account, "userId", "window-owner", 0, 2)
        .await
        .unwrap();
    assert_eq!(window.total, 0);
    assert!(window.records.is_empty());
    foreign.commit().await.unwrap();

    let mut read = store.begin(false).await.unwrap();
    assert_eq!(
        read.find_window(Model::Account, "userId", "window-owner", 0, 1)
            .await
            .unwrap()
            .total,
        5
    );
    let writer_store = store.clone();
    let (started, waiting) = tokio::sync::oneshot::channel();
    let mut writer = tokio::spawn(async move {
        started.send(()).unwrap();
        let mut tx = writer_store.begin(true).await.unwrap();
        tx.insert(
            Model::Account,
            json!({"id":"window-5","userId":"window-owner"}),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    });
    waiting.await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(40), &mut writer)
            .await
            .is_err(),
        "read transaction must hold graph stable"
    );
    assert_eq!(
        read.find_window(Model::Account, "userId", "window-owner", 4, 2)
            .await
            .unwrap()
            .records
            .len(),
        1
    );
    read.commit().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), writer)
        .await
        .expect("writer must resume after read commit")
        .unwrap();
    let mut tx = store.begin(true).await.unwrap();
    assert_eq!(
        tx.find_window(Model::Account, "userId", "window-owner", 0, 1)
            .await
            .unwrap()
            .total,
        6
    );
    for index in 0..6 {
        tx.delete(Model::Account, &format!("window-{index}"))
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
}
