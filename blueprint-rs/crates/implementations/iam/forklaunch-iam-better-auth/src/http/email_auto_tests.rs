use super::*;
use crate::accounts::{EmailVerificationObserver, EmailVerificationObserverContext};
use crate::sessions::new_session;
use crate::store::{AuthCursor, AuthPage, AuthWindow, memory::MemoryAuthStore};
use tower::ServiceExt;
#[path = "email_auto_fault_store.rs"]
mod fault_store;
struct After<S> {
    store: Arc<S>,
    mode: &'static str,
    entered: Arc<tokio::sync::Notify>,
}
impl<S: AuthStore + 'static> EmailVerificationObserver for After<S> {
    fn prepare(
        &self,
        context: EmailVerificationObserverContext,
    ) -> BlueprintResult<crate::background::DeliveryFuture> {
        let user: Value = serde_json::from_str(context.user_json()).unwrap();
        let store = self.store.clone();
        let mode = self.mode;
        let entered = self.entered.clone();
        Ok(Box::pin(async move {
            if mode == "after-fail" {
                return Err(internal());
            }
            if mode == "pending" {
                entered.notify_one();
                std::future::pending::<()>().await;
            }
            if mode.starts_with("stale") {
                let mut tx = store.begin(true).await?;
                let mut row = tx
                    .get(Model::User, user["id"].as_str().unwrap())
                    .await?
                    .unwrap();
                if mode == "stale-email" {
                    row["email"] = json!("changed-owner@example.test");
                } else {
                    row["emailVerified"] = json!(false);
                }
                tx.replace(Model::User, row).await?;
                tx.commit().await?;
            }
            Ok(())
        }))
    }
}
async fn seed<S: AuthStore>(store: &S, mode: &str, now: DateTime<Utc>) -> (String, Value, Value) {
    let id = format!("email-auto-{mode}");
    let foreign = format!("{id}-foreign");
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":id,"email":format!("{id}@example.test"),"emailVerified":mode=="already"}),
    )
    .await
    .unwrap();
    tx.insert(
        Model::User,
        json!({"id":foreign,"email":format!("{foreign}@example.test"),"emailVerified":true}),
    )
    .await
    .unwrap();
    let mut session = new_session(&id, "", "", now).unwrap();
    if mode.starts_with("expired") {
        session["expiresAt"] = json!((now - chrono::Duration::seconds(1)).to_rfc3339());
    }
    let foreign_session = new_session(&foreign, "", "", now).unwrap();
    tx.insert(Model::Session, session.clone()).await.unwrap();
    tx.insert(Model::Session, foreign_session.clone())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    (id, session, foreign_session)
}
fn configured<S: AuthStore + 'static>(
    store: Arc<S>,
    mode: &'static str,
    now: DateTime<Utc>,
    entered: Arc<tokio::sync::Notify>,
) -> AuthHttp<fault_store::FaultStore<S>> {
    let fault = Arc::new(fault_store::FaultStore {
        store: store.clone(),
        read: None,
        page: None,
        delete: None,
        insert: mode.ends_with("create-throw").then_some(Model::Session),
        begin: false,
        commit: mode == "create-commit-fail",
    });
    let app = AuthHttp::new(
        fault,
        "http://auth.fixture.test".into(),
        vec![42; 32],
        vec![],
    )
    .unwrap()
    .with_clock(move || now)
    .with_auto_sign_in_after_verification(mode != "disabled");
    if matches!(
        mode,
        "after-fail" | "stale-email" | "stale-verified" | "pending"
    ) {
        app.with_after_email_verification(Arc::new(After {
            store,
            mode,
            entered,
        }))
    } else {
        app
    }
}
fn cookie_fragment(value: &str) -> String {
    value.split(';').next().unwrap().to_owned()
}
#[allow(clippy::too_many_lines)]
pub(crate) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    for mode in [
        "disabled",
        "anonymous",
        "current",
        "foreign",
        "invalid",
        "expired",
        "already",
        "redirect",
        "change",
        "after-fail",
        "create-throw",
        "expired-create-throw",
        "create-commit-fail",
        "transient",
        "transient-current",
        "stale-email",
        "stale-verified",
    ] {
        let (id, initial, foreign) = seed(store.as_ref(), mode, now).await;
        let app = configured(store.clone(), mode, now, Arc::default());
        let config = app.cookie;
        let selected = if mode == "foreign" {
            &foreign
        } else {
            &initial
        };
        let mut cookie = if matches!(
            mode,
            "current" | "foreign" | "expired" | "expired-create-throw" | "transient-current"
        ) {
            cookie_fragment(&config.serialize(selected["token"].as_str().unwrap(), &[42; 32], true))
        } else if mode == "invalid" {
            "better-auth.session_token=invalid".into()
        } else {
            String::new()
        };
        if mode.starts_with("transient") {
            if !cookie.is_empty() {
                cookie.push_str("; ");
            }
            cookie.push_str(&cookie_fragment(&config.transient_flag(&[42; 32])));
        }
        let token = super::observer_tests::token(&format!("{id}@example.test"), mode, now);
        let uri = format!(
            "/verify-email?token={token}{}",
            if mode == "redirect" {
                "&callbackURL=%2Fdone"
            } else {
                ""
            }
        );
        let response = app
            .router()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let failed = matches!(
            mode,
            "after-fail" | "create-throw" | "expired-create-throw" | "create-commit-fail"
        );
        assert_eq!(
            response.status().as_u16(),
            if failed {
                500
            } else if mode.starts_with("stale") {
                401
            } else if mode == "redirect" {
                302
            } else {
                200
            },
            "{mode}"
        );
        let cookies: Vec<_> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect();
        let signed = cookies
            .iter()
            .rfind(|c| c.starts_with("better-auth.session_token="));
        let issued =
            !failed && !mode.starts_with("stale") && !matches!(mode, "disabled" | "already");
        assert_eq!(signed.is_some(), issued, "{mode}");
        if let Some(signed) = signed {
            let encoded = crate::cookies::cookie_value(signed, config.name()).unwrap();
            let token = crate::cookies::verify_cookie_value(&encoded, &[42; 32]).unwrap();
            assert_eq!(
                token == initial["token"].as_str().unwrap(),
                matches!(mode, "current" | "transient-current"),
                "{mode}"
            );
            assert_eq!(signed.contains("Max-Age="), !mode.starts_with("transient"));
        }
        if failed {
            assert!(cookies.is_empty());
            assert!(
                axum::body::to_bytes(response.into_body(), 1024)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        if mode == "expired" {
            assert_eq!(cookies.len(), 4);
            assert!(cookies[..3].iter().all(|c| c.contains("Max-Age=0")));
        }
        let mut tx = store.begin(false).await.unwrap();
        let user = tx.get(Model::User, &id).await.unwrap().unwrap();
        assert_eq!(user["emailVerified"], mode != "stale-verified");
        let sessions = tx
            .find_page(Model::Session, "userId", &id, None, 256)
            .await
            .unwrap()
            .records;
        let expected = if mode == "expired-create-throw" {
            0
        } else if matches!(
            mode,
            "disabled"
                | "current"
                | "expired"
                | "already"
                | "after-fail"
                | "create-throw"
                | "create-commit-fail"
                | "transient-current"
                | "stale-email"
                | "stale-verified"
        ) {
            1
        } else {
            2
        };
        assert_eq!(sessions.len(), expected, "{mode}");
        assert!(
            tx.get(Model::Session, foreign["id"].as_str().unwrap())
                .await
                .unwrap()
                .is_some()
        );
        if mode == "transient" {
            let created = sessions.iter().find(|s| s["id"] != initial["id"]).unwrap();
            assert_eq!(
                created["expiresAt"],
                (now + chrono::Duration::days(1)).to_rfc3339_opts(SecondsFormat::Millis, true)
            );
        }
        tx.commit().await.unwrap();
    }
}
#[tokio::test]
async fn verification_auto_signin_sessions_cookies_and_failure_matrix() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
#[tokio::test]
async fn verification_auto_signin_cancellation_after_hook_cannot_create_session() {
    let store = Arc::new(MemoryAuthStore::default());
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let (id, _, _) = seed(store.as_ref(), "pending", now).await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let router = configured(store.clone(), "pending", now, entered.clone()).router();
    let token = super::observer_tests::token(&format!("{id}@example.test"), "ordinary", now);
    let task = tokio::spawn(async move {
        router
            .oneshot(
                Request::builder()
                    .uri(format!("/verify-email?token={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
    });
    entered.notified().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::User, &id).await.unwrap().unwrap()["emailVerified"],
        true
    );
    assert_eq!(
        tx.find_page(Model::Session, "userId", &id, None, 256)
            .await
            .unwrap()
            .records
            .len(),
        1
    );
    tx.commit().await.unwrap();
}
