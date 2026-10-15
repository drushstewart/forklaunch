use super::*;
use crate::organizations::TeamSeatQueryProfile;

pub(super) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    for kind in 0..6 {
        for (i, limit) in [
            0.0,
            -1.0,
            0.5,
            1.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            2_147_483_647.0,
            4_294_967_296.0,
        ]
        .into_iter()
        .enumerate()
        {
            let (f, id) = prepare(
                store.as_ref(),
                &format!("query-profile-{kind}-{i}"),
                if kind >= 4 { 0 } else { kind },
            )
            .await;
            if kind == 4 {
                let mut tx = store.begin(true).await.unwrap();
                tx.insert(Model::TeamMember,json!({"id":format!("{id}-existing"),"teamId":id,"userId":f.recipient,"createdAt":timestamp(f.now)})).await.unwrap();
                tx.commit().await.unwrap();
            }
            let calls = Arc::new(AtomicUsize::new(0));
            let seen = calls.clone();
            let hook = Arc::new(Seat(Box::new(move |mut ctx| {
                seen.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    if kind == 5 {
                        let team = ctx.team_id().to_owned();
                        ctx.adapter().delete(Model::Team, &team).await?;
                    }
                    Ok(limit)
                })
            })));
            let svc = OrganizationService::new(store.clone());
            let svc = if i % 2 == 0 {
                svc.with_team_seat_query_profile(TeamSeatQueryProfile::MikroOrmPostgres)
                    .with_team_policy(TeamPolicy::default())
                    .with_team_member_limit_policy(hook)
            } else {
                svc.with_team_member_limit_policy(hook)
                    .with_team_seat_query_profile(TeamSeatQueryProfile::MikroOrmPostgres)
            };
            let out = operation(&svc, &f, &id, if kind >= 4 { 0 } else { kind }).await;
            let error = expected_error(kind, limit);
            assert_eq!(
                out.as_ref().err().map(|e| e.code.as_str()),
                error,
                "{kind}/{limit}"
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            let pairs = records(store.as_ref(), Model::TeamMember, "teamId", &id).await;
            assert_eq!(
                pairs.len(),
                usize::from(kind == 4 || (kind != 1 && error.is_none()))
            );
            let members = records(store.as_ref(), Model::Member, "organizationId", &f.org).await;
            assert_eq!(
                members.iter().any(|m| m["userId"] == f.recipient),
                kind == 0 || kind >= 4 || (kind != 1 && error.is_none())
            );
            if kind == 2 {
                let rows =
                    records(store.as_ref(), Model::Invitation, "organizationId", &f.org).await;
                assert_eq!(
                    rows[0]["status"],
                    if error.is_none() {
                        "accepted"
                    } else {
                        "pending"
                    }
                );
            }
        }
    }
    #[cfg(feature = "http")]
    http(store).await;
}
fn expected_error(kind: u8, limit: f64) -> Option<&'static str> {
    if kind == 4 {
        return None;
    }
    if kind != 1 && !limit.is_finite() {
        return Some("HTTP_INTERNAL_EMPTY");
    }
    if kind == 5 || limit <= 0.0 {
        Some("TEAM_MEMBER_LIMIT_REACHED")
    } else {
        None
    }
}
#[cfg(feature = "http")]
async fn http<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use tower::ServiceExt;
    for before in [false, true] {
        let (f, id) = prepare(store.as_ref(), &format!("query-http-{before}"), 0).await;
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
        let app = if before {
            app.with_team_seat_query_profile(TeamSeatQueryProfile::MikroOrmPostgres)
        } else {
            app
        };
        let app = app
            .with_team_policy(TeamPolicy::default())
            .with_team_member_limit_policy(Arc::new(Seat(Box::new(|_| {
                Box::pin(async { Ok(f64::INFINITY) })
            }))));
        let app = if before {
            app
        } else {
            app.with_team_seat_query_profile(TeamSeatQueryProfile::MikroOrmPostgres)
        };
        let cookie = (crate::cookies::SessionCookie { secure: false }).serialize(
            &f.owner_token,
            &[7; 32],
            true,
        );
        let response = app
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/organization/add-team-member")
                    .header(header::COOKIE, cookie.split(';').next().unwrap())
                    .header(header::ORIGIN, "http://localhost:3000")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"organizationId":f.org,"teamId":id,"userId":f.recipient})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 500);
        assert!(
            to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
