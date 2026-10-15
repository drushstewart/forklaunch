use super::*;
pub(super) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    count_races(store.clone()).await;
    for kind in [0, 2] {
        let (f, id) = prepare(store.as_ref(), &format!("callback-duplicate-{kind}"), kind).await;
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(Model::TeamMember,json!({"id":format!("{}-existing-pair",f.org),"teamId":id,"userId":f.recipient,"createdAt":timestamp(f.now)})).await.unwrap();
        tx.commit().await.unwrap();
        let svc = OrganizationService::new(store.clone()).with_team_member_limit_policy(Arc::new(
            Seat(Box::new(|_| Box::pin(async { Ok(f64::NAN) }))),
        ));
        operation(&svc, &f, &id, kind).await.unwrap();
        assert_eq!(
            records(store.as_ref(), Model::TeamMember, "teamId", &id)
                .await
                .len(),
            1
        );
    }
    let (f, id) = prepare(store.as_ref(), "callback-server-no-session", 3).await;
    let svc =
        OrganizationService::new(store.clone()).with_team_member_limit_policy(Arc::new(Seat(
            Box::new(|_| panic!("missing session must not invoke callback")),
        )));
    let error = svc
        .add_member(
            None,
            false,
            AddMemberInput {
                user_id: f.recipient.clone(),
                organization_id: Some(f.org.clone()),
                team_id: Some(id),
                role: MemberRoles::Role("member".into()),
            },
            f.now,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "UNAUTHORIZED");
    assert_eq!(
        records(store.as_ref(), Model::Member, "organizationId", &f.org)
            .await
            .len(),
        1
    );
    cancellation(store.clone()).await;
    #[cfg(feature = "http")]
    http(store).await;
}
async fn count_races<S: AuthStore + 'static>(store: Arc<S>) {
    for mode in 0..4 {
        let f = setup(store.as_ref(), &format!("callback-count-race-{mode}")).await;
        if mode == 1 {
            team(store.as_ref(), &f, &format!("{}-before", f.org), 0).await;
        }
        let svc=OrganizationService::new(store.clone()).with_team_count_limit_policy(Arc::new(Count(Box::new(move|mut ctx|Box::pin(async move {
 let org=ctx.organization_id().to_owned();let session=ctx.session().unwrap().session()["id"].as_str().unwrap().to_owned();let adapter=ctx.adapter();
 match mode {0=>adapter.insert(Model::Team,json!({"id":format!("{org}-callback"),"organizationId":org,"name":"Callback","memberCount":0,"createdAt":"2027-01-15T08:00:00.000Z"})).await?,1=>adapter.delete(Model::Team,&format!("{org}-before")).await?,2=>adapter.delete(Model::Session,&session).await?,_=>adapter.delete(Model::Organization,&org).await?}Ok(1.0)
 })))));
        let out = svc
            .create_team(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"name":"Next"}),
                f.now,
            )
            .await;
        assert_eq!(out.is_err(), mode == 1 || mode == 3);
        if mode == 1 {
            assert_eq!(
                out.unwrap_err().code,
                "YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_TEAMS"
            );
        } else if mode == 3 {
            assert_eq!(out.unwrap_err().code, "ORGANIZATION_NOT_FOUND");
        }
    }
}
async fn cancellation<S: AuthStore + 'static>(store: Arc<S>) {
    let (f, id) = prepare(store.as_ref(), "callback-accept-cancel", 2).await;
    let ready = Arc::new(tokio::sync::Notify::new());
    let signal = ready.clone();
    let svc = OrganizationService::new(store.clone()).with_team_member_limit_policy(Arc::new(
        Seat(Box::new(move |mut ctx| {
            let signal = signal.clone();
            Box::pin(async move {
                let org = ctx.organization_id().to_owned();
                let adapter = ctx.adapter();
                let mut invitation = adapter
                    .get(Model::Invitation, &format!("{org}-invite"))
                    .await?
                    .unwrap();
                assert_eq!(invitation["status"], "accepted");
                invitation["status"] = json!("canceled");
                adapter.replace(Model::Invitation, invitation).await?;
                signal.notify_one();
                std::future::pending().await
            })
        })),
    ));
    {
        let out = operation(&svc, &f, &id, 2);
        tokio::pin!(out);
        tokio::select! {()=ready.notified()=>{},_=&mut out=>panic!("callback must remain pending")}
    }
    let invitations = records(store.as_ref(), Model::Invitation, "organizationId", &f.org).await;
    assert_eq!(invitations[0]["status"], "pending");
    assert!(
        records(store.as_ref(), Model::TeamMember, "teamId", &id)
            .await
            .is_empty()
    );
}
#[cfg(feature = "http")]
async fn http<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use tower::ServiceExt;
    for (i, kind, clear) in [(0, 0, false), (1, 1, false), (2, 1, true), (3, 2, false)] {
        let (f, id) = prepare(store.as_ref(), &format!("callback-http-{i}"), kind).await;
        let now = f.now;
        let app = crate::http::AuthHttp::new(
            store.clone(),
            "http://localhost:3000".into(),
            vec![7; 32],
            vec![],
        )
        .unwrap()
        .with_rate_limiting(false)
        .with_clock(move || now)
        .with_team_member_limit_policy(Arc::new(Seat(Box::new(|_| {
            Box::pin(async { Ok(f64::NAN) })
        }))));
        let app = if clear {
            app.with_team_policy(TeamPolicy::default())
        } else {
            app
        };
        let (path, body, token) = match kind {
            0 => (
                "/organization/add-team-member",
                json!({"organizationId":f.org,"teamId":id,"userId":f.recipient}),
                &f.owner_token,
            ),
            1 => {
                let mut body = request(&f);
                body["teamId"] = json!(id);
                ("/organization/invite-member", body, &f.owner_token)
            }
            _ => (
                "/organization/accept-invitation",
                json!({"invitationId":format!("{}-invite",f.org)}),
                &f.recipient_token,
            ),
        };
        let cookie =
            (crate::cookies::SessionCookie { secure: false }).serialize(token, &[7; 32], true);
        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://localhost:3000")
            .header(header::COOKIE, cookie.split(';').next().unwrap())
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.router().oneshot(req).await.unwrap();
        assert_eq!(
            response.status().as_u16(),
            if kind == 1 { 200 } else { 403 }
        );
        let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
        if kind != 1 {
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap()["code"],
                "TEAM_MEMBER_LIMIT_REACHED"
            );
        }
    }
}
