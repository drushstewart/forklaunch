use super::invitation_policy::{Fixture, request, seed};
use super::*;
use crate::organizations::{AddMemberInput, MemberRoles, TeamPolicy};
pub(super) async fn setup<S: AuthStore>(store: &S, label: &str) -> Fixture {
    let f = seed(store, &format!("team-policy-{label}")).await;
    let mut tx = store.begin(true).await.unwrap();
    let mut grant = tx
        .get(Model::OrganizationRole, &format!("{}-grant", f.org))
        .await
        .unwrap()
        .unwrap();
    grant["permission"] = json!(
        "{\"team\":[\"create\",\"delete\",\"update\"],\"member\":[\"create\",\"update\"],\"invitation\":[\"create\"]}"
    );
    tx.replace(Model::OrganizationRole, grant).await.unwrap();
    let mut user = tx.get(Model::User, &f.recipient).await.unwrap().unwrap();
    user["emailVerified"] = json!(true);
    tx.replace(Model::User, user).await.unwrap();
    tx.commit().await.unwrap();
    f
}
pub(super) async fn team<S: AuthStore>(store: &S, f: &Fixture, id: &str, count: i64) {
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Team,json!({"id":id,"organizationId":f.org,"name":id,"memberCount":count,"createdAt":timestamp(f.now)})).await.unwrap();
    tx.commit().await.unwrap();
}
pub(super) async fn records<S: AuthStore>(
    store: &S,
    model: Model,
    field: &str,
    id: &str,
) -> Vec<Value> {
    let mut tx = store.begin(false).await.unwrap();
    let rows = crate::store::find_all(&mut tx, model, field, id)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    rows
}
#[allow(clippy::too_many_lines)]
pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    for (i, limit, expected) in [
        (0, None, true),
        (1, Some(0), true),
        (2, Some(1), false),
        (3, Some(2), true),
    ] {
        let f = setup(store.as_ref(), &format!("create-{i}")).await;
        let id = format!("{}-team", f.org);
        team(store.as_ref(), &f, &id, 0).await;
        let svc = OrganizationService::new(store.clone())
            .with_team_policy(TeamPolicy::new(true, limit, None, false));
        let out = svc
            .create_team(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"name":"Next"}),
                f.now,
            )
            .await;
        assert_eq!(out.is_ok(), expected);
        if !expected {
            assert_eq!(
                out.unwrap_err().code,
                "YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_TEAMS"
            );
        }
    }
    for enabled in [true, false] {
        let f = setup(store.as_ref(), &format!("default-{enabled}")).await;
        let mut tx = store.begin(true).await.unwrap();
        let mut session = tx
            .find_unique(Model::Session, "token", &f.owner_token)
            .await
            .unwrap()
            .unwrap();
        session["activeTeamId"] = json!("existing-active-team");
        tx.replace(Model::Session, session).await.unwrap();
        tx.commit().await.unwrap();
        let svc = OrganizationService::new(store.clone()).with_team_policy(TeamPolicy::new(
            enabled,
            Some(0),
            Some(0),
            false,
        ));
        let out = svc
            .create(
                &f.owner_token,
                false,
                &json!({"name":"Default","slug":format!("{}-new",f.org)}),
                f.now,
            )
            .await
            .unwrap();
        let id = text(&out, "id").unwrap();
        assert_eq!(
            records(store.as_ref(), Model::Team, "organizationId", id)
                .await
                .len(),
            usize::from(enabled)
        );
        let mut tx = store.begin(false).await.unwrap();
        let session = tx
            .find_unique(Model::Session, "token", &f.owner_token)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session["activeOrganizationId"], id);
        if !enabled {
            assert_eq!(session["activeTeamId"], "existing-active-team");
        }
        tx.commit().await.unwrap();
    }
    for allow in [true, false] {
        let f = setup(store.as_ref(), &format!("remove-{allow}")).await;
        let id = format!("{}-team", f.org);
        team(store.as_ref(), &f, &id, 0).await;
        let svc = OrganizationService::new(store.clone())
            .with_team_policy(TeamPolicy::new(true, None, None, allow));
        let out = svc
            .remove_team(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"teamId":id}),
                f.now,
            )
            .await;
        assert_eq!(out.is_ok(), allow);
        if !allow {
            assert_eq!(out.unwrap_err().code, "UNABLE_TO_REMOVE_LAST_TEAM");
        }
    }
    for limit in [0, 1] {
        let f = setup(store.as_ref(), &format!("add-{limit}")).await;
        let id = format!("{}-team", f.org);
        team(store.as_ref(), &f, &id, 0).await;
        let svc = OrganizationService::new(store.clone()).with_team_policy(TeamPolicy::new(
            true,
            None,
            Some(limit),
            false,
        ));
        let out = svc
            .add_member(
                None,
                false,
                AddMemberInput {
                    user_id: f.recipient.clone(),
                    role: MemberRoles::Role("member".into()),
                    organization_id: Some(f.org.clone()),
                    team_id: Some(id.clone()),
                },
                f.now,
            )
            .await;
        assert_eq!(out.is_ok(), limit == 1);
        assert_eq!(
            records(store.as_ref(), Model::Member, "organizationId", &f.org)
                .await
                .len(),
            if limit == 1 { 2 } else { 1 }
        );
        if limit == 1 {
            let zero = OrganizationService::new(store.clone()).with_team_policy(TeamPolicy::new(
                true,
                None,
                Some(0),
                false,
            ));
            let body = json!({"organizationId":f.org,"teamId":id,"userId":f.recipient});
            assert!(
                zero.add_team_member(&f.owner_token, false, &body, f.now)
                    .await
                    .is_ok()
            );
        }
    }
    for (label, count, stored, limit, expected) in [
        ("empty", 0, 0, 0, false),
        ("space", 0, 9, 1, true),
        ("full", 1, 0, 1, false),
        ("window", 101, 0, 101, true),
    ] {
        let f = setup(store.as_ref(), label).await;
        let id = format!("{}-team", f.org);
        team(store.as_ref(), &f, &id, stored).await;
        let mut tx = store.begin(true).await.unwrap();
        for i in 0..count {
            tx.insert(Model::TeamMember,json!({"id":format!("{id}-pair-{i}"),"teamId":id,"userId":format!("other-{i}"),"createdAt":timestamp(f.now)})).await.unwrap();
        }
        tx.commit().await.unwrap();
        let svc = OrganizationService::new(store.clone()).with_team_policy(TeamPolicy::new(
            true,
            None,
            Some(limit),
            false,
        ));
        let mut body = request(&f);
        body["teamId"] = json!(id);
        let out = svc
            .create_invitation(&f.owner_token, false, &body, f.now)
            .await;
        assert_eq!(out.is_ok(), expected);
        if !expected {
            assert_eq!(out.err().unwrap().code, "TEAM_MEMBER_LIMIT_REACHED");
        }
    }
    for limit in [0, 1] {
        let f = setup(store.as_ref(), &format!("accept-{limit}")).await;
        let id = format!("{}-team", f.org);
        team(store.as_ref(), &f, &id, 0).await;
        let svc = OrganizationService::new(store.clone());
        let mut body = request(&f);
        body["teamId"] = json!(id);
        let invitation = svc
            .create_invitation(&f.owner_token, false, &body, f.now)
            .await
            .unwrap();
        let svc = svc.with_team_policy(TeamPolicy::new(true, None, Some(limit), false));
        let out = svc
            .accept_invitation(
                &f.recipient_token,
                false,
                text(&invitation, "id").unwrap(),
                f.now,
            )
            .await;
        assert_eq!(out.is_ok(), limit == 1);
        let row = records(store.as_ref(), Model::Invitation, "organizationId", &f.org)
            .await
            .pop()
            .unwrap();
        assert_eq!(
            row["status"],
            if limit == 1 { "accepted" } else { "pending" }
        );
    }
    // Retain overcounts; reject actual full teams even if their persisted counter is low.
    for (label, actual, stored) in [("undercount", 1, 0), ("overcount", 0, 9)] {
        let f = setup(store.as_ref(), label).await;
        let id = format!("{}-team", f.org);
        team(store.as_ref(), &f, &id, stored).await;
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(Model::Member,json!({"id":format!("{}-recipient-member",f.org),"organizationId":f.org,"userId":f.recipient,"role":"member","createdAt":timestamp(f.now)})).await.unwrap();
        for i in 0..actual {
            tx.insert(Model::TeamMember,json!({"id":format!("{id}-pair-{i}"),"teamId":id,"userId":format!("other-{i}"),"createdAt":timestamp(f.now)})).await.unwrap();
        }
        tx.commit().await.unwrap();
        let svc = OrganizationService::new(store.clone()).with_team_policy(TeamPolicy::new(
            true,
            None,
            Some(1),
            false,
        ));
        let out = svc
            .add_team_member(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"teamId":id,"userId":f.recipient}),
                f.now,
            )
            .await;
        assert_eq!(out.err().unwrap().code, "TEAM_MEMBER_LIMIT_REACHED");
        let mut tx = store.begin(false).await.unwrap();
        assert_eq!(
            tx.get(Model::Team, &id).await.unwrap().unwrap()["memberCount"],
            stored
        );
        tx.commit().await.unwrap();
    }
    // A later full team rolls back a prior seat and preserves the pending invitation.
    let f = setup(store.as_ref(), "accept-rollback").await;
    let a = format!("{}-a", f.org);
    let b = format!("{}-b", f.org);
    team(store.as_ref(), &f, &a, 0).await;
    team(store.as_ref(), &f, &b, 1).await;
    let svc = OrganizationService::new(store.clone());
    let mut body = request(&f);
    body["teamId"] = json!([a, b]);
    let invite = svc
        .create_invitation(&f.owner_token, false, &body, f.now)
        .await
        .unwrap();
    let svc = svc.with_team_policy(TeamPolicy::new(true, None, Some(1), false));
    let out = svc
        .accept_invitation(
            &f.recipient_token,
            false,
            text(&invite, "id").unwrap(),
            f.now,
        )
        .await;
    assert_eq!(out.err().unwrap().code, "TEAM_MEMBER_LIMIT_REACHED");
    assert!(
        records(store.as_ref(), Model::TeamMember, "teamId", &a)
            .await
            .is_empty()
    );
    assert_eq!(
        records(store.as_ref(), Model::Invitation, "organizationId", &f.org).await[0]["status"],
        "pending"
    );
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::Team, &a).await.unwrap().unwrap()["memberCount"],
        0
    );
    tx.commit().await.unwrap();
    // Team quota preserves upstream first-100 list behavior even at a shifted window.
    let f = setup(store.as_ref(), "quota-window").await;
    let mut tx = store.begin(true).await.unwrap();
    for i in 0..101 {
        tx.insert(Model::Team,json!({"id":format!("{}-{i}",f.org),"organizationId":f.org,"name":"Window","memberCount":0,"createdAt":timestamp(f.now)})).await.unwrap();
    }
    tx.commit().await.unwrap();
    let svc = OrganizationService::new(store.clone()).with_team_policy(TeamPolicy::new(
        true,
        Some(101),
        None,
        false,
    ));
    assert!(
        svc.create_team(
            &f.owner_token,
            false,
            &json!({"organizationId":f.org,"name":"Window admitted"}),
            f.now
        )
        .await
        .is_ok()
    );
    // Concurrent deletion preserves one team unless explicitly allowed.
    let f = setup(store.as_ref(), "last-race").await;
    let first = format!("{}-first", f.org);
    let second = format!("{}-second", f.org);
    team(store.as_ref(), &f, &first, 0).await;
    team(store.as_ref(), &f, &second, 0).await;
    let svc = OrganizationService::new(store.clone());
    let a = json!({"organizationId":f.org,"teamId":first});
    let b = json!({"organizationId":f.org,"teamId":second});
    let (left, right) = tokio::join!(
        svc.remove_team(&f.owner_token, false, &a, f.now),
        svc.remove_team(&f.owner_token, false, &b, f.now)
    );
    assert_ne!(left.is_ok(), right.is_ok());
    assert_eq!(
        records(store.as_ref(), Model::Team, "organizationId", &f.org)
            .await
            .len(),
        1
    );
    // A quota scoped to one organization serializes concurrent new-team admission.
    let f = setup(store.as_ref(), "create-race").await;
    let svc = OrganizationService::new(store.clone()).with_team_policy(TeamPolicy::new(
        true,
        Some(1),
        None,
        false,
    ));
    let a = json!({"organizationId":f.org,"name":"First"});
    let b = json!({"organizationId":f.org,"name":"Second"});
    let (left, right) = tokio::join!(
        svc.create_team(&f.owner_token, false, &a, f.now),
        svc.create_team(&f.owner_token, false, &b, f.now)
    );
    assert_ne!(left.is_ok(), right.is_ok());
    assert_eq!(
        records(store.as_ref(), Model::Team, "organizationId", &f.org)
            .await
            .len(),
        1
    );
    #[cfg(feature = "http")]
    http_policy(store.clone()).await;
    // Concurrent new memberships cannot both take the final seat.
    let f = setup(store.as_ref(), "race").await;
    let id = format!("{}-team", f.org);
    team(store.as_ref(), &f, &id, 0).await;
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Member,json!({"id":format!("{}-recipient-member",f.org),"organizationId":f.org,"userId":f.recipient,"role":"member","createdAt":timestamp(f.now)})).await.unwrap();
    tx.commit().await.unwrap();
    let svc = OrganizationService::new(store.clone()).with_team_policy(TeamPolicy::new(
        true,
        None,
        Some(1),
        false,
    ));
    let a = json!({"organizationId":f.org,"teamId":id,"userId":f.owner});
    let b = json!({"organizationId":f.org,"teamId":id,"userId":f.recipient});
    let (left, right) = tokio::join!(
        svc.add_team_member(&f.owner_token, false, &a, f.now),
        svc.add_team_member(&f.owner_token, false, &b, f.now)
    );
    assert_ne!(left.is_ok(), right.is_ok());
    assert_eq!(
        records(store.as_ref(), Model::TeamMember, "teamId", &id)
            .await
            .len(),
        1
    );
}
#[cfg(feature = "http")]
async fn http_policy<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use tower::ServiceExt;
    for (i, path, limit, status, code) in [
        (
            0,
            "/organization/create-team",
            true,
            400,
            "YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_TEAMS",
        ),
        (
            1,
            "/organization/invite-member",
            false,
            403,
            "TEAM_MEMBER_LIMIT_REACHED",
        ),
    ] {
        let f = setup(store.as_ref(), &format!("http-{i}")).await;
        let id = format!("{}-team", f.org);
        team(store.as_ref(), &f, &id, 0).await;
        let now = f.now;
        let app = crate::http::AuthHttp::new(
            store.clone(),
            "http://localhost:3000".into(),
            vec![7; 32],
            vec![],
        )
        .unwrap()
        .with_team_policy(TeamPolicy::new(true, Some(1), Some(0), false))
        .with_invitation_policy(crate::organizations::InvitationPolicy::default())
        .with_rate_limiting(false)
        .with_clock(move || now);
        let cookie = (crate::cookies::SessionCookie { secure: false }).serialize(
            &f.owner_token,
            &[7; 32],
            true,
        );
        let body = if limit {
            json!({"organizationId":f.org,"name":"Denied","maximumTeams":999})
        } else {
            let mut body = request(&f);
            body["teamId"] = json!(id);
            body["maximumMembersPerTeam"] = json!(999);
            body
        };
        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://localhost:3000")
            .header(header::COOKIE, cookie.split(';').next().unwrap())
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.router().oneshot(req).await.unwrap();
        assert_eq!(response.status().as_u16(), status);
        assert!(!response.headers().contains_key(header::SET_COOKIE));
        let value: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(value["code"], code);
    }
}

#[tokio::test]
async fn scalar_team_options_all_admission_paths_and_atomic_seats() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}
