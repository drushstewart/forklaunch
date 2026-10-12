use super::*;
use crate::store::memory::MemoryAuthStore;

pub(crate) async fn exercise<S: AuthStore>(store: Arc<S>) {
    let service = GoogleAccounts::new(store.clone());
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let profile = json!({"sub":"native-google-subject","email":"native-google@fixture.test",
        "email_verified":true,"name":"Google Person","given_name":"Google","family_name":"Person"});
    let tokens = json!({"idToken":"private-google-id-token","accessToken":"private-google-access-token",
        "refreshToken":"private-google-refresh-token","scope":"openid,email"});
    let (one, two) = tokio::join!(
        service.sign_in_verified(&profile, &tokens, "127.0.0.1", "fixture", now),
        service.sign_in_verified(&profile, &tokens, "127.0.0.1", "fixture", now)
    );
    let one = one.unwrap();
    let two = two.unwrap();
    assert_eq!(usize::from(one.registered) + usize::from(two.registered), 1);
    assert_eq!(one.auth.user, two.auth.user);
    assert_ne!(one.auth.session["token"], two.auth.session["token"]);
    let mut changed = profile.clone();
    changed["email"] = json!("changed-google@fixture.test");
    changed["given_name"] = json!("Changed");
    let updated = service
        .sign_in_verified(
            &changed,
            &json!({"accessToken":"updated-private-access"}),
            "",
            "",
            now,
        )
        .await
        .unwrap();
    assert_eq!(updated.auth.user, one.auth.user);
    let mut tx = store.begin(false).await.unwrap();
    let account = tx
        .list(Model::Account)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row["accountId"] == profile["sub"])
        .unwrap();
    assert_eq!(account["userId"], one.auth.user["id"]);
    assert_eq!(account["idToken"], tokens["idToken"]);
    assert_eq!(account["accessToken"], "updated-private-access");
    assert_eq!(account["refreshToken"], tokens["refreshToken"]);
    tx.commit().await.unwrap();
    let mut tx = store.begin(true).await.unwrap();
    let local = json!({"id":"native-google-local","name":"Local","email":"native-local@fixture.test","emailVerified":false,"firstName":"Local","lastName":"Person"});
    tx.insert(Model::User, local.clone()).await.unwrap();
    tx.insert(Model::Account,json!({"id":"native-google-orphan","providerId":"google","accountId":"native-orphan-subject","userId":"missing-native-google-user"})).await.unwrap();
    tx.commit().await.unwrap();
    let mut link = profile.clone();
    link["sub"] = json!("native-linked-subject");
    link["email"] = local["email"].clone();
    assert_eq!(
        service
            .sign_in_verified(&link, &tokens, "", "", now)
            .await
            .err()
            .unwrap()
            .code,
        "OAUTH_LINK_ERROR"
    );
    let mut tx = store.begin(true).await.unwrap();
    assert!(
        !tx.list(Model::Account)
            .await
            .unwrap()
            .iter()
            .any(|row| row["accountId"] == link["sub"])
    );
    let mut local = local;
    local["emailVerified"] = json!(true);
    tx.replace(Model::User, local.clone()).await.unwrap();
    tx.commit().await.unwrap();
    let linked = service
        .sign_in_verified(&link, &tokens, "", "", now)
        .await
        .unwrap();
    assert!(!linked.registered);
    assert_eq!(linked.auth.user["id"], local["id"]);
    link["sub"] = json!("native-orphan-subject");
    assert_eq!(
        service
            .sign_in_verified(&link, &tokens, "", "", now)
            .await
            .err()
            .unwrap()
            .message,
        "unable to link account"
    );
}

#[tokio::test]
async fn google_identity_ownership_linking_tokens_and_concurrent_signup() {
    exercise(Arc::new(MemoryAuthStore::default())).await;
}
