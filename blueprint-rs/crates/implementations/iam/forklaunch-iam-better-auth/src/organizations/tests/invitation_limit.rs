use super::invitation_policy::{Fixture, request, rows, seed};
use super::*;
use crate::organizations::{
    InvitationLimitContext, InvitationLimitFuture, InvitationLimitPolicy, InvitationPolicy,
};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
struct Hook(
    Box<dyn Fn(InvitationLimitContext) -> BlueprintResult<InvitationLimitFuture> + Send + Sync>,
);
impl InvitationLimitPolicy for Hook {
    fn prepare(&self, context: InvitationLimitContext) -> BlueprintResult<InvitationLimitFuture> {
        (self.0)(context)
    }
}
fn constant(limit: f64) -> Arc<dyn InvitationLimitPolicy> {
    Arc::new(Hook(Box::new(move |ctx| {
        assert_eq!(ctx.member()["userId"], ctx.user()["id"]);
        assert_eq!(ctx.member()["organizationId"], ctx.organization()["id"]);
        assert!(ctx.member().get("user").is_none());
        Ok(Box::pin(async move { Ok(limit) }))
    })))
}
async fn run<S: AuthStore>(svc: &OrganizationService<S>, f: &Fixture) -> BlueprintResult<Value> {
    svc.create_invitation(&f.owner_token, false, &request(f), f.now)
        .await
}
async fn pending<S: AuthStore>(store: &S, f: &Fixture, id: &str, email: &str, expired: bool) {
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Invitation,json!({"id":format!("{}-{id}",f.org),"organizationId":f.org,"email":email,"role":"member","inviterId":f.owner,"status":"pending","createdAt":timestamp(f.now),"expiresAt":timestamp(f.now+chrono::Duration::hours(if expired{-1}else{1}))})).await.unwrap();
    tx.commit().await.unwrap();
}
pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    for (i, limit) in [
        0.0,
        -0.0,
        -1.0,
        0.5,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        4_294_967_296.0,
        f64::MAX,
    ]
    .into_iter()
    .enumerate()
    {
        let f = seed(store.as_ref(), &format!("invitation-limit-ieee-{i}")).await;
        let svc =
            OrganizationService::new(store.clone()).with_invitation_limit_policy(constant(limit));
        let out = run(&svc, &f).await;
        if 0.0 >= limit {
            assert_eq!(out.unwrap_err().code, "INVITATION_LIMIT_REACHED");
        } else {
            assert!(out.is_ok(), "{out:?}");
        }
    }
    cancellation_and_failures(store.clone()).await;
    mutations(store.clone()).await;
    windows(store.clone()).await;
    delivery_and_projection(store.clone()).await;
    for clear in [false, true] {
        let f = seed(store.as_ref(), &format!("invitation-limit-builder-{clear}")).await;
        let svc = OrganizationService::new(store.clone());
        let policy = InvitationPolicy::new(10, 0, true);
        let svc = if clear {
            svc.with_invitation_limit_policy(constant(100.0))
                .with_invitation_policy(policy)
        } else {
            svc.with_invitation_policy(policy)
                .with_invitation_limit_policy(constant(100.0))
        };
        assert_eq!(run(&svc, &f).await.is_err(), clear);
    }
    #[cfg(feature = "http")]
    clock_and_http(store).await;
}
async fn cancellation_and_failures<S: AuthStore + 'static>(store: Arc<S>) {
    for mode in 0..6 {
        let f = seed(store.as_ref(), &format!("invitation-limit-cancel-{mode}")).await;
        pending(
            store.as_ref(),
            &f,
            "old",
            &format!("{}@example.test", f.recipient),
            false,
        )
        .await;
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let inner = store.clone();
        let org = f.org.clone();
        let hook = Hook(Box::new(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            if mode == 2 {
                return Err(BlueprintError::new("SECRET", "private"));
            }
            let inner = inner.clone();
            let org = org.clone();
            Ok(Box::pin(async move {
                let tx = inner.begin(true).await?;
                tx.commit().await?;
                if mode == 3 {
                    return Err(BlueprintError::new("SECRET", "private"));
                }
                let mut tx = inner.begin(false).await?;
                if mode >= 2 {
                    assert_eq!(
                        tx.get(Model::Invitation, &format!("{org}-old"))
                            .await?
                            .unwrap()["status"],
                        "canceled"
                    );
                }
                tx.commit().await?;
                Ok(if mode == 4 { 0.0 } else { 100.0 })
            }))
        }));
        let svc = OrganizationService::new(store.clone())
            .with_cancel_pending_invitations_on_reinvite(mode >= 2)
            .with_invitation_limit_policy(Arc::new(hook));
        let mut body = request(&f);
        if mode == 1 {
            body["resend"] = json!(true);
        }
        if mode == 5 {
            body["teamId"] = json!("missing");
        }
        let out = svc
            .create_invitation(&f.owner_token, false, &body, f.now)
            .await;
        let code = match mode {
            0 => Some("USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION"),
            1 => None,
            2 | 3 => Some("HTTP_INTERNAL_EMPTY"),
            4 => Some("INVITATION_LIMIT_REACHED"),
            _ => Some("TEAM_NOT_FOUND"),
        };
        if let Some(code) = code {
            assert_eq!(out.unwrap_err().code, code);
        } else {
            assert!(out.is_ok());
        }
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(mode >= 2));
        let existing = rows(store.as_ref(), &f.org).await;
        assert_eq!(existing.len(), 1);
        assert_eq!(
            existing[0]["status"],
            if mode >= 2 { "canceled" } else { "pending" }
        );
    }
    commit_and_drop_failures(store).await;
}
async fn commit_and_drop_failures<S: AuthStore + 'static>(store: Arc<S>) {
    use super::invitation_cancel::store::{Failure, Store};
    for (i, failure) in [
        Failure::CancelCommit,
        Failure::CancelAcknowledgement,
        Failure::Insert,
        Failure::ReplacementCommit,
        Failure::ReplacementAcknowledgement,
    ]
    .into_iter()
    .enumerate()
    {
        let f = seed(store.as_ref(), &format!("invitation-limit-commit-{i}")).await;
        pending(
            store.as_ref(),
            &f,
            "old",
            &format!("{}@example.test", f.recipient),
            false,
        )
        .await;
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let wrapped = Arc::new(Store {
            inner: store.clone(),
            after: Mutex::new(None),
            failure,
        });
        let svc = OrganizationService::new(wrapped)
            .with_cancel_pending_invitations_on_reinvite(true)
            .with_invitation_limit_policy(Arc::new(Hook(Box::new(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(Box::pin(async { Ok(100.0) }))
            }))));
        assert!(run(&svc, &f).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(i >= 2));
        let existing = rows(store.as_ref(), &f.org).await;
        assert_eq!(
            existing.len(),
            if failure == Failure::ReplacementAcknowledgement {
                2
            } else {
                1
            }
        );
        assert_eq!(
            existing[0]["status"],
            if failure == Failure::CancelCommit {
                "pending"
            } else {
                "canceled"
            }
        );
    }
    let f = seed(store.as_ref(), "invitation-limit-drop").await;
    pending(
        store.as_ref(),
        &f,
        "old",
        &format!("{}@example.test", f.recipient),
        false,
    )
    .await;
    let ready = Arc::new(tokio::sync::Notify::new());
    let signal = ready.clone();
    let svc = OrganizationService::new(store.clone())
        .with_cancel_pending_invitations_on_reinvite(true)
        .with_invitation_limit_policy(Arc::new(Hook(Box::new(move |_| {
            let signal = signal.clone();
            Ok(Box::pin(async move {
                signal.notify_one();
                std::future::pending().await
            }))
        }))));
    {
        let request = run(&svc, &f);
        tokio::pin!(request);
        tokio::select! {()=ready.notified()=>{},_=&mut request=>panic!("callback must await")}
    }
    let existing = rows(store.as_ref(), &f.org).await;
    assert_eq!(existing.len(), 1);
    assert_eq!(existing[0]["status"], "canceled");
}
async fn mutations<S: AuthStore + 'static>(store: Arc<S>) {
    for mode in 0..10 {
        let f = seed(store.as_ref(), &format!("invitation-limit-race-{mode}")).await;
        if mode == 8 {
            pending(store.as_ref(), &f, "deleted", "other@example.test", false).await;
        }
        let inner = store.clone();
        let org = f.org.clone();
        let user = f.owner.clone();
        let token = f.owner_token.clone();
        let recipient = f.recipient.clone();
        let now = f.now;
        let hook = Hook(Box::new(move |_| {
            let inner = inner.clone();
            let org = org.clone();
            let user = user.clone();
            let token = token.clone();
            let recipient = recipient.clone();
            Ok(Box::pin(async move {
                let mut tx = inner.begin(true).await?;
                match mode{
            0|7=>tx.insert(Model::Invitation,json!({"id":format!("{org}-concurrent"),"organizationId":org,"email":if mode==7{format!("{recipient}@example.test")}else{"other@example.test".into()},"role":"member","inviterId":user,"status":"pending","expiresAt":timestamp(now+chrono::Duration::hours(1))})).await?,
            1=>{let row=tx.find_unique(Model::Session,"token",&token).await?.unwrap();tx.delete(Model::Session,text(&row,"id")?).await?;},
            2=>{tx.delete(Model::User,&user).await?;},3=>{tx.delete(Model::Member,&format!("{org}-member")).await?;},4=>{let mut row=tx.get(Model::Member,&format!("{org}-member")).await?.unwrap();row["role"]=json!("member");tx.replace(Model::Member,row).await?;},5=>{tx.delete(Model::Organization,&org).await?;},6=>tx.insert(Model::Member,json!({"id":format!("{org}-target-member"),"organizationId":org,"userId":recipient,"role":"member"})).await?,8=>{tx.delete(Model::Invitation,&format!("{org}-deleted")).await?;},9=>{let mut row=tx.get(Model::User,&user).await?.unwrap();row["email"]=json!("changed@example.test");tx.replace(Model::User,row).await?;},_=>unreachable!()}
                tx.commit().await?;
                Ok(if mode == 0 || mode == 8 { 1.0 } else { 100.0 })
            }))
        }));
        let svc =
            OrganizationService::new(store.clone()).with_invitation_limit_policy(Arc::new(hook));
        let out = tokio::time::timeout(std::time::Duration::from_secs(10), run(&svc, &f))
            .await
            .unwrap();
        if mode == 0 {
            assert_eq!(out.unwrap_err().code, "INVITATION_LIMIT_REACHED");
        } else {
            assert!(out.is_ok(), "mode{mode} {out:?}");
        }
    }
}
async fn windows<S: AuthStore + 'static>(store: Arc<S>) {
    for (label, count, expired, limit, ok) in [
        ("saturated", 101, 0, 101.0, true),
        ("expired", 101, 100, 1.0, true),
        ("at-limit", 1, 0, 1.0, false),
    ] {
        let f = seed(store.as_ref(), &format!("invitation-limit-window-{label}")).await;
        for i in 0..count {
            pending(
                store.as_ref(),
                &f,
                &format!("pending-{i}"),
                &format!("window-{i}@example.test"),
                i < expired,
            )
            .await;
        }
        let svc =
            OrganizationService::new(store.clone()).with_invitation_limit_policy(constant(limit));
        assert_eq!(run(&svc, &f).await.is_ok(), ok);
    }
}
#[cfg(feature = "http")]
async fn clock_and_http<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use tower::ServiceExt;
    for mode in 0..4 {
        let f = seed(store.as_ref(), &format!("invitation-limit-http-{mode}")).await;
        let now = f.now;
        let later = now + chrono::Duration::hours(2);
        let clock = Arc::new(Mutex::new(now));
        let source = clock.clone();
        let change = clock.clone();
        pending(store.as_ref(), &f, "expires", "other@example.test", false).await;
        let hook = Hook(Box::new(move |_| {
            *change.lock().unwrap() = later;
            if mode == 1 {
                return Err(BlueprintError::new("SECRET", "private"));
            }
            Ok(Box::pin(
                async move { Ok(if mode == 0 { 0.0 } else { 1.0 }) },
            ))
        }));
        let app = crate::http::AuthHttp::new(
            store.clone(),
            "http://localhost:3000".into(),
            vec![7; 32],
            vec![],
        )
        .unwrap()
        .with_clock(move || *source.lock().unwrap())
        .with_rate_limiting(false)
        .with_invitation_limit_policy(Arc::new(hook));
        let app = if mode == 3 {
            app.with_invitation_policy(InvitationPolicy::new(10, 0, true))
        } else {
            app
        };
        let cookie = (crate::cookies::SessionCookie { secure: false }).serialize(
            &f.owner_token,
            &[7; 32],
            true,
        );
        let req = Request::builder()
            .method("POST")
            .uri("/organization/invite-member")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://localhost:3000")
            .header(header::COOKIE, cookie.split(';').next().unwrap())
            .body(Body::from(request(&f).to_string()))
            .unwrap();
        let response = app.router().oneshot(req).await.unwrap();
        assert_eq!(
            response.status().as_u16(),
            match mode {
                1 => 500,
                2 => 200,
                _ => 403,
            }
        );
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        if mode == 1 {
            assert!(bytes.is_empty());
        }
        if mode == 2 {
            let row: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(row["createdAt"], timestamp(later));
            assert_eq!(
                row["expiresAt"],
                timestamp(later + chrono::Duration::hours(48))
            );
        }
    }
}
#[tokio::test]
async fn invitation_quota_callback_order_ieee_cancellation_and_races() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}

struct Delivery<S> {
    store: Arc<S>,
    mode: u8,
    events: Arc<Mutex<Vec<&'static str>>>,
}
impl<S: AuthStore + 'static> crate::organizations::InvitationEmailDelivery for Delivery<S> {
    fn prepare(
        &self,
        context: crate::organizations::InvitationDeliveryContext,
    ) -> BlueprintResult<crate::background::DeliveryFuture> {
        self.events.lock().unwrap().push("delivery");
        assert_eq!(context.inviter()["role"], "owner");
        if self.mode == 2 {
            return Err(BlueprintError::new("SECRET", "private delivery"));
        }
        let store = self.store.clone();
        let org = context.organization()["id"].as_str().unwrap().to_owned();
        let mode = self.mode;
        Ok(Box::pin(async move {
            let mut tx = store.begin(false).await?;
            assert_eq!(
                tx.find_window(Model::Invitation, "organizationId", &org, 0, 1)
                    .await?
                    .total,
                2
            );
            tx.commit().await?;
            if mode == 3 {
                Err(BlueprintError::new("SECRET", "private delivery"))
            } else {
                Ok(())
            }
        }))
    }
}
async fn delivery_and_projection<S: AuthStore + 'static>(store: Arc<S>) {
    for mode in 0..4u8 {
        let f = seed(store.as_ref(), &format!("invitation-limit-delivery-{mode}")).await;
        pending(
            store.as_ref(),
            &f,
            "old",
            &format!("{}@example.test", f.recipient),
            false,
        )
        .await;
        let events = Arc::new(Mutex::new(vec![]));
        let record = events.clone();
        let svc = OrganizationService::new(store.clone())
            .with_cancel_pending_invitations_on_reinvite(true)
            .with_invitation_limit_policy(Arc::new(Hook(Box::new(move |_| {
                record.lock().unwrap().push("quota");
                Ok(Box::pin(
                    async move { Ok(if mode == 0 { 0.0 } else { 100.0 }) },
                ))
            }))))
            .with_invitation_delivery(Arc::new(Delivery {
                store: store.clone(),
                mode,
                events: events.clone(),
            }));
        let out = run(&svc, &f).await;
        assert_eq!(out.is_ok(), mode == 1 || mode == 3);
        assert_eq!(
            *events.lock().unwrap(),
            if mode == 0 {
                vec!["quota"]
            } else {
                vec!["quota", "delivery"]
            }
        );
        let existing = rows(store.as_ref(), &f.org).await;
        assert_eq!(existing.len(), if mode == 0 { 1 } else { 2 });
        assert_eq!(existing[0]["status"], "canceled");
    }
    for user in [false, true] {
        let f = seed(
            store.as_ref(),
            &format!("invitation-limit-projection-{user}"),
        )
        .await;
        pending(
            store.as_ref(),
            &f,
            "old",
            &format!("{}@example.test", f.recipient),
            false,
        )
        .await;
        let mut tx = store.begin(true).await.unwrap();
        let (model, id, field) = if user {
            (Model::User, f.owner.as_str(), "name")
        } else {
            (Model::Organization, f.org.as_str(), "metadata")
        };
        let mut row = tx.get(model, id).await.unwrap().unwrap();
        row[field] = json!({"nested":[{"secret":"not cloned recursively"}]});
        tx.replace(model, row).await.unwrap();
        tx.commit().await.unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let svc = OrganizationService::new(store.clone())
            .with_cancel_pending_invitations_on_reinvite(true)
            .with_invitation_limit_policy(Arc::new(Hook(Box::new(move |_| {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(Box::pin(async { Ok(100.0) }))
            }))));
        assert_eq!(run(&svc, &f).await.unwrap_err().code, "HTTP_INTERNAL_EMPTY");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let existing = rows(store.as_ref(), &f.org).await;
        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0]["status"], "canceled");
        // No callback means no new projection admission on the legacy path.
        assert!(
            run(&OrganizationService::new(store.clone()), &f)
                .await
                .is_ok()
        );
    }
}
