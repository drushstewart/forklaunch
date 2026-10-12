use super::*;
use crate::oauth::google_client::GoogleClient;
use std::sync::atomic::{AtomicBool, AtomicUsize};

pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"native-token-user","email":"native-token@fixture.test"}),
    )
    .await
    .unwrap();
    let original = json!({"id":"native-token-account","userId":"native-token-user","providerId":"google","accountId":"native-token-subject",
        "accessToken":"native-old-access","refreshToken":"native-old-refresh","idToken":"native-old-id","scope":"email,profile",
        "accessTokenExpiresAt":now.to_rfc3339_opts(chrono::SecondsFormat::Millis,true)});
    tx.insert(Model::Account, original.clone()).await.unwrap();
    tx.commit().await.unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let revoke = Arc::new(AtomicBool::new(false));
    let (address, shutdown, server) =
        start_provider(store.clone(), calls.clone(), revoke.clone()).await;
    let google = GoogleClient::new("fixture-google-id".into(), "fixture-google-secret".into())
        .unwrap()
        .with_token_endpoint(&format!("http://{address}/token"))
        .unwrap();
    let app = AuthHttp::new(
        store.clone(),
        "http://localhost:3000".into(),
        vec![9; 32],
        vec![],
    )
    .unwrap()
    .with_google(google);
    let selection = json!({"accountId":"native-token-account"});
    let result = super::super::account_tokens::handle(
        &app,
        "/get-access-token",
        &selection,
        "native-token-user",
        now,
    )
    .await
    .unwrap();
    assert_eq!(result["accessToken"], "native-updated-access");
    assert_eq!(result["scopes"], json!(["email", "profile"]));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let mut tx = store.begin(false).await.unwrap();
    let account = tx
        .get(Model::Account, "native-token-account")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account["refreshToken"], "native-rotated-refresh");
    assert_eq!(account["idToken"], "native-updated-id");
    assert_eq!(account["scope"], original["scope"]);
    assert_eq!(account["accessTokenExpiresAt"], "2027-01-15T08:10:00.000Z");
    tx.commit().await.unwrap();
    assert_eq!(
        super::super::account_tokens::handle(
            &app,
            "/refresh-token",
            &selection,
            "another-user",
            now
        )
        .await
        .err()
        .unwrap()
        .code,
        "ACCOUNT_NOT_FOUND"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let mut tx = store.begin(true).await.unwrap();
    let mut revoked = original;
    revoked["id"] = json!("native-token-revoked-account");
    tx.insert(Model::Account, revoked).await.unwrap();
    tx.commit().await.unwrap();
    revoke.store(true, Ordering::SeqCst);
    assert_eq!(
        super::super::account_tokens::handle(
            &app,
            "/refresh-token",
            &json!({"accountId":"native-token-revoked-account"}),
            "native-token-user",
            now
        )
        .await
        .err()
        .unwrap()
        .code,
        "FAILED_TO_REFRESH_ACCESS_TOKEN"
    );
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.get(Model::Account, "native-token-revoked-account")
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
    shutdown.send(()).unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn live_google_token_refresh_preserves_scope_and_account_ownership() {
    exercise(Arc::new(MemoryAuthStore::default())).await;
}

async fn start_provider<S: AuthStore + 'static>(
    store: Arc<S>,
    calls: Arc<AtomicUsize>,
    revoke: Arc<AtomicBool>,
) -> (
    std::net::SocketAddr,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let provider=axum::Router::new().route("/token",axum::routing::post(move |body: String| {
        let calls=calls.clone(); let revoke=revoke.clone(); let store=store.clone();
        async move {
            calls.fetch_add(1,Ordering::SeqCst);
            if revoke.load(Ordering::SeqCst) {
                let mut tx=store.begin(true).await.unwrap();
                tx.delete(Model::Account,"native-token-revoked-account").await.unwrap();
                tx.commit().await.unwrap();
            }
            let form=super::super::request::parse_query(&body);
            assert_eq!(form["grant_type"],"refresh_token");
            assert_eq!(form["client_id"],"fixture-google-id");
            assert_eq!(form["client_secret"],"fixture-google-secret");
            Json(json!({"access_token":"native-updated-access","refresh_token":"native-rotated-refresh",
                "id_token":"native-updated-id","expires_in":600,"scope":"narrow"}))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown, stop) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, provider)
            .with_graceful_shutdown(async {
                stop.await.unwrap();
            })
            .await
            .unwrap();
    });
    (address, shutdown, server)
}
