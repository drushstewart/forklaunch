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
    exercise_link(&service, &store, &profile, &link, &local, &tokens, now).await;
    exercise_direct_link(&service, &profile, &local, &tokens, now).await;
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

async fn exercise_link<S: AuthStore>(
    service: &GoogleAccounts<S>,
    store: &Arc<S>,
    profile: &Value,
    link: &Value,
    local: &Value,
    tokens: &Value,
    now: DateTime<Utc>,
) {
    let owner = json!({"userId":local["id"],"email":local["email"]});
    let mut explicit = link.clone();
    explicit["sub"] = json!("native-explicit-subject");
    service
        .link_verified(
            &explicit,
            &json!({"accessToken":"first-explicit-access","scope":"email,profile"}),
            &owner,
            now,
        )
        .await
        .unwrap();
    service
        .link_verified(
            &explicit,
            &json!({"refreshToken":"private-google-refresh-token","scope":" calendar ,email"}),
            &owner,
            now,
        )
        .await
        .unwrap();
    let mut tx = store.begin(false).await.unwrap();
    let account = tx
        .list(Model::Account)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row["accountId"] == explicit["sub"])
        .unwrap();
    assert_eq!(account["scope"], "email,profile,calendar");
    assert_eq!(account["accessToken"], "first-explicit-access");
    assert_eq!(account["refreshToken"], "private-google-refresh-token");
    tx.commit().await.unwrap();
    explicit["sub"] = profile["sub"].clone();
    assert_eq!(
        service
            .link_verified(&explicit, tokens, &owner, now)
            .await
            .err()
            .unwrap()
            .code,
        "account_already_linked_to_different_user"
    );
    explicit["email_verified"] = json!(false);
    assert_eq!(
        service
            .link_verified(&explicit, tokens, &owner, now)
            .await
            .err()
            .unwrap()
            .code,
        "unable_to_link_account"
    );
    explicit["email_verified"] = json!(true);
    explicit["email"] = json!("mismatch@fixture.test");
    assert_eq!(
        service
            .link_verified(&explicit, tokens, &owner, now)
            .await
            .err()
            .unwrap()
            .code,
        "email_does_not_match"
    );
}

async fn exercise_direct_link<S: AuthStore>(
    service: &GoogleAccounts<S>,
    profile: &Value,
    local: &Value,
    tokens: &Value,
    now: DateTime<Utc>,
) {
    let user_id = local["id"].as_str().unwrap();
    let mut direct = profile.clone();
    direct["sub"] = json!("native-explicit-subject");
    direct["email"] = json!("changed-own@fixture.test");
    direct["email_verified"] = json!(false);
    service
        .link_direct_verified(&direct, tokens, user_id, now)
        .await
        .unwrap();
    direct["sub"] = json!("native-direct-new");
    direct["email"] = local["email"].clone();
    assert_eq!(
        service
            .link_direct_verified(&direct, tokens, user_id, now)
            .await
            .err()
            .unwrap()
            .code,
        "LINKING_NOT_ALLOWED"
    );
    direct["email_verified"] = json!(true);
    service
        .link_direct_verified(&direct, tokens, user_id, now)
        .await
        .unwrap();
    direct["sub"] = json!("native-direct-mismatch");
    direct["email"] = json!("mismatch@fixture.test");
    assert_eq!(
        service
            .link_direct_verified(&direct, tokens, user_id, now)
            .await
            .err()
            .unwrap()
            .code,
        "LINKING_DIFFERENT_EMAILS_NOT_ALLOWED"
    );
    direct["sub"] = profile["sub"].clone();
    assert_eq!(
        service
            .link_direct_verified(&direct, tokens, user_id, now)
            .await
            .err()
            .unwrap()
            .code,
        "SOCIAL_ACCOUNT_ALREADY_LINKED"
    );
}
