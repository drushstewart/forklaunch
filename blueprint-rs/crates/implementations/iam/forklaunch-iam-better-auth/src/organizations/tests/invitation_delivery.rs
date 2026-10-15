use super::invitation_cancel::store::{Failure, Store};
use super::invitation_policy::{request, rows, seed};
use super::*;
use crate::background::{AuthBackgroundDispatcher, AuthBackgroundTask, DeliveryFuture};
use crate::organizations::{InvitationDeliveryContext, InvitationEmailDelivery, InvitationPolicy};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Success,
    Prepare,
    Async,
}
struct Hook<S> {
    store: Arc<S>,
    mode: Mode,
    calls: AtomicUsize,
    polled: Arc<AtomicUsize>,
    seen: Mutex<Vec<Value>>,
    wait: Option<Arc<tokio::sync::Notify>>,
    ready: Arc<tokio::sync::Notify>,
}
impl<S> Hook<S> {
    fn new(store: Arc<S>, mode: Mode) -> Self {
        Self {
            store,
            mode,
            calls: AtomicUsize::new(0),
            polled: Arc::new(AtomicUsize::new(0)),
            seen: Mutex::new(vec![]),
            wait: None,
            ready: Arc::new(tokio::sync::Notify::new()),
        }
    }
}
impl<S: AuthStore + 'static> InvitationEmailDelivery for Hook<S> {
    fn prepare(&self, context: InvitationDeliveryContext) -> BlueprintResult<DeliveryFuture> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.seen.lock().unwrap().push(json!({"id":context.id(),"email":context.email(),"role":context.role(),"organization":context.organization(),"invitation":context.invitation(),"inviter":context.inviter()}));
        if self.mode == Mode::Prepare {
            return Err(BlueprintError::new(
                "owned-secret-code",
                "owned-secret-message",
            ));
        }
        let store = self.store.clone();
        let id = context.id().to_owned();
        let polled = self.polled.clone();
        let fail = self.mode == Mode::Async;
        let wait = self.wait.clone();
        let ready = self.ready.clone();
        Ok(Box::pin(async move {
            polled.fetch_add(1, Ordering::SeqCst);
            let mut tx = store.begin(true).await?;
            assert!(tx.get(Model::Invitation, &id).await?.is_some());
            tx.commit().await?;
            ready.notify_one();
            if let Some(wait) = wait {
                wait.notified().await;
            }
            if fail {
                Err(BlueprintError::new(
                    "owned-secret-code",
                    "owned-secret-message",
                ))
            } else {
                Ok(())
            }
        }))
    }
}
struct Dispatcher {
    tasks: Mutex<Vec<AuthBackgroundTask>>,
    calls: AtomicUsize,
    reject: bool,
}
impl AuthBackgroundDispatcher for Dispatcher {
    fn dispatch(&self, task: AuthBackgroundTask) -> BlueprintResult<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.tasks.lock().unwrap().push(task);
        if self.reject {
            Err(BlueprintError::new(
                "owned-secret-code",
                "owned-secret-message",
            ))
        } else {
            Ok(())
        }
    }
}
async fn bounded<F: std::future::Future>(f: F) -> F::Output {
    tokio::time::timeout(std::time::Duration::from_secs(10), f)
        .await
        .expect("delivery must not hold writer lock")
}

#[allow(clippy::too_many_lines)]
pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    for (index, mode) in [Mode::Success, Mode::Prepare, Mode::Async]
        .into_iter()
        .enumerate()
    {
        for resend in [false, true] {
            let f = seed(store.as_ref(), &format!("delivery-{index}-{resend}")).await;
            if resend {
                OrganizationService::new(store.clone())
                    .create_invitation(&f.owner_token, false, &request(&f), f.now)
                    .await
                    .unwrap();
            }
            let hook = Arc::new(Hook::new(store.clone(), mode));
            let svc = OrganizationService::new(store.clone())
                .with_invitation_delivery(hook.clone())
                .with_invitation_policy(InvitationPolicy::new(3, 100, true));
            let mut body = request(&f);
            body["resend"] = json!(resend);
            let now = f.now + chrono::Duration::seconds(1);
            let result = bounded(svc.create_invitation(&f.owner_token, false, &body, now)).await;
            if mode == Mode::Prepare {
                assert_eq!(
                    result.err().unwrap().code,
                    "INVITATION_DELIVERY_PREPARATION_FAILED"
                );
            } else {
                assert!(result.is_ok());
            }
            let current = rows(store.as_ref(), &f.org).await;
            assert_eq!(current.len(), 1);
            assert_eq!(
                current[0]["expiresAt"],
                timestamp(now + chrono::Duration::seconds(3))
            );
            assert_eq!(hook.calls.load(Ordering::SeqCst), 1);
            let seen = hook.seen.lock().unwrap();
            assert_eq!(seen[0]["invitation"], current[0]);
            assert_eq!(seen[0]["inviter"]["user"]["id"], f.owner);
            assert_eq!(seen[0]["organization"]["id"], f.org);
        }
    }
    for reject in [false, true] {
        let f = seed(store.as_ref(), &format!("delivery-scheduler-{reject}")).await;
        let hook = Arc::new(Hook::new(store.clone(), Mode::Async));
        let dispatcher = Arc::new(Dispatcher {
            tasks: Mutex::new(vec![]),
            calls: AtomicUsize::new(0),
            reject,
        });
        let svc = OrganizationService::new(store.clone())
            .with_background_dispatcher(dispatcher.clone())
            .with_invitation_delivery(hook.clone());
        assert!(
            bounded(svc.create_invitation(&f.owner_token, false, &request(&f), f.now))
                .await
                .is_ok()
        );
        assert_eq!(hook.polled.load(Ordering::SeqCst), 0);
        assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 1);
        let tasks = std::mem::take(&mut *dispatcher.tasks.lock().unwrap());
        for task in tasks {
            bounded(task).await;
        }
        assert_eq!(hook.polled.load(Ordering::SeqCst), 1);
    }
    let f = seed(store.as_ref(), "delivery-await").await;
    let wait = Arc::new(tokio::sync::Notify::new());
    let mut h = Hook::new(store.clone(), Mode::Success);
    h.wait = Some(wait.clone());
    let hook = Arc::new(h);
    let svc = OrganizationService::new(store.clone()).with_invitation_delivery(hook.clone());
    let body = request(&f);
    let request = svc.create_invitation(&f.owner_token, false, &body, f.now);
    tokio::pin!(request);
    tokio::select! {() = hook.ready.notified()=>{},_ = &mut request=>panic!("awaited delivery cannot finish before release")};
    assert_eq!(rows(store.as_ref(), &f.org).await.len(), 1);
    wait.notify_one();
    assert!(bounded(request).await.is_ok());
    canceled_delivery(store.clone()).await;
    persistence_faults(store.clone()).await;
    projection_and_no_delivery(store.clone()).await;
    #[cfg(feature = "http")]
    {
        http_delivery(store.clone()).await;
        http_dispatcher(store).await;
    }
}
async fn persistence_faults<S: AuthStore + 'static>(store: Arc<S>) {
    for (index, failure) in [
        Failure::Insert,
        Failure::ReplacementCommit,
        Failure::ReplacementAcknowledgement,
    ]
    .into_iter()
    .enumerate()
    {
        let f = seed(store.as_ref(), &format!("delivery-fault-{index}")).await;
        let hook = Arc::new(Hook::new(store.clone(), Mode::Success));
        let wrapped = Arc::new(Store {
            inner: store.clone(),
            after: Mutex::new(None),
            failure,
        });
        assert!(
            OrganizationService::new(wrapped)
                .with_invitation_delivery(hook.clone())
                .create_invitation(&f.owner_token, false, &request(&f), f.now)
                .await
                .is_err()
        );
        assert_eq!(hook.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            rows(store.as_ref(), &f.org).await.len(),
            usize::from(failure == Failure::ReplacementAcknowledgement)
        );
    }
}
async fn projection_and_no_delivery<S: AuthStore + 'static>(store: Arc<S>) {
    for enabled in [false, true] {
        let f = seed(store.as_ref(), &format!("delivery-projection-{enabled}")).await;
        let mut tx = store.begin(true).await.unwrap();
        let mut user = tx.get(Model::User, &f.owner).await.unwrap().unwrap();
        user["name"] = json!({"nested":"unsupported"});
        tx.replace(Model::User, user).await.unwrap();
        tx.commit().await.unwrap();
        let hook = Arc::new(Hook::new(store.clone(), Mode::Success));
        let mut svc = OrganizationService::new(store.clone());
        if enabled {
            svc = svc.with_invitation_delivery(hook.clone());
        }
        let out = svc
            .create_invitation(&f.owner_token, false, &request(&f), f.now)
            .await;
        if enabled {
            assert_eq!(
                out.err().unwrap().code,
                "INVITATION_DELIVERY_PREPARATION_FAILED"
            );
        } else {
            assert!(out.is_ok());
        }
        assert_eq!(rows(store.as_ref(), &f.org).await.len(), 1);
        assert_eq!(hook.calls.load(Ordering::SeqCst), 0);
    }
    let f = seed(store.as_ref(), "delivery-cancel").await;
    OrganizationService::new(store.clone())
        .create_invitation(&f.owner_token, false, &request(&f), f.now)
        .await
        .unwrap();
    let hook = Arc::new(Hook::new(store.clone(), Mode::Success));
    let svc = OrganizationService::new(store.clone())
        .with_invitation_delivery(hook.clone())
        .with_cancel_pending_invitations_on_reinvite(true)
        .with_invitation_policy(InvitationPolicy::new(3, 0, true));
    assert_eq!(
        svc.create_invitation(&f.owner_token, false, &request(&f), f.now)
            .await
            .err()
            .unwrap()
            .code,
        "INVITATION_LIMIT_REACHED"
    );
    assert_eq!(hook.calls.load(Ordering::SeqCst), 0);
    assert_eq!(rows(store.as_ref(), &f.org).await[0]["status"], "canceled");
}
#[cfg(feature = "http")]
async fn http_delivery<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use tower::ServiceExt;
    for (index, mode) in [Mode::Prepare, Mode::Async].into_iter().enumerate() {
        for resend in [false, true] {
            let f = seed(store.as_ref(), &format!("delivery-http-{index}-{resend}")).await;
            let now = f.now;
            if resend {
                OrganizationService::new(store.clone())
                    .create_invitation(&f.owner_token, false, &request(&f), now)
                    .await
                    .unwrap();
            }
            let hook = Arc::new(Hook::new(store.clone(), mode));
            let app = crate::http::AuthHttp::new(
                store.clone(),
                "http://localhost:3000".into(),
                vec![7; 32],
                vec![],
            )
            .unwrap()
            .with_invitation_delivery(hook.clone())
            .with_invitation_policy(InvitationPolicy::new(3, 100, true))
            .with_rate_limiting(false)
            .with_clock(move || now);
            let cookie = (crate::cookies::SessionCookie { secure: false }).serialize(
                &f.owner_token,
                &[7; 32],
                true,
            );
            let mut body = request(&f);
            body["resend"] = json!(resend);
            let req = Request::builder()
                .method("POST")
                .uri("/organization/invite-member")
                .header("origin", "http://localhost:3000")
                .header("content-type", "application/json")
                .header("cookie", cookie)
                .body(Body::from(body.to_string()))
                .unwrap();
            let response = bounded(app.router().oneshot(req)).await.unwrap();
            assert_eq!(
                response.status(),
                if mode == Mode::Prepare {
                    StatusCode::INTERNAL_SERVER_ERROR
                } else {
                    StatusCode::OK
                }
            );
            assert!(!response.headers().contains_key(header::SET_COOKIE));
            let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
            if mode == Mode::Prepare {
                assert!(bytes.is_empty());
            }
            assert_eq!(rows(store.as_ref(), &f.org).await.len(), 1);
            assert_eq!(hook.calls.load(Ordering::SeqCst), 1);
        }
    }
}
#[tokio::test]
async fn invitation_delivery_persistence_scheduler_and_reentry() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}

async fn canceled_delivery<S: AuthStore + 'static>(store: Arc<S>) {
    let f = seed(store.as_ref(), "delivery-canceled-future").await;
    let mut h = Hook::new(store.clone(), Mode::Success);
    h.wait = Some(Arc::new(tokio::sync::Notify::new()));
    let hook = Arc::new(h);
    let svc = OrganizationService::new(store.clone()).with_invitation_delivery(hook.clone());
    let body = request(&f);
    let mut attempt = Box::pin(svc.create_invitation(&f.owner_token, false, &body, f.now));
    bounded(async{tokio::select!{() = hook.ready.notified()=>{},_ = &mut attempt=>panic!("delivery must remain pending")}}).await;
    drop(attempt);
    assert_eq!(rows(store.as_ref(), &f.org).await.len(), 1);
    assert_eq!(hook.calls.load(Ordering::SeqCst), 1);
}
#[cfg(feature = "http")]
async fn http_dispatcher<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    for reject in [false, true] {
        let f = seed(
            store.as_ref(),
            &format!("delivery-http-dispatcher-{reject}"),
        )
        .await;
        let now = f.now;
        let hook = Arc::new(Hook::new(store.clone(), Mode::Async));
        let dispatcher = Arc::new(Dispatcher {
            tasks: Mutex::new(vec![]),
            calls: AtomicUsize::new(0),
            reject,
        });
        let app = crate::http::AuthHttp::new(
            store.clone(),
            "http://localhost:3000".into(),
            vec![7; 32],
            vec![],
        )
        .unwrap()
        .with_background_dispatcher(dispatcher.clone())
        .with_invitation_delivery(hook.clone())
        .with_rate_limiting(false)
        .with_clock(move || now);
        let cookie = (crate::cookies::SessionCookie { secure: false }).serialize(
            &f.owner_token,
            &[7; 32],
            true,
        );
        let req = Request::builder()
            .method("POST")
            .uri("/organization/invite-member")
            .header("origin", "http://localhost:3000")
            .header("content-type", "application/json")
            .header("cookie", cookie)
            .body(Body::from(request(&f).to_string()))
            .unwrap();
        assert_eq!(
            bounded(app.router().oneshot(req)).await.unwrap().status(),
            StatusCode::OK
        );
        assert_eq!(hook.polled.load(Ordering::SeqCst), 0);
        assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 1);
        assert_eq!(rows(store.as_ref(), &f.org).await.len(), 1);
        let tasks = std::mem::take(&mut *dispatcher.tasks.lock().unwrap());
        for task in tasks {
            bounded(task).await;
        }
        assert_eq!(hook.polled.load(Ordering::SeqCst), 1);
    }
}
