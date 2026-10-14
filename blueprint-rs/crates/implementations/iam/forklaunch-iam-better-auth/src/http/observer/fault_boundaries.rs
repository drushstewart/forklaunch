use super::*;
use chrono::{DateTime, Utc};

#[tokio::test]
async fn signup_maps_only_user_creation_and_preserves_unexpected_transaction_failures() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for (name, insert, begin, commit, status, callbacks) in [
        ("user", Some(Model::User), false, false, 422, 0),
        ("account", Some(Model::Account), false, false, 500, 1),
        ("session", Some(Model::Session), false, false, 500, 1),
        ("begin", None, true, false, 500, 1),
        ("commit", None, false, true, 500, 1),
        ("validation", None, false, false, 400, 0),
        ("duplicate", None, false, false, 422, 0),
    ] {
        let store = Arc::new(MemoryAuthStore::default());
        if name == "duplicate" {
            let mut tx = store.begin(true).await.unwrap();
            tx.insert(
                Model::User,
                json!({"id":"existing","email":"fault@example.test"}),
            )
            .await
            .unwrap();
            tx.commit().await.unwrap();
        }
        let events = Arc::new(Mutex::new(Vec::new()));
        let capture = events.clone();
        let router = AuthHttp::new(
            Arc::new(fault_store::FaultStore {
                store: store.clone(),
                read: None,
                delete: None,
                insert,
                begin,
                commit,
            }),
            "http://localhost:3000".into(),
            vec![42; 32],
            vec![],
        )
        .unwrap()
        .with_clock(move || now)
        .with_rate_limiting(false)
        .with_api_error_observer_mode(AuthApiErrorObserverMode::UnexpectedOnly)
        .with_api_error_observer(move |event| capture.lock().unwrap().push(event.clone()))
        .router();
        let response=router.oneshot(Request::builder().method("POST").uri("/sign-up/email").header("origin","http://localhost:3000").header("content-type","application/json")
            .body(Body::from(json!({"email":"fault@example.test","password":if name=="validation"{"short"}else{"owned-fixture-password"},"name":"Fault","firstName":"Fault","lastName":"User"}).to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), status, "{name}");
        assert!(response.headers().get("set-cookie").is_none(), "{name}");
        if status == 500 {
            assert!(response.headers().get("content-type").is_none(), "{name}");
        }
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        if status == 500 {
            assert!(bytes.is_empty(), "{name}");
        } else {
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                body["code"],
                match name {
                    "user" => "FAILED_TO_CREATE_USER",
                    "duplicate" => "USER_ALREADY_EXISTS_USE_ANOTHER_EMAIL",
                    _ => "PASSWORD_TOO_SHORT",
                }
            );
            if name == "user" {
                assert_eq!(body["message"], "Failed to create user");
            }
        }
        assert_eq!(events.lock().unwrap().len(), callbacks, "{name}");
        let mut tx = store.begin(false).await.unwrap();
        for model in [Model::User, Model::Account, Model::Session] {
            let expected = usize::from(name == "duplicate" && model == Model::User);
            assert_eq!(
                tx.list(model).await.unwrap().len(),
                expected,
                "{name}: graph rollback"
            );
        }
        tx.commit().await.unwrap();
    }
}

#[tokio::test]
async fn signout_failures_clear_exact_cookies_without_committing_partial_work() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for secure in [false, true] {
        for failure in ["read", "delete", "begin", "commit", "anonymous"] {
            let store = Arc::new(MemoryAuthStore::default());
            let mut tx = store.begin(true).await.unwrap();
            tx.insert(
                Model::User,
                json!({"id":"owner","email":"owner@example.test"}),
            )
            .await
            .unwrap();
            tx.commit().await.unwrap();
            let session = crate::sessions::SessionService::new(store.clone())
                .create("owner", "", "", now)
                .await
                .unwrap();
            let cookie = crate::cookies::SessionCookie { secure };
            let encoded = cookie.serialize(session["token"].as_str().unwrap(), &[42; 32], true);
            let events = Arc::new(Mutex::new(Vec::new()));
            let capture = events.clone();
            let base = if secure {
                "https://auth.example.test"
            } else {
                "http://localhost:3000"
            };
            let router = AuthHttp::new(
                Arc::new(fault_store::FaultStore {
                    store: store.clone(),
                    read: (failure == "read").then_some(Model::Session),
                    delete: (failure == "delete").then_some(Model::Session),
                    insert: None,
                    begin: matches!(failure, "begin" | "anonymous"),
                    commit: failure == "commit",
                }),
                base.into(),
                vec![42; 32],
                vec![],
            )
            .unwrap()
            .with_clock(move || now)
            .with_rate_limiting(false)
            .with_api_error_observer_mode(AuthApiErrorObserverMode::UnexpectedOnly)
            .with_api_error_observer(move |event| capture.lock().unwrap().push(event.clone()))
            .router();
            let mut request = Request::builder()
                .method("POST")
                .uri("/sign-out")
                .header("origin", base)
                .header("content-type", "application/json");
            if failure != "anonymous" {
                request = request.header("cookie", encoded.split(';').next().unwrap());
            }
            let response = router
                .oneshot(request.body(Body::from("{}")).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), 200, "{failure}");
            let cleared: Vec<_> = response
                .headers()
                .get_all("set-cookie")
                .iter()
                .map(|value| value.to_str().unwrap().to_owned())
                .collect();
            let prefix = if secure { "__Secure-" } else { "" };
            let secure_attribute = if secure { "; Secure" } else { "" };
            let expected: Vec<_> = ["session_token", "session_data", "dont_remember"].iter().map(|name| format!("{prefix}better-auth.{name}=; Max-Age=0; Path=/; HttpOnly{secure_attribute}; SameSite=Lax")).collect();
            assert_eq!(cleared, expected, "{failure}");
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(
                    &to_bytes(response.into_body(), 4096).await.unwrap()
                )
                .unwrap(),
                json!({"success":true})
            );
            assert!(events.lock().unwrap().is_empty(), "{failure}");
            let mut tx = store.begin(false).await.unwrap();
            assert!(
                tx.get(Model::Session, session["id"].as_str().unwrap())
                    .await
                    .unwrap()
                    .is_some(),
                "{failure}: failed transaction must roll back"
            );
            tx.commit().await.unwrap();
        }
    }
}
