use super::*;
use crate::organizations::{
    AddMemberInput, MemberRoles, MembershipLimitContext, MembershipLimitFuture,
    MembershipLimitPolicy, OrganizationPolicy, TeamPolicy,
};
use std::sync::Mutex;
struct Hook(
    Box<dyn Fn(MembershipLimitContext) -> BlueprintResult<MembershipLimitFuture> + Send + Sync>,
);
impl MembershipLimitPolicy for Hook {
    fn prepare(&self, ctx: MembershipLimitContext) -> BlueprintResult<MembershipLimitFuture> {
        (self.0)(ctx)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    Add,
    Accept,
}
struct Fixture {
    user: String,
    org: String,
    invitation: String,
    team: String,
    token: String,
    session: String,
    now: DateTime<Utc>,
}
async fn seed<S: AuthStore>(store: &S, label: &str, count: usize, team: bool) -> Fixture {
    let prefix = format!("membership-policy-{label}");
    let user = format!("{prefix}-user");
    let org = format!("{prefix}-org");
    let invitation = format!("{prefix}-invite");
    let team_id = format!("{prefix}-team");
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User,json!({"id":user,"email":format!("{user}@example.test"),"name":"Original","emailVerified":true,"privateField":"secret"})).await.unwrap();
    tx.insert(
        Model::Organization,
        json!({"id":org,"name":"Organization","slug":prefix,"privateField":"secret"}),
    )
    .await
    .unwrap();
    for n in 0..count {
        tx.insert(Model::Member,json!({"id":format!("{prefix}-member-{n}"),"userId":"other-user","organizationId":org,"role":"member"})).await.unwrap();
    }
    let mut invite = json!({"id":invitation,"email":format!("{user}@example.test"),"organizationId":org,"role":"member","status":"pending","expiresAt":timestamp(now+chrono::Duration::hours(1))});
    if team {
        tx.insert(
            Model::Team,
            json!({"id":team_id,"organizationId":org,"name":"Team","memberCount":0}),
        )
        .await
        .unwrap();
        invite["teamId"] = json!(team_id);
    }
    tx.insert(Model::Invitation, invite).await.unwrap();
    let session = crate::sessions::new_session(&user, "", "", now).unwrap();
    let token = session["token"].as_str().unwrap().to_owned();
    let sid = session["id"].as_str().unwrap().to_owned();
    tx.insert(Model::Session, session).await.unwrap();
    tx.commit().await.unwrap();
    Fixture {
        user,
        org,
        invitation,
        team: team_id,
        token,
        session: sid,
        now,
    }
}
fn constant(value: f64) -> Arc<dyn MembershipLimitPolicy> {
    Arc::new(Hook(Box::new(move |ctx| {
        assert!(ctx.user().get("privateField").is_none());
        assert!(ctx.organization().get("privateField").is_none());
        Ok(Box::pin(async move { Ok(value) }))
    })))
}
async fn run<S: AuthStore>(
    svc: &OrganizationService<S>,
    f: &Fixture,
    flow: Flow,
    team: bool,
) -> BlueprintResult<Value> {
    match flow {
        Flow::Add => {
            svc.add_member(
                None,
                false,
                AddMemberInput {
                    user_id: f.user.clone(),
                    role: MemberRoles::Role("member".into()),
                    organization_id: Some(f.org.clone()),
                    team_id: team.then(|| f.team.clone()),
                },
                f.now,
            )
            .await
        }
        Flow::Accept => svc
            .accept_invitation(&f.token, false, &f.invitation, f.now)
            .await
            .map(|out| out.body),
    }
}
#[allow(clippy::too_many_lines)]
pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    metadata_shapes(store.clone()).await;
    for flow in [Flow::Add, Flow::Accept] {
        for count in [0, 1] {
            for (i, limit) in [
                0.0,
                -0.0,
                0.5,
                1.0,
                -0.5,
                f64::NAN,
                f64::INFINITY,
                f64::NEG_INFINITY,
                4_294_967_296.0,
                f64::MAX,
            ]
            .into_iter()
            .enumerate()
            {
                let f = seed(
                    store.as_ref(),
                    &format!("numeric-{flow:?}-{count}-{i}"),
                    count,
                    false,
                )
                .await;
                let svc = OrganizationService::new(store.clone())
                    .with_organization_policy(OrganizationPolicy::new(true, None, 1))
                    .with_membership_limit_policy(constant(limit));
                let out = run(&svc, &f, flow, false).await;
                if f64::from(u32::try_from(count).unwrap()) >= limit {
                    assert_eq!(
                        out.unwrap_err().code,
                        "ORGANIZATION_MEMBERSHIP_LIMIT_REACHED"
                    );
                } else {
                    assert!(out.is_ok());
                }
            }
        }
    }
    for flow in [Flow::Add, Flow::Accept] {
        for mutation in [
            "count",
            "duplicate",
            "delete-user",
            "delete-org",
            "revoke",
            "cancel-invite",
            "change-invite",
        ] {
            let f = seed(
                store.as_ref(),
                &format!("race-{flow:?}-{mutation}"),
                0,
                false,
            )
            .await;
            let (user, org, invite, session) = (
                f.user.clone(),
                f.org.clone(),
                f.invitation.clone(),
                f.session.clone(),
            );
            let inner = store.clone();
            let hook = Arc::new(Hook(Box::new(move |ctx| {
                assert_eq!(ctx.user()["id"], user);
                assert_eq!(ctx.organization()["id"], org);
                let (user, org, invite, session, store) = (
                    user.clone(),
                    org.clone(),
                    invite.clone(),
                    session.clone(),
                    inner.clone(),
                );
                Ok(Box::pin(async move {
                    let mut tx = store.begin(true).await?;
                    match mutation {
                        "count" | "duplicate" => {
                            tx.insert(Model::Member,json!({"id":format!("{invite}-member"),"userId":if mutation=="count"{"other"}else{&user},"organizationId":org,"role":"member"})).await?;
                        }
                        "delete-user" => {
                            tx.delete(Model::User, &user).await?;
                        }
                        "delete-org" => {
                            tx.delete(Model::Organization, &org).await?;
                        }
                        "revoke" => {
                            tx.delete(Model::Session, &session).await?;
                        }
                        _ => {
                            let mut row = tx.get(Model::Invitation, &invite).await?.unwrap();
                            if mutation == "cancel-invite" {
                                row["status"] = json!("canceled");
                            } else {
                                row["role"] = json!("admin");
                                row["email"] = json!("someone@example.test");
                            }
                            tx.replace(Model::Invitation, row).await?;
                        }
                    }
                    tx.commit().await?;
                    Ok(1.0)
                }))
            })));
            let svc = OrganizationService::new(store.clone()).with_membership_limit_policy(hook);
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                run(&svc, &f, flow, false),
            )
            .await
            .unwrap();
            if flow == Flow::Accept && mutation == "cancel-invite" {
                assert_eq!(out.unwrap_err().code, "INVITATION_NOT_FOUND");
            } else {
                let out = out.unwrap();
                if flow == Flow::Accept && mutation == "change-invite" {
                    assert_eq!(out["member"]["role"], "admin");
                }
            }
        }
    }
    team_races(store.clone()).await;
    errors_and_hydration(store.clone()).await;
    #[cfg(feature = "http")]
    http(store).await;
}
async fn team_races<S: AuthStore + 'static>(store: Arc<S>) {
    for flow in [Flow::Add, Flow::Accept] {
        for bounded in [false, true] {
            for mutation in [
                "revoke",
                "token",
                "session-user",
                "team-delete",
                "team-org",
                "invite-org",
            ] {
                let f = seed(
                    store.as_ref(),
                    &format!("team-{flow:?}-{bounded}-{mutation}"),
                    0,
                    true,
                )
                .await;
                let (session, team, invite) =
                    (f.session.clone(), f.team.clone(), f.invitation.clone());
                let inner = store.clone();
                let hook = Arc::new(Hook(Box::new(move |_| {
                    let (session, team, invite, store) =
                        (session.clone(), team.clone(), invite.clone(), inner.clone());
                    Ok(Box::pin(async move {
                        let mut tx = store.begin(true).await?;
                        match mutation {
                            "revoke" => {
                                tx.delete(Model::Session, &session).await?;
                            }
                            "token" | "session-user" => {
                                let mut row = tx.get(Model::Session, &session).await?.unwrap();
                                row[if mutation == "token" {
                                    "token"
                                } else {
                                    "userId"
                                }] = json!(format!("changed-{session}"));
                                tx.replace(Model::Session, row).await?;
                            }
                            "team-delete" => {
                                tx.delete(Model::Team, &team).await?;
                            }
                            _ => {
                                let mut row = tx.get(Model::Team, &team).await?.unwrap();
                                row["organizationId"] = json!("elsewhere");
                                tx.replace(Model::Team, row).await?;
                                if mutation == "invite-org" {
                                    let mut row =
                                        tx.get(Model::Invitation, &invite).await?.unwrap();
                                    row["organizationId"] = json!("elsewhere");
                                    tx.replace(Model::Invitation, row).await?;
                                }
                            }
                        }
                        tx.commit().await?;
                        Ok(f64::INFINITY)
                    }))
                })));
                let svc = OrganizationService::new(store.clone())
                    .with_team_policy(TeamPolicy::new(true, None, bounded.then_some(2), false))
                    .with_membership_limit_policy(hook);
                let out = run(&svc, &f, flow, true).await;
                let expected = if flow == Flow::Accept && ["revoke", "token"].contains(&mutation) {
                    Some("HTTP_INTERNAL_EMPTY")
                } else if flow == Flow::Accept && ["team-delete", "team-org"].contains(&mutation) {
                    Some("TEAM_NOT_FOUND")
                } else if flow == Flow::Add && bounded && mutation == "team-delete" {
                    Some("TEAM_MEMBER_LIMIT_REACHED")
                } else {
                    None
                };
                if let Some(code) = expected {
                    assert_eq!(out.unwrap_err().code, code);
                } else {
                    assert!(out.is_ok());
                }
                let mut tx = store.begin(false).await.unwrap();
                assert_eq!(
                    tx.get(Model::Invitation, &f.invitation)
                        .await
                        .unwrap()
                        .unwrap()["status"],
                    if flow == Flow::Accept && expected.is_none() {
                        "accepted"
                    } else {
                        "pending"
                    }
                );
                tx.commit().await.unwrap();
            }
        }
    }
}
#[tokio::test]
async fn awaited_membership_quota_ieee_order_races_and_current_invitation() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}
use super::creation_policy::fault_store;
#[allow(clippy::too_many_lines)]
async fn errors_and_hydration<S: AuthStore + 'static>(store: Arc<S>) {
    for flow in [Flow::Add, Flow::Accept] {
        for mode in [1, 2] {
            let f = seed(store.as_ref(), &format!("error-{flow:?}-{mode}"), 0, false).await;
            let hook = Arc::new(Hook(Box::new(move |_| {
                if mode == 1 {
                    return Err(BlueprintError::new("secret", "private"));
                }
                Ok(Box::pin(async {
                    Err(BlueprintError::new("secret", "private"))
                }))
            })));
            let svc = OrganizationService::new(store.clone()).with_membership_limit_policy(hook);
            assert_eq!(
                run(&svc, &f, flow, false).await.unwrap_err().code,
                "HTTP_INTERNAL_EMPTY"
            );
            let mut tx = store.begin(false).await.unwrap();
            assert!(
                crate::store::find_all(&mut tx, Model::Member, "organizationId", &f.org)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                tx.get(Model::Invitation, &f.invitation)
                    .await
                    .unwrap()
                    .unwrap()["status"],
                "pending"
            );
            tx.commit().await.unwrap();
        }
    }
    for flow in [Flow::Add, Flow::Accept] {
        let f = seed(store.as_ref(), &format!("cancel-{flow:?}"), 0, false).await;
        let ready = Arc::new(tokio::sync::Notify::new());
        let signal = ready.clone();
        let inner = store.clone();
        let hook = Arc::new(Hook(Box::new(move |_| {
            let signal = signal.clone();
            let store = inner.clone();
            Ok(Box::pin(async move {
                let tx = store.begin(true).await?;
                tx.commit().await?;
                signal.notify_one();
                std::future::pending().await
            }))
        })));
        let svc = OrganizationService::new(store.clone()).with_membership_limit_policy(hook);
        {
            let request = run(&svc, &f, flow, false);
            tokio::pin!(request);
            tokio::select! {()=ready.notified()=>{},_=&mut request=>panic!("pending quota must await")};
        }
        let mut tx = store.begin(true).await.unwrap();
        assert!(
            crate::store::find_all(&mut tx, Model::Member, "organizationId", &f.org)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            tx.get(Model::Invitation, &f.invitation)
                .await
                .unwrap()
                .unwrap()["status"],
            "pending"
        );
        tx.commit().await.unwrap();
    }
    for flow in [Flow::Add, Flow::Accept] {
        for acknowledge in [false, true] {
            let f = seed(
                store.as_ref(),
                &format!("commit-{flow:?}-{acknowledge}"),
                0,
                false,
            )
            .await;
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
            let svc = OrganizationService::new(fault).with_membership_limit_policy(constant(1.0));
            assert_eq!(
                run(&svc, &f, flow, false).await.unwrap_err().code,
                "better_auth_store_failure"
            );
            let mut tx = store.begin(false).await.unwrap();
            assert_eq!(
                crate::store::find_all(&mut tx, Model::Member, "organizationId", &f.org)
                    .await
                    .unwrap()
                    .len(),
                usize::from(acknowledge)
            );
            assert_eq!(
                tx.get(Model::Invitation, &f.invitation)
                    .await
                    .unwrap()
                    .unwrap()["status"],
                if acknowledge && flow == Flow::Accept {
                    "accepted"
                } else {
                    "pending"
                }
            );
            tx.commit().await.unwrap();
        }
    }
    for reverse in [false, true] {
        let f = seed(store.as_ref(), &format!("builder-{reverse}"), 0, false).await;
        let svc = OrganizationService::new(store.clone());
        let policy = OrganizationPolicy::new(true, None, 1);
        let svc = if reverse {
            svc.with_membership_limit_policy(constant(0.0))
                .with_organization_policy(policy)
        } else {
            svc.with_organization_policy(policy)
                .with_membership_limit_policy(constant(0.0))
        };
        assert_eq!(run(&svc, &f, Flow::Add, false).await.is_ok(), reverse);
    }
    for present in [false, true] {
        let f = seed(store.as_ref(), &format!("projection-{present}"), 0, false).await;
        let mut tx = store.begin(true).await.unwrap();
        let mut org = tx.get(Model::Organization, &f.org).await.unwrap().unwrap();
        org["name"] = json!({"nested":"bad"});
        tx.replace(Model::Organization, org).await.unwrap();
        tx.commit().await.unwrap();
        let svc = OrganizationService::new(store.clone());
        let svc = if present {
            svc.with_membership_limit_policy(constant(1.0))
        } else {
            svc
        };
        assert_eq!(run(&svc, &f, Flow::Add, false).await.is_ok(), !present);
    }
    let f = seed(store.as_ref(), "full-hydration", 0, false).await;
    let mut tx = store.begin(true).await.unwrap();
    let extra = format!("{}-extra", f.user);
    tx.insert(
        Model::User,
        json!({"id":extra,"email":format!("{extra}@example.test"),"name":"Extra"}),
    )
    .await
    .unwrap();
    for (user, n) in [(&f.user, 0), (&extra, 1)] {
        tx.insert(Model::Member,json!({"id":format!("{}-m{n}",f.org),"userId":user,"organizationId":f.org,"role":"member"})).await.unwrap();
    }
    tx.commit().await.unwrap();
    let calls = Arc::new(Mutex::new(0));
    let counted = calls.clone();
    let hook = Arc::new(Hook(Box::new(move |_| {
        *counted.lock().unwrap() += 1;
        Ok(Box::pin(async { Ok(0.0) }))
    })));
    let svc = OrganizationService::new(store)
        .with_organization_policy(OrganizationPolicy::new(true, None, 1));
    assert!(
        svc.full(&f.token, false, Some(&f.org), None, None, f.now)
            .await
            .is_err()
    );
    let out = svc
        .with_membership_limit_policy(hook)
        .full(&f.token, false, Some(&f.org), None, None, f.now)
        .await
        .unwrap();
    assert_eq!(out["members"].as_array().unwrap().len(), 2);
    assert_eq!(*calls.lock().unwrap(), 0);
}
#[cfg(feature = "http")]
async fn http<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use tower::ServiceExt;
    for mode in [0, 1, 2, 3] {
        let f = seed(store.as_ref(), &format!("http-{mode}"), 0, false).await;
        let now = f.now;
        let hook = Arc::new(Hook(Box::new(move |_| {
            if mode == 1 {
                return Err(BlueprintError::new("secret", "private"));
            }
            Ok(Box::pin(async move {
                if mode == 2 {
                    Err(BlueprintError::new("secret", "private"))
                } else {
                    Ok(0.0)
                }
            }))
        })));
        let app = crate::http::AuthHttp::new(
            store.clone(),
            "http://localhost:3000".into(),
            vec![7; 32],
            vec![],
        )
        .unwrap()
        .with_clock(move || now)
        .with_rate_limiting(false)
        .with_membership_limit_policy(hook);
        let app = if mode == 3 {
            app.with_organization_policy(OrganizationPolicy::new(true, None, 1))
        } else {
            app
        };
        let cookie =
            (crate::cookies::SessionCookie { secure: false }).serialize(&f.token, &[7; 32], true);
        let out = app
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/organization/accept-invitation")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ORIGIN, "http://localhost:3000")
                    .header(header::COOKIE, cookie.split(';').next().unwrap())
                    .body(Body::from(json!({"invitationId":f.invitation}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            out.status().as_u16(),
            match mode {
                0 => 403,
                3 => 200,
                _ => 500,
            }
        );
        assert_eq!(
            to_bytes(out.into_body(), 65536).await.unwrap().is_empty(),
            [1, 2].contains(&mode)
        );
    }
}

async fn metadata_shapes<S: AuthStore + 'static>(store: Arc<S>) {
    for flow in [Flow::Add, Flow::Accept] {
        for (index, metadata) in [
            json!({"nested":{"enabled":true}}),
            json!(42),
            json!([true, null]),
            json!("{\"stored\":true}"),
            Value::Null,
        ]
        .into_iter()
        .enumerate()
        {
            let f = seed(
                store.as_ref(),
                &format!("metadata-{flow:?}-{index}"),
                0,
                false,
            )
            .await;
            let mut tx = store.begin(true).await.unwrap();
            let mut org = tx.get(Model::Organization, &f.org).await.unwrap().unwrap();
            org["metadata"] = metadata.clone();
            tx.replace(Model::Organization, org).await.unwrap();
            tx.commit().await.unwrap();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let capture = seen.clone();
            let svc = OrganizationService::new(store.clone()).with_membership_limit_policy(
                Arc::new(Hook(Box::new(move |ctx| {
                    capture
                        .lock()
                        .unwrap()
                        .push(ctx.organization()["metadata"].clone());
                    Ok(Box::pin(async { Ok(100.0) }))
                }))),
            );
            run(&svc, &f, flow, false).await.unwrap();
            assert_eq!(*seen.lock().unwrap(), vec![metadata]);
        }
    }
}
