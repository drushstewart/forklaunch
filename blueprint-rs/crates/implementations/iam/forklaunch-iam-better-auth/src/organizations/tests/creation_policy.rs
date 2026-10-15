use super::*;
use crate::organizations::{
    OrganizationCreationContext, OrganizationCreationFuture, OrganizationCreationPolicy,
    OrganizationPolicy,
};
use std::sync::Mutex;

type Work = dyn Fn(OrganizationCreationContext) -> BlueprintResult<OrganizationCreationFuture>
    + Send
    + Sync;
struct Hook(Box<Work>);
impl OrganizationCreationPolicy for Hook {
    fn prepare(
        &self,
        context: OrganizationCreationContext,
    ) -> BlueprintResult<OrganizationCreationFuture> {
        (self.0)(context)
    }
}
struct Fixture {
    user: String,
    token: String,
    session: String,
    slug: String,
    now: DateTime<Utc>,
}
async fn seed<S: AuthStore>(store: &S, label: &str) -> Fixture {
    let user = format!("creation-policy-{label}");
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User,json!({"id":user,"name":"Original","email":format!("{user}@example.test"),"emailVerified":true,"privateField":"not-delivered"})).await.unwrap();
    let session = crate::sessions::new_session(&user, "", "", now).unwrap();
    let token = session["token"].as_str().unwrap().to_owned();
    let id = session["id"].as_str().unwrap().to_owned();
    tx.insert(Model::Session, session).await.unwrap();
    tx.commit().await.unwrap();
    Fixture {
        slug: user.clone(),
        user,
        token,
        session: id,
        now,
    }
}
fn body(f: &Fixture) -> Value {
    json!({"name":"Created","slug":f.slug})
}
fn constant(value: bool) -> Arc<dyn OrganizationCreationPolicy> {
    Arc::new(Hook(Box::new(move |_| {
        Ok(Box::pin(async move { Ok(value) }))
    })))
}
fn recorded(
    stage: &'static str,
    value: bool,
    mode: u8,
    calls: Arc<Mutex<Vec<String>>>,
) -> Arc<dyn OrganizationCreationPolicy> {
    Arc::new(Hook(Box::new(move |ctx| {
        assert!(ctx.user().get("privateField").is_none());
        calls.lock().unwrap().push(stage.into());
        if mode == 1 {
            return Err(BlueprintError::new("secret", "private"));
        }
        Ok(Box::pin(async move {
            if mode == 2 {
                Err(BlueprintError::new("secret", "private"))
            } else {
                Ok(value)
            }
        }))
    })))
}
#[allow(clippy::too_many_lines)]
pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    for (index, allow, quota, error_stage, mode, code) in [
        (0, true, false, "", 0, None),
        (
            1,
            false,
            false,
            "",
            0,
            Some("YOU_ARE_NOT_ALLOWED_TO_CREATE_A_NEW_ORGANIZATION"),
        ),
        (
            2,
            true,
            true,
            "",
            0,
            Some("YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_ORGANIZATIONS"),
        ),
        (3, true, false, "allow", 1, Some("HTTP_INTERNAL_EMPTY")),
        (4, true, false, "allow", 2, Some("HTTP_INTERNAL_EMPTY")),
        (5, true, false, "limit", 1, Some("HTTP_INTERNAL_EMPTY")),
        (6, true, false, "limit", 2, Some("HTTP_INTERNAL_EMPTY")),
    ] {
        let f = seed(store.as_ref(), &format!("matrix-{index}")).await;
        let calls = Arc::new(Mutex::new(vec![]));
        let service = OrganizationService::new(store.clone())
            .with_organization_policy(OrganizationPolicy::new(false, Some(0), 100))
            .with_allow_user_to_create_organization(recorded(
                "allow",
                allow,
                if error_stage == "allow" { mode } else { 0 },
                calls.clone(),
            ))
            .with_organization_limit_policy(recorded(
                "limit",
                quota,
                if error_stage == "limit" { mode } else { 0 },
                calls.clone(),
            ));
        let result = service.create(&f.token, false, &body(&f), f.now).await;
        if let Some(code) = code {
            assert_eq!(result.unwrap_err().code, code);
        } else {
            assert!(result.is_ok());
        }
        assert_eq!(
            *calls.lock().unwrap(),
            if !allow || error_stage == "allow" {
                vec!["allow"]
            } else {
                vec!["allow", "limit"]
            }
        );
        let mut tx = store.begin(false).await.unwrap();
        assert_eq!(
            tx.find_unique(Model::Organization, "slug", &f.slug)
                .await
                .unwrap()
                .is_some(),
            code.is_none()
        );
        tx.commit().await.unwrap();
    }
    for stage in ["allow", "limit"] {
        for mutation in [
            "revoke",
            "delete-user",
            "change-user",
            "slug",
            "quota",
            "token",
            "session-id",
            "session-user",
            "replace-token",
        ] {
            let f = seed(store.as_ref(), &format!("race-{stage}-{mutation}")).await;
            let user = f.user.clone();
            let token = f.token.clone();
            let id = f.session.clone();
            let slug = f.slug.clone();
            let inner = store.clone();
            let callback = Arc::new(Hook(Box::new(move |ctx| {
                assert_eq!(ctx.user()["name"], "Original");
                let (user, token, id, slug, store) = (
                    user.clone(),
                    token.clone(),
                    id.clone(),
                    slug.clone(),
                    inner.clone(),
                );
                Ok(Box::pin(async move {
                    // Reentry requires no outer transaction or writer lock during callback.
                    let mut tx = store.begin(true).await?;
                    match mutation {
                        "revoke" => {
                            tx.delete(Model::Session, &id).await?;
                        }
                        "delete-user" => {
                            tx.delete(Model::User, &user).await?;
                        }
                        "change-user" => {
                            let mut row = tx.get(Model::User, &user).await?.unwrap();
                            row["name"] = json!("Changed");
                            tx.replace(Model::User, row).await?;
                        }
                        "slug" => {
                            tx.insert(
                                Model::Organization,
                                json!({"id":slug,"slug":slug,"name":"Concurrent"}),
                            )
                            .await?;
                        }
                        "quota" => {
                            tx.insert(Model::Member,json!({"id":slug,"organizationId":"concurrent","userId":user,"role":"member"})).await?;
                        }
                        _ => {
                            let mut row = tx.get(Model::Session, &id).await?.unwrap();
                            match mutation {
                                "token" => {
                                    row["token"] = json!(format!("changed-{token}"));
                                    tx.replace(Model::Session, row).await?;
                                }
                                "session-id" => {
                                    tx.delete(Model::Session, &id).await?;
                                    row["id"] = json!(format!("changed-{id}"));
                                    tx.insert(Model::Session, row).await?;
                                }
                                "session-user" => {
                                    row["userId"] = json!("repurposed");
                                    tx.replace(Model::Session, row).await?;
                                }
                                "replace-token" => {
                                    row["token"] = json!(format!("changed-{token}"));
                                    tx.replace(Model::Session, row.clone()).await?;
                                    row["token"] = json!(token);
                                    row["id"] = json!(format!("replacement-{id}"));
                                    row["userId"] = json!("repurposed");
                                    tx.insert(Model::Session, row).await?;
                                }
                                _ => unreachable!(),
                            }
                        }
                    }
                    tx.commit().await?;
                    Ok(stage == "allow")
                }))
            }))) as Arc<dyn OrganizationCreationPolicy>;
            let check = Arc::new(Hook(Box::new(move |ctx| {
                assert_eq!(ctx.user()["name"], "Original");
                Ok(Box::pin(async move { Ok(stage != "allow") }))
            }))) as Arc<dyn OrganizationCreationPolicy>;
            let service = OrganizationService::new(store.clone());
            let service = if stage == "allow" {
                service
                    .with_allow_user_to_create_organization(callback)
                    .with_organization_limit_policy(check)
            } else {
                service
                    .with_allow_user_to_create_organization(check)
                    .with_organization_limit_policy(callback)
            };
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                service.create(&f.token, false, &body(&f), f.now),
            )
            .await
            .unwrap();
            if mutation == "slug" {
                assert_eq!(result.unwrap_err().code, "ORGANIZATION_ALREADY_EXISTS");
                continue;
            }
            let org = result.unwrap();
            assert_eq!(org["members"][0]["userId"], f.user);
            let mut tx = store.begin(false).await.unwrap();
            let session = tx
                .find_unique(Model::Session, "token", &f.token)
                .await
                .unwrap();
            if ["revoke", "token"].contains(&mutation) {
                assert!(session.is_none());
            } else {
                assert_eq!(session.unwrap()["activeOrganizationId"], org["id"]);
            }
            tx.commit().await.unwrap();
        }
    }
    // Last scalar installation clears callbacks; reverse order installs the override.
    for reverse in [false, true] {
        let f = seed(store.as_ref(), &format!("order-{reverse}")).await;
        let service = OrganizationService::new(store.clone());
        let policy = OrganizationPolicy::new(false, None, 100);
        let service = if reverse {
            service
                .with_allow_user_to_create_organization(constant(true))
                .with_organization_policy(policy)
        } else {
            service
                .with_organization_policy(policy)
                .with_allow_user_to_create_organization(constant(true))
        };
        assert_eq!(
            service
                .create(&f.token, false, &body(&f), f.now)
                .await
                .is_ok(),
            !reverse
        );
    }
    cancellation_and_failures(store.clone()).await;
    #[cfg(feature = "http")]
    {
        http(store.clone()).await;
        delayed_http_clock(store).await;
    }
}
#[tokio::test]
async fn awaited_creation_policy_order_reentry_races_and_selectors() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}

#[cfg(feature = "http")]
async fn http<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use tower::ServiceExt;
    for (reverse, mode) in [(false, 0), (true, 0), (false, 1), (false, 2)] {
        let f = seed(store.as_ref(), &format!("http-{reverse}-{mode}")).await;
        let now = f.now;
        let app = crate::http::AuthHttp::new(
            store.clone(),
            "http://localhost:3000".into(),
            vec![7; 32],
            vec![],
        )
        .unwrap()
        .with_clock(move || now)
        .with_rate_limiting(false);
        let policy = OrganizationPolicy::new(false, None, 100);
        let hook = recorded("allow", true, mode, Arc::new(Mutex::new(vec![])));
        let app = if reverse {
            app.with_allow_user_to_create_organization(hook)
                .with_organization_policy(policy)
        } else {
            app.with_organization_policy(policy)
                .with_allow_user_to_create_organization(hook)
        };
        let cookie =
            (crate::cookies::SessionCookie { secure: false }).serialize(&f.token, &[7; 32], true);
        let response = app
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/organization/create")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ORIGIN, "http://localhost:3000")
                    .header(header::COOKIE, cookie.split(';').next().unwrap())
                    .body(Body::from(body(&f).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status().as_u16(),
            if mode > 0 {
                500
            } else if reverse {
                403
            } else {
                200
            }
        );
        let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
        assert_eq!(bytes.is_empty(), mode > 0);
    }
}

#[path = "creation_policy_store.rs"]
mod fault_store;
#[allow(clippy::too_many_lines)]
async fn cancellation_and_failures<S: AuthStore + 'static>(store: Arc<S>) {
    for stage in ["allow", "limit"] {
        let f = seed(store.as_ref(), &format!("cancel-{stage}")).await;
        let ready = Arc::new(tokio::sync::Notify::new());
        let signal = ready.clone();
        let hook = Arc::new(Hook(Box::new(move |_| {
            let signal = signal.clone();
            Ok(Box::pin(async move {
                signal.notify_one();
                std::future::pending().await
            }))
        })));
        let service = OrganizationService::new(store.clone());
        let service = if stage == "allow" {
            service.with_allow_user_to_create_organization(hook)
        } else {
            service.with_organization_limit_policy(hook)
        };
        let b = body(&f);
        {
            let request = service.create(&f.token, false, &b, f.now);
            tokio::pin!(request);
            tokio::select! {()=ready.notified()=>{}, _=&mut request=>panic!("policy must remain pending")};
        }
        let mut tx = store.begin(true).await.unwrap();
        assert!(
            tx.find_unique(Model::Organization, "slug", &f.slug)
                .await
                .unwrap()
                .is_none()
        );
        tx.commit().await.unwrap();
    }
    for allowed in [false, true] {
        let f = seed(store.as_ref(), &format!("read-failure-{allowed}")).await;
        let fault = Arc::new(fault_store::FaultStore {
            store: store.clone(),
            read: None,
            page: Some(Model::Member),
            delete: None,
            insert: None,
            begin: false,
            commit: false,
            acknowledge: false,
        });
        let calls = Arc::new(Mutex::new(vec![]));
        let service = OrganizationService::new(fault)
            .with_allow_user_to_create_organization(recorded("allow", allowed, 0, calls.clone()))
            .with_organization_limit_policy(recorded("limit", false, 0, calls.clone()));
        assert_eq!(
            service
                .create(&f.token, false, &body(&f), f.now)
                .await
                .unwrap_err()
                .code,
            if allowed {
                "better_auth_store_failure"
            } else {
                "YOU_ARE_NOT_ALLOWED_TO_CREATE_A_NEW_ORGANIZATION"
            }
        );
        assert_eq!(*calls.lock().unwrap(), vec!["allow"]);
    }
    for acknowledge in [false, true] {
        let f = seed(store.as_ref(), &format!("commit-{acknowledge}")).await;
        let fault = Arc::new(fault_store::FaultStore {
            store: store.clone(),
            read: None,
            page: None,
            delete: None,
            insert: None,
            begin: false,
            commit: true,
            acknowledge,
        });
        let calls = Arc::new(Mutex::new(vec![]));
        let service = OrganizationService::new(fault)
            .with_allow_user_to_create_organization(recorded("allow", true, 0, calls.clone()));
        assert_eq!(
            service
                .create(&f.token, false, &body(&f), f.now)
                .await
                .unwrap_err()
                .code,
            "better_auth_store_failure"
        );
        assert_eq!(*calls.lock().unwrap(), vec!["allow"]);
        let mut tx = store.begin(false).await.unwrap();
        assert_eq!(
            tx.find_unique(Model::Organization, "slug", &f.slug)
                .await
                .unwrap()
                .is_some(),
            acknowledge
        );
        tx.commit().await.unwrap();
    }
    for hook_present in [false, true] {
        let f = seed(store.as_ref(), &format!("projection-{hook_present}")).await;
        let mut tx = store.begin(true).await.unwrap();
        let mut user = tx.get(Model::User, &f.user).await.unwrap().unwrap();
        user["name"] = json!({"nested":"invalid"});
        tx.replace(Model::User, user).await.unwrap();
        tx.commit().await.unwrap();
        let calls = Arc::new(Mutex::new(vec![]));
        let service = OrganizationService::new(store.clone());
        let service = if hook_present {
            service.with_allow_user_to_create_organization(recorded(
                "allow",
                true,
                0,
                calls.clone(),
            ))
        } else {
            service
        };
        let result = service.create(&f.token, false, &body(&f), f.now).await;
        if hook_present {
            assert_eq!(result.unwrap_err().code, "HTTP_INTERNAL_EMPTY");
        } else {
            assert!(result.is_ok());
        }
        assert!(calls.lock().unwrap().is_empty());
    }
    let f = seed(store.as_ref(), "missing-session").await;
    let calls = Arc::new(Mutex::new(vec![]));
    let service = OrganizationService::new(store).with_allow_user_to_create_organization(recorded(
        "allow",
        true,
        0,
        calls.clone(),
    ));
    assert_eq!(
        service
            .create("missing-token", false, &body(&f), f.now)
            .await
            .unwrap_err()
            .code,
        "UNAUTHORIZED"
    );
    assert!(calls.lock().unwrap().is_empty());
}

#[cfg(feature = "http")]
async fn delayed_http_clock<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use std::sync::atomic::{AtomicI64, Ordering};
    use tower::ServiceExt;
    let f = seed(store.as_ref(), "http-clock").await;
    let clock = Arc::new(AtomicI64::new(f.now.timestamp()));
    let sampled = clock.clone();
    let hook = Arc::new(Hook(Box::new(move |_| {
        let clock = clock.clone();
        Ok(Box::pin(async move {
            clock.fetch_add(5, Ordering::SeqCst);
            Ok(true)
        }))
    })));
    let app =
        crate::http::AuthHttp::new(store, "http://localhost:3000".into(), vec![7; 32], vec![])
            .unwrap()
            .with_clock(move || {
                DateTime::from_timestamp(sampled.load(Ordering::SeqCst), 0).unwrap()
            })
            .with_rate_limiting(false)
            .with_allow_user_to_create_organization(hook);
    let cookie =
        (crate::cookies::SessionCookie { secure: false }).serialize(&f.token, &[7; 32], true);
    let response = app
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/organization/create")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::COOKIE, cookie.split(';').next().unwrap())
                .body(Body::from(body(&f).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let output: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(
        output["createdAt"],
        timestamp(f.now + chrono::Duration::seconds(5))
    );
    assert_eq!(output["members"][0]["createdAt"], output["createdAt"]);
}
