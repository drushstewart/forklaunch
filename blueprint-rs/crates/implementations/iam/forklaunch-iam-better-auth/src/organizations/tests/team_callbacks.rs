mod edges;
mod query_profile;
use super::invitation_policy::{Fixture, request};
use super::team_policy::{records, setup, team};
use super::*;
use crate::organizations::{
    AddMemberInput, MemberRoles, TeamCountLimitContext, TeamCountLimitPolicy, TeamLimitFuture,
    TeamMemberLimitContext, TeamMemberLimitPolicy, TeamPolicy,
};
use std::sync::atomic::{AtomicUsize, Ordering};
struct Count(Box<dyn for<'a> Fn(TeamCountLimitContext<'a>) -> TeamLimitFuture<'a> + Send + Sync>);
impl TeamCountLimitPolicy for Count {
    fn evaluate<'a>(&'a self, c: TeamCountLimitContext<'a>) -> TeamLimitFuture<'a> {
        (self.0)(c)
    }
}
struct Seat(Box<dyn for<'a> Fn(TeamMemberLimitContext<'a>) -> TeamLimitFuture<'a> + Send + Sync>);
impl TeamMemberLimitPolicy for Seat {
    fn evaluate<'a>(&'a self, c: TeamMemberLimitContext<'a>) -> TeamLimitFuture<'a> {
        (self.0)(c)
    }
}
async fn prepare<S: AuthStore>(store: &S, label: &str, kind: u8) -> (Fixture, String) {
    let f = setup(store, label).await;
    let id = format!("{}-team", f.org);
    team(store, &f, &id, 0).await;
    let mut tx = store.begin(true).await.unwrap();
    if kind == 0 {
        tx.insert(Model::Member,json!({"id":format!("{}-target-member",f.org),"userId":f.recipient,"organizationId":f.org,"role":"member","createdAt":timestamp(f.now)})).await.unwrap();
    }
    if kind == 2 {
        tx.insert(Model::Invitation,json!({"id":format!("{}-invite",f.org),"email":format!("{}@example.test",f.recipient),"role":"member","organizationId":f.org,"inviterId":f.owner,"teamId":id,"status":"pending","expiresAt":timestamp(f.now+chrono::Duration::hours(1)),"createdAt":timestamp(f.now)})).await.unwrap();
    }
    tx.commit().await.unwrap();
    (f, id)
}
async fn operation<S: AuthStore>(
    svc: &OrganizationService<S>,
    f: &Fixture,
    id: &str,
    kind: u8,
) -> BlueprintResult<Value> {
    match kind {
        0 => {
            svc.add_team_member(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"teamId":id,"userId":f.recipient}),
                f.now,
            )
            .await
        }
        1 => {
            let mut body = request(f);
            body["teamId"] = json!(id);
            svc.create_invitation(&f.owner_token, false, &body, f.now)
                .await
        }
        2 => svc
            .accept_invitation(
                &f.recipient_token,
                false,
                &format!("{}-invite", f.org),
                f.now,
            )
            .await
            .map(|v| v.body),
        _ => {
            svc.add_member(
                Some(&f.owner_token),
                false,
                AddMemberInput {
                    user_id: f.recipient.clone(),
                    organization_id: Some(f.org.clone()),
                    team_id: Some(id.to_owned()),
                    role: MemberRoles::Role("member".into()),
                },
                f.now,
            )
            .await
        }
    }
}
pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    for kind in 0..4 {
        for (i, limit) in [
            0.0,
            -1.0,
            0.5,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            4_294_967_296.0,
        ]
        .into_iter()
        .enumerate()
        {
            let (f, id) = prepare(store.as_ref(), &format!("callback-ieee-{kind}-{i}"), kind).await;
            let calls = Arc::new(AtomicUsize::new(0));
            let c = calls.clone();
            let svc = OrganizationService::new(store.clone()).with_team_member_limit_policy(
                Arc::new(Seat(Box::new(move |ctx| {
                    c.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(
                        ctx.session().user()["id"],
                        ctx.session().session()["userId"]
                    );
                    assert!(ctx.session().session()["token"].is_string());
                    Box::pin(async move { Ok(limit) })
                }))),
            );
            let out = operation(&svc, &f, &id, kind).await;
            let denied = if kind == 1 {
                0.0 >= limit
            } else {
                0.0_f64.partial_cmp(&limit) != Some(std::cmp::Ordering::Less)
            };
            assert_eq!(out.is_err(), denied, "{kind}/{limit}: {out:?}");
            if denied {
                assert_eq!(out.unwrap_err().code, "TEAM_MEMBER_LIMIT_REACHED");
            }
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }
    count_cases(store.clone()).await;
    mutation_cases(store.clone()).await;
    visibility_and_defaults(store.clone()).await;
    edges::exercise(store.clone()).await;
    query_profile::exercise(store).await;
}
async fn count_cases<S: AuthStore + 'static>(store: Arc<S>) {
    for (i, limit) in [
        0.0,
        -1.0,
        0.5,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        4_294_967_296.0,
    ]
    .into_iter()
    .enumerate()
    {
        let (f, _) = prepare(store.as_ref(), &format!("callback-count-{i}"), 3).await;
        let svc = OrganizationService::new(store.clone()).with_team_count_limit_policy(Arc::new(
            Count(Box::new(move |ctx| {
                assert!(ctx.session().is_some());
                Box::pin(async move { Ok(limit) })
            })),
        ));
        let out = svc
            .create_team(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"name":"Next"}),
                f.now,
            )
            .await;
        assert_eq!(
            out.is_err(),
            limit != 0.0 && !limit.is_nan() && 1.0 >= limit
        );
    }
    for clear in [false, true] {
        let (f, _) = prepare(
            store.as_ref(),
            &format!("callback-count-builder-{clear}"),
            3,
        )
        .await;
        let hook = Arc::new(Count(Box::new(|_| Box::pin(async { Ok(0.0) }))));
        let scalar = TeamPolicy::new(true, Some(1), Some(0), false);
        let svc = if clear {
            OrganizationService::new(store.clone())
                .with_team_count_limit_policy(hook)
                .with_team_policy(scalar)
        } else {
            OrganizationService::new(store.clone())
                .with_team_policy(scalar)
                .with_team_count_limit_policy(hook)
        };
        assert_eq!(
            svc.create_team(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"name":"Next"}),
                f.now
            )
            .await
            .is_err(),
            clear
        );
    }
}
async fn mutation_cases<S: AuthStore + 'static>(store: Arc<S>) {
    for kind in 0..4 {
        for mode in 0..6 {
            let (f, id) = prepare(
                store.as_ref(),
                &format!("callback-mutation-{kind}-{mode}"),
                kind,
            )
            .await;
            let hook = Seat(Box::new(move |mut ctx| {
                Box::pin(async move {
                    let team = ctx.team_id().to_owned();
                    let org = ctx.organization_id().to_owned();
                    let session = ctx.session().session()["id"].as_str().unwrap().to_owned();
                    let adapter = ctx.adapter();
                    match mode {
    0=>adapter.insert(Model::TeamMember,json!({"id":format!("{org}-callback-pair"),"teamId":team,"userId":"other","createdAt":"2027-01-15T08:00:00.000Z"})).await?,
    1=>adapter.delete(Model::Team,&team).await?,
    2=>adapter.delete(Model::Session,&session).await?,
    3=>{let page=adapter.find_page(Model::Member,"organizationId",&org,None,100).await?;for row in page.records {adapter.delete(Model::Member,row["id"].as_str().unwrap()).await?;}},
    4=>return Err(BlueprintError::new("SECRET","must not escape")),
    _=>{if let Some(mut row)=adapter.get(Model::Invitation,&format!("{org}-invite")).await?{row["status"]=json!("canceled");adapter.replace(Model::Invitation,row).await?;}},
   }
                    Ok(1.0)
                })
            }));
            let svc = OrganizationService::new(store.clone())
                .with_team_member_limit_policy(Arc::new(hook));
            let out = operation(&svc, &f, &id, kind).await;
            let denied = mode == 4 || kind != 1 && mode <= 1 || kind == 2 && mode == 2;
            assert_eq!(out.is_err(), denied, "{kind}/{mode}: {out:?}");
            if let Err(e) = out {
                assert_eq!(
                    e.code,
                    if mode == 4 || kind == 2 && mode == 2 {
                        "HTTP_INTERNAL_EMPTY"
                    } else {
                        "TEAM_MEMBER_LIMIT_REACHED"
                    }
                );
            }
            if kind == 2 {
                let rows =
                    records(store.as_ref(), Model::Invitation, "organizationId", &f.org).await;
                assert_eq!(
                    rows[0]["status"],
                    if denied {
                        "pending"
                    } else if mode == 5 {
                        "canceled"
                    } else {
                        "accepted"
                    }
                );
                if denied {
                    assert!(
                        records(store.as_ref(), Model::TeamMember, "teamId", &id)
                            .await
                            .is_empty()
                    );
                    let mut tx = store.begin(false).await.unwrap();
                    assert!(tx.get(Model::Team, &id).await.unwrap().is_some());
                    assert!(
                        tx.find_unique(Model::Session, "token", &f.recipient_token)
                            .await
                            .unwrap()
                            .is_some()
                    );
                    tx.commit().await.unwrap();
                }
            }
        }
    }
}
async fn visibility_and_defaults<S: AuthStore + 'static>(store: Arc<S>) {
    for kind in [2, 3] {
        let (f, id) = prepare(store.as_ref(), &format!("callback-visibility-{kind}"), kind).await;
        let svc = OrganizationService::new(store.clone()).with_team_member_limit_policy(Arc::new(
            Seat(Box::new(move |mut ctx| {
                Box::pin(async move {
                    let org = ctx.organization_id().to_owned();
                    let adapter = ctx.adapter();
                    let members = adapter
                        .find_page(Model::Member, "organizationId", &org, None, 100)
                        .await?
                        .records;
                    assert_eq!(members.len(), if kind == 3 { 2 } else { 1 });
                    if kind == 2 {
                        assert_eq!(
                            adapter
                                .get(Model::Invitation, &format!("{org}-invite"))
                                .await?
                                .unwrap()["status"],
                            "accepted"
                        );
                    }
                    Ok(10.0)
                })
            })),
        ));
        operation(&svc, &f, &id, kind).await.unwrap();
    }
    let f = setup(store.as_ref(), "callback-default-bypass").await;
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let d = calls.clone();
    let svc = OrganizationService::new(store.clone())
        .with_team_count_limit_policy(Arc::new(Count(Box::new(move |ctx| {
            c.fetch_add(1, Ordering::SeqCst);
            assert!(ctx.session().is_none());
            Box::pin(async { Ok(0.0) })
        }))))
        .with_team_member_limit_policy(Arc::new(Seat(Box::new(move |_| {
            d.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(0.0) })
        }))));
    svc.create(
        &f.owner_token,
        false,
        &json!({"name":"Default","slug":format!("{}-default",f.org)}),
        f.now,
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        svc.create_team_for_organization("", "Empty scope", f.now)
            .await
            .unwrap_err()
            .code,
        "NO_ACTIVE_ORGANIZATION"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    svc.create_team_for_organization(&f.org, "Trusted", f.now)
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn team_callback_caller_order_ieee_current_adapter_and_rollback() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}
