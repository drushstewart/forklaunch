use super::*;
use crate::store::{AuthCursor, AuthPage, AuthWindow, memory::MemoryAuthStore};
use std::sync::Arc;
#[path = "reset_fault_store.rs"]
mod fault_store;

pub(crate) async fn lifecycle<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for (label, enabled, failed) in [
        ("default", false, false),
        ("revoke", true, false),
        ("failure", true, true),
    ] {
        let user = format!("reset-revocation-{label}");
        seed(store.as_ref(), &user, now, 3).await;
        let fault = Arc::new(fault_store::FaultStore {
            store: store.clone(),
            read: None,
            page: None,
            delete: failed.then_some(Model::Session),
            insert: None,
            begin: false,
            commit: false,
        });
        let service = CredentialService::new(fault).with_revoke_sessions_on_password_reset(enabled);
        let result = service
            .reset_password(&user, "new-owned-password".into(), now)
            .await;
        assert_eq!(result.is_err(), failed);
        let mut tx = store.begin(false).await.unwrap();
        assert!(tx.get(Model::Verification, &user).await.unwrap().is_none());
        let account = super::super::find_credential(&mut tx, &user)
            .await
            .unwrap()
            .unwrap();
        assert!(credentials::verify_password(
            "new-owned-password",
            account["password"].as_str().unwrap()
        ));
        let page = tx
            .find_page(Model::Session, "userId", &user, None, 256)
            .await
            .unwrap();
        assert_eq!(page.records.len(), if enabled && !failed { 0 } else { 3 });
        assert!(
            tx.get(Model::Session, &format!("{user}-foreign-session"))
                .await
                .unwrap()
                .is_some()
        );
        tx.commit().await.unwrap();
        assert_eq!(
            service
                .reset_password(&user, "retry-owned-password".into(), now)
                .await
                .unwrap_err()
                .code,
            "INVALID_TOKEN"
        );
    }
}
async fn seed<S: AuthStore>(store: &S, user: &str, now: DateTime<Utc>, count: usize) {
    let mut tx = store.begin(true).await.unwrap();
    let stamp = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    tx.insert(
        Model::User,
        json!({"id":user,"email":format!("{user}@fixture.test")}),
    )
    .await
    .unwrap();
    let foreign = format!("{user}-foreign");
    tx.insert(
        Model::User,
        json!({"id":foreign,"email":format!("{foreign}@fixture.test")}),
    )
    .await
    .unwrap();
    tx.insert(Model::Verification,json!({"id":user,"identifier":format!("reset-password:{user}"),"value":user,"createdAt":stamp,"expiresAt":(now+chrono::Duration::minutes(1)).to_rfc3339()})).await.unwrap();
    for index in 0..count {
        tx.insert(Model::Session,json!({"id":format!("{user}-session-{index:04}"),"userId":user,"token":format!("{user}-token-{index}"),"createdAt":stamp,"updatedAt":stamp,"expiresAt":(now-chrono::Duration::seconds(1)).to_rfc3339()})).await.unwrap();
    }
    tx.insert(Model::Session,json!({"id":format!("{user}-foreign-session"),"userId":foreign,"token":format!("{user}-foreign-token"),"createdAt":stamp,"updatedAt":stamp,"expiresAt":stamp})).await.unwrap();
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn reset_revocation_default_enabled_and_post_commit_failure() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
#[tokio::test]
async fn reset_revocation_indexed_pagination_removes_all_pages() {
    let store = Arc::new(MemoryAuthStore::default());
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    seed(store.as_ref(), "reset-many", now, 513).await;
    CredentialService::new(store.clone())
        .with_revoke_sessions_on_password_reset(true)
        .reset_password("reset-many", "new-owned-password".into(), now)
        .await
        .unwrap();
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.find_page(Model::Session, "userId", "reset-many", None, 256)
            .await
            .unwrap()
            .records
            .is_empty()
    );
    assert!(
        tx.get(Model::Session, "reset-many-foreign-session")
            .await
            .unwrap()
            .is_some()
    );
    tx.commit().await.unwrap();
}
