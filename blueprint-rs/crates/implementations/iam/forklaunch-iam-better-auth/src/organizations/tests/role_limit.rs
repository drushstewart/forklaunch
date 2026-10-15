use super::invitation_policy::{Fixture, seed};
use super::*;
use crate::organizations::{OrganizationControls, RoleLimitFuture, RoleLimitPolicy};
use std::sync::atomic::{AtomicUsize, Ordering};
struct Hook(Box<dyn Fn(String) -> BlueprintResult<RoleLimitFuture> + Send + Sync>);
impl RoleLimitPolicy for Hook {
    fn prepare(&self, id: String) -> BlueprintResult<RoleLimitFuture> {
        (self.0)(id)
    }
}
fn constant(value: f64) -> Arc<dyn RoleLimitPolicy> {
    Arc::new(Hook(Box::new(move |_| {
        Ok(Box::pin(async move { Ok(value) }))
    })))
}
async fn setup<S: AuthStore>(store: &S, label: &str) -> Fixture {
    let f = seed(store, &format!("role-limit-{label}")).await;
    let mut tx = store.begin(true).await.unwrap();
    let mut row = tx
        .get(Model::OrganizationRole, &format!("{}-grant", f.org))
        .await
        .unwrap()
        .unwrap();
    row["permission"] = json!("{\"ac\":[\"create\"]}");
    tx.replace(Model::OrganizationRole, row).await.unwrap();
    tx.commit().await.unwrap();
    f
}
async fn run<S: AuthStore>(svc: &OrganizationService<S>, f: &Fixture) -> BlueprintResult<Value> {
    svc.create_organization_role(
        &f.owner_token,
        false,
        &json!({"organizationId":f.org,"role":"new","permission":{}}),
        f.now,
    )
    .await
}
pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    for (i, limit) in [
        0.0,
        -0.0,
        -1.0,
        1.0,
        1.5,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        4_294_967_296.0,
        f64::MAX,
    ]
    .into_iter()
    .enumerate()
    {
        let f = setup(store.as_ref(), &format!("ieee-{i}")).await;
        let svc = OrganizationService::new(store.clone()).with_role_limit_policy(constant(limit));
        let out = run(&svc, &f).await;
        if 1.0 >= limit {
            assert_eq!(out.unwrap_err().code, "TOO_MANY_ROLES");
        } else {
            assert!(out.is_ok(), "{out:?}");
        }
    }
    races(store.clone()).await;
    ordering(store.clone()).await;
    failures_and_grants(store.clone()).await;
    cache_races(store.clone()).await;
    for clear in [false, true] {
        let f = setup(store.as_ref(), &format!("builder-{clear}")).await;
        let svc = OrganizationService::new(store.clone());
        let controls = OrganizationControls::new(false, Some(0));
        let svc = if clear {
            svc.with_role_limit_policy(constant(f64::INFINITY))
                .with_organization_controls(controls)
        } else {
            svc.with_organization_controls(controls)
                .with_role_limit_policy(constant(f64::INFINITY))
        };
        assert_eq!(run(&svc, &f).await.is_err(), clear);
    }
    #[cfg(feature = "http")]
    {
        clock(store.clone()).await;
        http(store).await;
    }
}
async fn races<S: AuthStore + 'static>(store: Arc<S>) {
    for mode in 0..10 {
        let f = setup(store.as_ref(), &format!("race-{mode}")).await;
        let inner = store.clone();
        let org = f.org.clone();
        let user = f.owner.clone();
        let token = f.owner_token.clone();
        let hook = Hook(Box::new(move |id| {
            assert_eq!(id, org);
            if mode == 8 {
                return Err(BlueprintError::new("SECRET", "private error"));
            }
            let inner = inner.clone();
            let org = org.clone();
            let user = user.clone();
            let token = token.clone();
            Ok(Box::pin(async move {
                if mode == 9 {
                    return Err(BlueprintError::new("SECRET", "private error"));
                }
                // Acquiring this writer proves no store transaction is held by the caller.
                let mut tx = inner.begin(true).await?;
                match mode {
                    0|7=>tx.insert(Model::OrganizationRole,json!({"id":format!("{org}-new-row"),"organizationId":org,"role":if mode==7{"new"}else{"other"},"permission":"{}"})).await?,
                    1=>{tx.delete(Model::OrganizationRole,&format!("{org}-grant")).await?;},
                    2=>{let row=tx.find_unique(Model::Session,"token",&token).await?.unwrap();tx.delete(Model::Session,text(&row,"id")?).await?;},
                    3=>{tx.delete(Model::User,&user).await?;},
                    4=>{tx.delete(Model::Member,&format!("{org}-member")).await?;},
                    5=>{let mut row=tx.get(Model::Member,&format!("{org}-member")).await?.unwrap();row["role"]=json!("member");tx.replace(Model::Member,row).await?;},
                    6=>{tx.delete(Model::Organization,&org).await?;},
                    _=>unreachable!(),
                }
                tx.commit().await?;
                Ok(if mode == 0 { 1.0 } else { 100.0 })
            }))
        }));
        let svc = OrganizationService::new(store.clone()).with_role_limit_policy(Arc::new(hook));
        let out = tokio::time::timeout(std::time::Duration::from_secs(10), run(&svc, &f))
            .await
            .unwrap();
        match mode {
            0 => assert_eq!(out.unwrap_err().code, "TOO_MANY_ROLES"),
            7 => assert_eq!(out.unwrap_err().code, "ROLE_NAME_IS_ALREADY_TAKEN"),
            8 | 9 => {
                let e = out.unwrap_err();
                assert_eq!(e.code, "HTTP_INTERNAL_EMPTY");
                assert!(!format!("{e:?}").contains("SECRET"));
            }
            _ => assert!(out.is_ok(), "mode{mode}: {out:?}"),
        }
    }
}
async fn ordering<S: AuthStore + 'static>(store: Arc<S>) {
    for (i, name, permission, code, calls_expected) in [
        (0, "viewer", json!({}), "ROLE_NAME_IS_ALREADY_TAKEN", 0),
        (1, "new", json!({"unknown":["x"]}), "TOO_MANY_ROLES", 1),
        (2, "owner", json!({}), "ROLE_NAME_IS_ALREADY_TAKEN", 0),
    ] {
        let f = setup(store.as_ref(), &format!("order-{i}")).await;
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let svc = OrganizationService::new(store.clone()).with_role_limit_policy(Arc::new(Hook(
            Box::new(move |_| {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(Box::pin(async { Ok(0.0) }))
            }),
        )));
        let e = svc
            .create_organization_role(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"role":name,"permission":permission}),
                f.now,
            )
            .await
            .unwrap_err();
        assert_eq!(e.code, code);
        assert_eq!(calls.load(Ordering::SeqCst), calls_expected);
    }
}
#[cfg(feature = "http")]
async fn clock<S: AuthStore + 'static>(store: Arc<S>) {
    let f = setup(store.as_ref(), "clock").await;
    let current = Arc::new(std::sync::Mutex::new(f.now));
    let source = current.clone();
    let hook_clock = current.clone();
    let later = f.now + chrono::Duration::hours(1);
    let svc = OrganizationService::new(store)
        .with_time_source(crate::time_source::TimeSource::http(Arc::new(move || {
            *source.lock().unwrap()
        })))
        .with_role_limit_policy(Arc::new(Hook(Box::new(move |_| {
            *hook_clock.lock().unwrap() = later;
            Ok(Box::pin(async { Ok(100.0) }))
        }))));
    let out = run(&svc, &f).await.unwrap();
    assert_eq!(out["roleData"]["createdAt"], timestamp(later));
}
#[tokio::test]
async fn awaited_role_limit_numeric_ordering_and_admitted_authority() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}

async fn failures_and_grants<S: AuthStore + 'static>(store: Arc<S>) {
    for permit in [false, true] {
        let f = setup(store.as_ref(), &format!("current-grant-{permit}")).await;
        let mut tx = store.begin(true).await.unwrap();
        let mut member = tx
            .get(Model::Member, &format!("{}-member", f.org))
            .await
            .unwrap()
            .unwrap();
        member["role"] = json!("quota-author");
        tx.replace(Model::Member, member).await.unwrap();
        let mut grant = tx
            .get(Model::OrganizationRole, &format!("{}-grant", f.org))
            .await
            .unwrap()
            .unwrap();
        grant["role"] = json!("quota-author");
        grant["permission"] = json!("{\"ac\":[\"create\"],\"platform\":[\"platform:write\"]}");
        tx.replace(Model::OrganizationRole, grant).await.unwrap();
        tx.commit().await.unwrap();
        let inner = store.clone();
        let org = f.org.clone();
        let svc=OrganizationService::new(store.clone()).with_role_limit_policy(Arc::new(Hook(Box::new(move |_| {
            let inner=inner.clone();let org=org.clone();Ok(Box::pin(async move {
                let mut tx=inner.begin(true).await?;let mut row=tx.get(Model::OrganizationRole,&format!("{org}-grant")).await?.unwrap();
                row["permission"]=json!(json!({"ac":["create"],"platform":if permit{vec!["platform:write"]}else{vec![]}}).to_string());
                tx.replace(Model::OrganizationRole,row).await?;tx.commit().await?;Ok(100.0)
            }))
        }))));
        let out=svc.create_organization_role(&f.owner_token,false,&json!({"organizationId":f.org,"role":"new","permission":{"platform":["platform:write"]}}),f.now).await;
        assert!(out.is_ok(), "{out:?}");
    }
    let f = setup(store.as_ref(), "cancel").await;
    let ready = Arc::new(tokio::sync::Notify::new());
    let signal = ready.clone();
    let svc = OrganizationService::new(store.clone()).with_role_limit_policy(Arc::new(Hook(
        Box::new(move |_| {
            let signal = signal.clone();
            Ok(Box::pin(async move {
                signal.notify_one();
                std::future::pending().await
            }))
        }),
    )));
    {
        let request = run(&svc, &f);
        tokio::pin!(request);
        tokio::select! {()=ready.notified()=>{},_=&mut request=>panic!("callback must await")}
    }
    let mut tx = store.begin(true).await.unwrap();
    assert_eq!(
        tx.find_window(Model::OrganizationRole, "organizationId", &f.org, 0, 1)
            .await
            .unwrap()
            .total,
        1
    );
    tx.commit().await.unwrap();
    for acknowledge in [false, true] {
        let f = setup(store.as_ref(), &format!("commit-{acknowledge}")).await;
        let fault = Arc::new(super::creation_policy::fault_store::FaultStore {
            store: store.clone(),
            read: None,
            page: None,
            delete: None,
            insert: None,
            begin: false,
            commit: true,
            acknowledge,
        });
        let svc = OrganizationService::new(fault).with_role_limit_policy(constant(100.0));
        assert!(run(&svc, &f).await.is_err());
        let mut tx = store.begin(false).await.unwrap();
        assert_eq!(
            tx.find_window(Model::OrganizationRole, "organizationId", &f.org, 0, 1)
                .await
                .unwrap()
                .total,
            if acknowledge { 2 } else { 1 }
        );
        tx.commit().await.unwrap();
    }
}
#[cfg(feature = "http")]
async fn http<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use tower::ServiceExt;
    for mode in 0..4 {
        let f = setup(store.as_ref(), &format!("http-{mode}")).await;
        let now = f.now;
        let hook: Arc<dyn RoleLimitPolicy> = if mode == 1 {
            Arc::new(Hook(Box::new(|_| {
                Err(BlueprintError::new("SECRET", "private"))
            })))
        } else {
            constant(if mode == 0 { 0.0 } else { 100.0 })
        };
        let app = crate::http::AuthHttp::new(
            store.clone(),
            "http://localhost:3000".into(),
            vec![7; 32],
            vec![],
        )
        .unwrap()
        .with_clock(move || now)
        .with_rate_limiting(false)
        .with_role_limit_policy(hook);
        let app = if mode == 3 {
            app.with_organization_controls(OrganizationControls::new(false, Some(0)))
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
            .uri("/organization/create-role")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://localhost:3000")
            .header(header::COOKIE, cookie.split(';').next().unwrap())
            .body(Body::from(
                json!({"organizationId":f.org,"role":"new","permission":{}}).to_string(),
            ))
            .unwrap();
        let response = app.router().oneshot(req).await.unwrap();
        assert_eq!(
            response.status().as_u16(),
            match mode {
                0 | 3 => 400,
                1 => 500,
                _ => 200,
            }
        );
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        if mode == 1 {
            assert!(bytes.is_empty());
        }
    }
}

async fn cache_races<S: AuthStore + 'static>(store: Arc<S>) {
    for mode in 0..4 {
        let f = setup(store.as_ref(), &format!("cache-{mode}")).await;
        let other = setup(store.as_ref(), &format!("cache-other-{mode}")).await;
        let mut tx = store.begin(true).await.unwrap();
        let mut member = tx
            .get(Model::Member, &format!("{}-member", f.org))
            .await
            .unwrap()
            .unwrap();
        member["role"] = json!("quota-author");
        tx.replace(Model::Member, member).await.unwrap();
        let mut grant = tx
            .get(Model::OrganizationRole, &format!("{}-grant", f.org))
            .await
            .unwrap()
            .unwrap();
        grant["role"] = json!("quota-author");
        grant["permission"] = json!("{\"ac\":[\"create\"],\"platform\":[\"platform:write\"]}");
        tx.replace(Model::OrganizationRole, grant).await.unwrap();
        tx.commit().await.unwrap();
        let ready = Arc::new(tokio::sync::Notify::new());
        let resume = Arc::new(tokio::sync::Notify::new());
        let inner = store.clone();
        let org = f.org.clone();
        let token = f.owner_token.clone();
        let now = f.now;
        let signal = ready.clone();
        let release = resume.clone();
        let hook = Hook(Box::new(move |_| {
            let inner = inner.clone();
            let org = org.clone();
            let token = token.clone();
            let signal = signal.clone();
            let release = release.clone();
            let other_org = other.org.clone();
            let other_token = other.owner_token.clone();
            Ok(Box::pin(async move {
                let mut tx = inner.begin(true).await?;
                let mut row = tx
                    .get(Model::OrganizationRole, &format!("{org}-grant"))
                    .await?
                    .unwrap();
                row["permission"] = json!("{\"ac\":[\"create\"]}");
                tx.replace(Model::OrganizationRole, row).await?;
                tx.commit().await?;
                if mode == 2 {
                    signal.notify_one();
                    release.notified().await;
                } else {
                    let mut svc = OrganizationService::new(inner.clone());
                    if mode == 3 {
                        let mut tx = inner.begin(true).await?;
                        tx.delete(Model::OrganizationRole, &format!("{org}-grant"))
                            .await?;
                        tx.commit().await?;
                        svc = svc.with_role_authorizers(cache_authorizer());
                    }
                    let (org, token) = if mode == 1 {
                        (other_org, other_token)
                    } else {
                        (org, token)
                    };
                    svc.has_permission(&token,false,&json!({"organizationId":org,"permissions":{"platform":["platform:write"]}}),now).await?;
                }
                Ok(100.0)
            }))
        }));
        let svc = OrganizationService::new(store.clone()).with_role_limit_policy(Arc::new(hook));
        let body = json!({"organizationId":f.org,"role":"new","permission":{"platform":["platform:write"]}});
        let request = svc.create_organization_role(&f.owner_token, false, &body, f.now);
        tokio::pin!(request);
        if mode == 2 {
            tokio::select! {()=ready.notified()=>{},_=&mut request=>panic!("callback must wait")};
            OrganizationService::new(store.clone())
                .has_permission(
                    &f.owner_token,
                    false,
                    &json!({"organizationId":f.org,"permissions":{"platform":["platform:write"]}}),
                    f.now,
                )
                .await
                .unwrap();
            resume.notify_one();
        }
        let out = request.await;
        assert_eq!(out.is_ok(), mode == 1, "cache mode{mode}: {out:?}");
    }
}
fn cache_authorizer() -> crate::organizations::OrganizationRoleAuthorizers {
    use crate::organizations::{
        OrganizationAccessControl, OrganizationAuthorizationRequest, OrganizationRoleAuthorizer,
        OrganizationRoleAuthorizers,
    };
    use std::collections::BTreeMap;
    struct Deny;
    impl OrganizationRoleAuthorizer for Deny {
        fn authorize(
            &self,
            request: OrganizationAuthorizationRequest<'_>,
        ) -> BlueprintResult<bool> {
            assert_eq!(request.role(), "quota-author");
            Ok(false)
        }
    }
    let config = OrganizationAccessControl::new(
        BTreeMap::from([("platform".into(), vec!["platform:write".into()])]),
        BTreeMap::from([(
            "quota-author".into(),
            BTreeMap::from([("platform".into(), vec!["platform:write".into()])]),
        )]),
    )
    .unwrap();
    let callback: Arc<dyn OrganizationRoleAuthorizer> = Arc::new(Deny);
    OrganizationRoleAuthorizers::new(config, BTreeMap::from([("quota-author".into(), callback)]))
        .unwrap()
}
