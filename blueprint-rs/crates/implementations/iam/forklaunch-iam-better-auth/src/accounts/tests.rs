use super::*;
use crate::store::memory::{MemoryAuthStore, MemoryAuthTransaction};
use tokio::sync::Notify;

fn signup(email: &str) -> SignupInput {
    serde_json::from_value(json!({"name":"Person","firstName":"First","lastName":"Last","email":email,"password":"test-only-password"})).unwrap()
}
fn signin(email: &str) -> SigninInput {
    serde_json::from_value(json!({"email":email,"password":"test-only-password"})).unwrap()
}
pub(crate) async fn exercise_credentials<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let service = CredentialService::new(store.clone());
    let grant = service
        .sign_up(
            signup("Credentials@Fixture.test"),
            "private-ip",
            "private-agent",
            now,
        )
        .await
        .unwrap();
    assert_eq!(grant.user["email"], "credentials@fixture.test");
    assert!(grant.user.get("password").is_none());
    assert!(grant.persistent);
    assert_eq!(
        service
            .sign_up(signup("credentials@fixture.test"), "", "", now)
            .await
            .err()
            .unwrap()
            .code,
        "USER_ALREADY_EXISTS_USE_ANOTHER_EMAIL"
    );
    let login = service
        .sign_in(signin("CREDENTIALS@FIXTURE.TEST"), "", "", now)
        .await
        .unwrap();
    assert_eq!(grant.user, login.user);
    assert_ne!(grant.session["token"], login.session["token"]);
    let sessions = crate::sessions::SessionService::new(store.clone());
    assert_eq!(
        sessions
            .get(login.session["token"].as_str().unwrap(), false, false, now)
            .await
            .unwrap()
            .unwrap()
            .user,
        grant.user
    );
    let (left, right) = tokio::join!(
        service.sign_up(signup("race-credentials@fixture.test"), "", "", now),
        service.sign_up(signup("race-credentials@fixture.test"), "", "", now)
    );
    let winner = match (left, right) {
        (Ok(winner), Err(error)) | (Err(error), Ok(winner)) => {
            assert_eq!(error.code, "USER_ALREADY_EXISTS_USE_ANOTHER_EMAIL");
            winner
        }
        _ => panic!("one concurrent signup must win and one must conflict"),
    };
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.list(Model::User)
            .await
            .unwrap()
            .iter()
            .filter(|user| user["email"] == "race-credentials@fixture.test")
            .count(),
        1
    );
    assert_eq!(
        tx.list(Model::Account)
            .await
            .unwrap()
            .iter()
            .filter(|account| account["userId"] == winner.user["id"])
            .count(),
        1
    );
    assert_eq!(
        tx.list(Model::Session)
            .await
            .unwrap()
            .iter()
            .filter(|session| session["userId"] == winner.user["id"])
            .count(),
        1
    );
    let account = tx
        .list(Model::Account)
        .await
        .unwrap()
        .into_iter()
        .find(|account| account["userId"] == grant.user["id"])
        .unwrap();
    assert_ne!(account["password"], "test-only-password");
    assert_eq!(account["password"].as_str().unwrap().len(), 161);
    tx.commit().await.unwrap();
    exercise_password_change(store.clone(), grant, login, winner, now).await;
    super::reset::tests::exercise_reset(store.clone()).await;
    super::set::tests::exercise_set(store).await;
}

async fn exercise_password_change<S: AuthStore>(
    store: Arc<S>,
    grant: AuthGrant,
    login: AuthGrant,
    winner: AuthGrant,
    now: DateTime<Utc>,
) {
    let service = CredentialService::new(store.clone());
    let sessions = crate::sessions::SessionService::new(store);
    let original = grant.session["token"].as_str().unwrap();
    assert_eq!(
        service
            .verify_password(original, "wrong".into(), false, now)
            .await
            .unwrap_err()
            .code,
        "INVALID_PASSWORD"
    );
    let bad: ChangePasswordInput = serde_json::from_value(json!({"currentPassword":"wrong","newPassword":"replacement-password","revokeOtherSessions":true})).unwrap();
    assert_eq!(
        service
            .change_password(original, bad, false, "", "", now)
            .await
            .err()
            .unwrap()
            .code,
        "INVALID_PASSWORD"
    );
    // Failed password changes cannot revoke existing sessions or alter hashes.
    assert!(
        sessions
            .get(login.session["token"].as_str().unwrap(), false, false, now)
            .await
            .unwrap()
            .is_some()
    );
    service
        .verify_password(original, "test-only-password".into(), false, now)
        .await
        .unwrap();
    let payload: ChangePasswordInput = serde_json::from_value(json!({"currentPassword":"test-only-password","newPassword":"replacement-password","revokeOtherSessions":true})).unwrap();
    let changed = service
        .change_password(original, payload, false, "new-ip", "new-agent", now)
        .await
        .unwrap();
    let replacement = changed.session.unwrap();
    assert_eq!(replacement["ipAddress"], "new-ip");
    assert_eq!(replacement["userAgent"], "new-agent");
    assert!(
        sessions
            .get(original, false, false, now)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        sessions
            .get(login.session["token"].as_str().unwrap(), false, false, now)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        sessions
            .get(winner.session["token"].as_str().unwrap(), false, false, now)
            .await
            .unwrap()
            .is_some()
    );
    service
        .verify_password(
            replacement["token"].as_str().unwrap(),
            "replacement-password".into(),
            false,
            now,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn native_credentials_case_normalization_and_concurrent_atomic_signup() {
    exercise_credentials(Arc::new(MemoryAuthStore::default())).await;
}

struct PausedWrites {
    inner: MemoryAuthStore,
    ready: Notify,
    resume: Notify,
}
impl AuthStore for PausedWrites {
    type Transaction<'a> = MemoryAuthTransaction;
    async fn begin(&self, write: bool) -> BlueprintResult<Self::Transaction<'_>> {
        if write {
            self.ready.notify_one();
            self.resume.notified().await;
        }
        self.inner.begin(write).await
    }
}
#[tokio::test]
async fn password_changed_after_verification_cannot_issue_a_session() {
    let store = MemoryAuthStore::default();
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let existing = CredentialService::new(Arc::new(store.clone()));
    let grant = existing
        .sign_up(signup("credentials@fixture.test"), "", "", now)
        .await
        .unwrap();
    let paused = Arc::new(PausedWrites {
        inner: store.clone(),
        ready: Notify::new(),
        resume: Notify::new(),
    });
    let service = CredentialService::new(paused.clone());
    let ((), result) = tokio::time::timeout(std::time::Duration::from_secs(90), async {
        tokio::join!(
            async {
                paused.ready.notified().await;
                let mut tx = store.begin(true).await.unwrap();
                let mut account = tx.list(Model::Account).await.unwrap().remove(0);
                let hash = account["password"].as_str().unwrap();
                let last = if hash.ends_with('a') { 'b' } else { 'a' };
                account["password"] = json!(format!("{}{last}", &hash[..hash.len() - 1]));
                tx.replace(Model::Account, account).await.unwrap();
                tx.commit().await.unwrap();
                paused.resume.notify_one();
            },
            service.sign_in(signin("credentials@fixture.test"), "", "", now)
        )
    })
    .await
    .expect("credential verification race must complete");
    assert_eq!(result.err().unwrap().code, "INVALID_EMAIL_OR_PASSWORD");
    let mut tx = store.begin(false).await.unwrap();
    let sessions = tx.list(Model::Session).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["token"], grant.session["token"]);
    tx.commit().await.unwrap();
}
