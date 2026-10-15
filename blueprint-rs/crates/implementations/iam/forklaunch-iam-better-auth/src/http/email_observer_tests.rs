use super::*;
use crate::accounts::{EmailVerificationObserver, EmailVerificationObserverContext};
use crate::store::memory::MemoryAuthStore;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt;
struct Hook<S> {
    store: Arc<S>,
    phase: &'static str,
    mode: &'static str,
    calls: Arc<AtomicUsize>,
    entered: Arc<tokio::sync::Notify>,
}
impl<S: AuthStore + 'static> EmailVerificationObserver for Hook<S> {
    fn prepare(
        &self,
        context: EmailVerificationObserverContext,
    ) -> BlueprintResult<crate::background::DeliveryFuture> {
        let user: Value = serde_json::from_str(context.user_json()).unwrap();
        assert!(user.get("privateSecret").is_none());
        assert_eq!(user["emailVerified"], self.phase == "after");
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.mode == "before-sync" && self.phase == "before" {
            return Err(internal());
        }
        let (store, phase, mode, entered) = (
            self.store.clone(),
            self.phase,
            self.mode,
            self.entered.clone(),
        );
        Ok(Box::pin(async move {
            let mut tx = store.begin(true).await?;
            let mut current = tx
                .get(Model::User, user["id"].as_str().unwrap())
                .await?
                .unwrap();
            assert_eq!(current["emailVerified"], phase == "after");
            if mode == "stale" {
                current["email"] = json!("callback-changed@example.test");
                tx.replace(Model::User, current).await?;
            }
            tx.commit().await?;
            if (phase == "before" && matches!(mode, "before-async" | "redirect-before-fail"))
                || (phase == "after" && matches!(mode, "after-async" | "change-after-fail"))
            {
                return Err(internal());
            }
            if mode == format!("{phase}-pending") {
                entered.notify_one();
                std::future::pending::<()>().await;
            }
            Ok(())
        }))
    }
}
pub(super) fn token(email: &str, mode: &str, now: DateTime<Utc>) -> String {
    let mut claims = json!({"email":email,"exp":now.timestamp()+60});
    if matches!(
        mode,
        "change" | "change-after-fail" | "legacy" | "confirmation"
    ) {
        claims["updateTo"] = json!(format!("changed-{email}"));
        if mode != "legacy" {
            claims["requestType"] = json!(if mode == "confirmation" {
                "change-email-confirmation"
            } else {
                "change-email-verification"
            });
        }
    }
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256"}"#),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(&[42; 32]).unwrap();
    mac.update(input.as_bytes());
    format!(
        "{input}.{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
}
async fn seed<S: AuthStore>(store: &S, mode: &str) -> String {
    let id = format!("email-observer-{mode}");
    let mut user = json!({"id":id,"email":format!("{id}@example.test"),"name":"Owned","emailVerified":mode=="already","privateSecret":"not-public"});
    if mode.contains("malformed") {
        user["name"] = json!({"nested":"not-scalar"});
    }
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User, user).await.unwrap();
    tx.commit().await.unwrap();
    id
}
fn app<S: AuthStore + 'static>(
    store: Arc<S>,
    mode: &'static str,
    before: Arc<AtomicUsize>,
    after: Arc<AtomicUsize>,
    entered: Arc<tokio::sync::Notify>,
    now: DateTime<Utc>,
) -> AuthHttp<S> {
    let mut app = AuthHttp::new(
        store.clone(),
        "http://auth.fixture.test".into(),
        vec![42; 32],
        vec![],
    )
    .unwrap()
    .with_clock(move || now);
    if mode != "no-hook-malformed" {
        if mode != "after-malformed" {
            app = app.with_before_email_verification(Arc::new(Hook {
                store: store.clone(),
                mode,
                phase: "before",
                calls: before,
                entered: entered.clone(),
            }));
        }
        app = app.with_after_email_verification(Arc::new(Hook {
            store,
            mode,
            phase: "after",
            calls: after,
            entered,
        }));
    }
    app
}
#[allow(clippy::too_many_lines)]
pub(crate) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    for mode in [
        "ordinary",
        "already",
        "before-sync",
        "before-async",
        "after-async",
        "change",
        "change-after-fail",
        "legacy",
        "confirmation",
        "redirect-before-fail",
        "stale",
        "before-malformed",
        "after-malformed",
        "no-hook-malformed",
    ] {
        let id = seed(store.as_ref(), mode).await;
        let email = format!("{id}@example.test");
        let before = Arc::new(AtomicUsize::new(0));
        let after = Arc::new(AtomicUsize::new(0));
        let router = app(
            store.clone(),
            mode,
            before.clone(),
            after.clone(),
            Arc::default(),
            now,
        )
        .router();
        let uri = format!(
            "/verify-email?token={}{}",
            token(&email, mode, now),
            if mode == "redirect-before-fail" {
                "&callbackURL=%2Fdone"
            } else {
                ""
            }
        );
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            router
                .clone()
                .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap()),
        )
        .await
        .unwrap()
        .unwrap();
        let failed = matches!(
            mode,
            "before-sync"
                | "before-async"
                | "after-async"
                | "change-after-fail"
                | "redirect-before-fail"
                | "before-malformed"
                | "after-malformed"
        );
        assert_eq!(
            response.status().as_u16(),
            if failed {
                500
            } else if mode == "stale" {
                401
            } else {
                200
            },
            "{mode}"
        );
        if failed {
            assert!(!response.headers().contains_key(header::SET_COOKIE));
            assert!(!response.headers().contains_key(header::LOCATION));
            assert!(
                axum::body::to_bytes(response.into_body(), 1024)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        let mut tx = store.begin(false).await.unwrap();
        let user = tx.get(Model::User, &id).await.unwrap().unwrap();
        assert_eq!(
            user["emailVerified"],
            !matches!(
                mode,
                "before-sync"
                    | "before-async"
                    | "redirect-before-fail"
                    | "legacy"
                    | "confirmation"
                    | "stale"
                    | "before-malformed"
            ),
            "{mode}"
        );
        let sessions = tx
            .find_page(Model::Session, "userId", &id, None, 256)
            .await
            .unwrap()
            .records
            .len();
        assert_eq!(
            sessions,
            usize::from(matches!(mode, "change" | "change-after-fail" | "legacy"))
        );
        tx.commit().await.unwrap();
        assert_eq!(
            before.load(Ordering::SeqCst),
            usize::from(matches!(
                mode,
                "ordinary"
                    | "before-sync"
                    | "before-async"
                    | "after-async"
                    | "redirect-before-fail"
                    | "stale"
            )),
            "{mode}"
        );
        assert_eq!(
            after.load(Ordering::SeqCst),
            usize::from(matches!(
                mode,
                "ordinary" | "after-async" | "change" | "change-after-fail"
            )),
            "{mode}"
        );
        if mode == "ordinary" {
            let counts = (before.load(Ordering::SeqCst), after.load(Ordering::SeqCst));
            assert_eq!(counts, (1, 1));
            router
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                (before.load(Ordering::SeqCst), after.load(Ordering::SeqCst)),
                counts
            );
        }
    }
}
#[tokio::test]
async fn email_observer_branches_failures_projection_and_replay() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
#[tokio::test]
async fn email_observer_cancellation_before_and_after_commit() {
    for mode in ["before-pending", "after-pending"] {
        let store = Arc::new(MemoryAuthStore::default());
        let id = seed(store.as_ref(), mode).await;
        let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let router = app(
            store.clone(),
            mode,
            Arc::default(),
            Arc::default(),
            entered.clone(),
            now,
        )
        .router();
        let uri = format!(
            "/verify-email?token={}",
            token(&format!("{id}@example.test"), mode, now)
        );
        let task = tokio::spawn(async move {
            router
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
        });
        entered.notified().await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let mut tx = store.begin(false).await.unwrap();
        assert_eq!(
            tx.get(Model::User, &id).await.unwrap().unwrap()["emailVerified"],
            mode == "after-pending"
        );
        tx.commit().await.unwrap();
    }
}
