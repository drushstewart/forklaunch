use super::*;
use crate::{
    accounts::SigninInput,
    store::{AuthTransaction, Model, memory::MemoryAuthStore},
};
fn input(name: &str) -> SignupInput {
    serde_json::from_value(json!({"name":name,"firstName":"Submitted","lastName":"User","email":"auto-signin@example.test","password":"owned-auto-password"})).unwrap()
}
#[tokio::test]
async fn signup_auto_sign_in_legacy_guard_duplicate_privacy_and_later_login() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
pub(crate) async fn lifecycle<S: AuthStore>(store: Arc<S>) {
    let service = CredentialService::new(store.clone()).with_signup_auto_sign_in(false);
    let now = Utc::now();
    assert_eq!(
        service
            .sign_up(input("Original"), "", "", now)
            .await
            .err()
            .unwrap()
            .code,
        "SIGNUP_SESSION_DISABLED"
    );
    let outcome = service
        .sign_up_outcome(input("Original"), "", "", now)
        .await
        .unwrap();
    assert!(matches!(&outcome, SignupOutcome::Unauthenticated { .. }));
    let first = outcome.body();
    let duplicate = service
        .sign_up_outcome(input("Different submitted name"), "", "", now)
        .await
        .unwrap()
        .body();
    assert_eq!(duplicate["user"]["name"], "Different submitted name");
    assert_ne!(duplicate["user"]["id"], first["user"]["id"]);
    assert_eq!(duplicate["user"]["image"], Value::Null);
    assert_eq!(duplicate["user"].get("phoneNumber"), Some(&Value::Null));
    assert_eq!(first["token"], Value::Null);
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.find_page(
            Model::Session,
            "userId",
            first["user"]["id"].as_str().unwrap(),
            None,
            10
        )
        .await
        .unwrap()
        .records
        .is_empty()
    );
    tx.commit().await.unwrap();
    let signin = service
        .sign_in(
            SigninInput {
                email: "auto-signin@example.test".into(),
                password: "owned-auto-password".into(),
                callback_url: None,
                remember_me: true,
            },
            "",
            "",
            now,
        )
        .await
        .unwrap();
    assert_eq!(signin.user["id"], first["user"]["id"]);
    assert!(signin.session["token"].as_str().is_some());
}

#[tokio::test]
async fn signup_auto_sign_in_concurrent_duplicates_commit_one_account_without_sessions() {
    let store = Arc::new(MemoryAuthStore::default());
    let service = CredentialService::new(store.clone()).with_signup_auto_sign_in(false);
    let now = Utc::now();
    let (left, right) = tokio::join!(
        service.sign_up_outcome(input("Left"), "", "", now),
        service.sign_up_outcome(input("Right"), "", "", now)
    );
    let left = left.unwrap().body();
    let right = right.unwrap().body();
    assert!(left["token"].is_null() && right["token"].is_null());
    assert_ne!(left["user"]["id"], right["user"]["id"]);
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.list_page(Model::User, None, 10)
            .await
            .unwrap()
            .records
            .len(),
        1
    );
    assert_eq!(
        tx.list_page(Model::Account, None, 10)
            .await
            .unwrap()
            .records
            .len(),
        1
    );
    assert!(
        tx.list_page(Model::Session, None, 10)
            .await
            .unwrap()
            .records
            .is_empty()
    );
    tx.commit().await.unwrap();
}
