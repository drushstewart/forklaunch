use super::invitation_policy::{Fixture, seed};
use super::*;
use crate::organizations::CreatorRole;
async fn fixture<S: AuthStore>(store: &S, label: &str, actor: &str, second: bool) -> Fixture {
    let f = seed(store, &format!("creator-{label}")).await;
    let mut tx = store.begin(true).await.unwrap();
    let mut member = tx
        .get(Model::Member, &format!("{}-member", f.org))
        .await
        .unwrap()
        .unwrap();
    member["role"] = json!(actor);
    tx.replace(Model::Member, member).await.unwrap();
    let mut grant = tx
        .get(Model::OrganizationRole, &format!("{}-grant", f.org))
        .await
        .unwrap()
        .unwrap();
    grant["role"] = json!(actor);
    grant["permission"] = json!("{\"invitation\":[\"create\"],\"member\":[\"delete\"]}");
    tx.replace(Model::OrganizationRole, grant).await.unwrap();
    if second {
        tx.insert(Model::Member,json!({"id":format!("{}-second",f.org),"organizationId":f.org,"userId":f.recipient,"role":"viewer"})).await.unwrap();
    }
    tx.commit().await.unwrap();
    f
}
fn service<S: AuthStore>(store: Arc<S>) -> OrganizationService<S> {
    OrganizationService::new(store).with_creator_role(CreatorRole::new("viewer").unwrap())
}
#[allow(clippy::too_many_lines)]
pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    assert_eq!(CreatorRole::new("").unwrap(), CreatorRole::default());
    assert!(CreatorRole::new(&"x".repeat(4097)).is_err());
    assert!(CreatorRole::new(&"x".repeat(4096)).is_ok());
    for (role, deny) in [("viewer", true), ("owner", false)] {
        let f = fixture(store.as_ref(), &format!("leave-{role}"), role, false).await;
        let out = service(store.clone())
            .leave(&f.owner_token, false, &f.org, f.now)
            .await;
        if deny {
            assert_eq!(
                out.unwrap_err().code,
                "YOU_CANNOT_LEAVE_THE_ORGANIZATION_AS_THE_ONLY_OWNER"
            );
        } else {
            assert!(out.is_ok());
        }
    }
    for (i, actor, target, second, code) in [
        (
            0,
            "viewer",
            "owner",
            false,
            Some("YOU_CANNOT_LEAVE_THE_ORGANIZATION_WITHOUT_AN_OWNER"),
        ),
        (
            1,
            "owner",
            "viewer",
            false,
            Some("YOU_ARE_NOT_ALLOWED_TO_UPDATE_THIS_MEMBER"),
        ),
        (2, "viewer", "owner", true, None),
    ] {
        let f = fixture(store.as_ref(), &format!("update-{i}"), actor, second).await;
        let out=service(store.clone()).update_member_role(&f.owner_token,false,&json!({"organizationId":f.org,"memberId":format!("{}-member",f.org),"role":target}),f.now).await;
        if let Some(code) = code {
            assert_eq!(out.unwrap_err().code, code);
        } else {
            assert!(out.is_ok());
        }
    }
    for (actor, deny) in [("owner", true), ("viewer", false)] {
        let f = fixture(store.as_ref(), &format!("invite-{actor}"), actor, false).await;
        let out=service(store.clone()).create_invitation(&f.owner_token,false,&json!({"organizationId":f.org,"email":format!("new-{}@example.test",f.org),"role":"viewer"}),f.now).await;
        if deny {
            assert_eq!(
                out.unwrap_err().code,
                "YOU_ARE_NOT_ALLOWED_TO_INVITE_USER_WITH_THIS_ROLE"
            );
        } else {
            assert!(out.is_ok());
        }
        let f = fixture(store.as_ref(), &format!("remove-{actor}"), actor, true).await;
        let out = service(store.clone())
            .remove_member(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"memberIdOrEmail":format!("{}-second",f.org)}),
                f.now,
            )
            .await;
        if deny {
            assert_eq!(
                out.unwrap_err().code,
                "YOU_CANNOT_LEAVE_THE_ORGANIZATION_AS_THE_ONLY_OWNER"
            );
        } else {
            assert!(out.is_ok());
        }
    }
    for (i, role) in ["", " viewer ", "owner,viewer"].into_iter().enumerate() {
        let f = fixture(store.as_ref(), &format!("exact-{i}"), "owner", false).await;
        let svc = OrganizationService::new(store.clone())
            .with_creator_role(CreatorRole::new(role).unwrap());
        let created = svc
            .create(
                &f.owner_token,
                false,
                &json!({"name":"Exact","slug":format!("{}-new",f.org)}),
                f.now,
            )
            .await
            .unwrap();
        assert_eq!(
            created["members"][0]["role"],
            if role.is_empty() { "owner" } else { role }
        );
        let out = svc
            .leave(
                &f.owner_token,
                false,
                created["id"].as_str().unwrap(),
                f.now,
            )
            .await;
        assert_eq!(out.is_ok(), role.contains(','));
    }
    // Serialized last-creator checks prevent concurrent departures from removing both.
    let f = fixture(store.as_ref(), "concurrent", "viewer", true).await;
    let svc = service(store.clone());
    let (a, b) = tokio::join!(
        svc.leave(&f.owner_token, false, &f.org, f.now),
        svc.leave(&f.recipient_token, false, &f.org, f.now)
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        crate::store::find_all(&mut tx, Model::Member, "organizationId", &f.org)
            .await
            .unwrap()
            .len(),
        1
    );
    tx.commit().await.unwrap();
    let f = fixture(store.as_ref(), "demotion-race", "viewer", true).await;
    let svc = service(store.clone());
    let a = json!({"organizationId":f.org,"memberId":format!("{}-member",f.org),"role":"owner"});
    let b = json!({"organizationId":f.org,"memberId":format!("{}-second",f.org),"role":"owner"});
    let (a, b) = tokio::join!(
        svc.update_member_role(&f.owner_token, false, &a, f.now),
        svc.update_member_role(&f.recipient_token, false, &b, f.now)
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let mut tx = store.begin(false).await.unwrap();
    let members = crate::store::find_all(&mut tx, Model::Member, "organizationId", &f.org)
        .await
        .unwrap();
    assert_eq!(
        members
            .iter()
            .filter(|member| member["role"] == "viewer")
            .count(),
        1
    );
    assert_eq!(members.len(), 2);
    tx.commit().await.unwrap();
    // Scoped first-100 count deliberately excludes a creator beyond the adapter window.
    let f = fixture(store.as_ref(), "window", "viewer", false).await;
    let mut tx = store.begin(true).await.unwrap();
    for i in 0..99 {
        tx.insert(Model::Member,json!({"id":format!("{}-fill-{i}",f.org),"organizationId":f.org,"userId":format!("filler-{i}"),"role":"member"})).await.unwrap();
    }
    tx.insert(Model::Member,json!({"id":format!("{}-late",f.org),"organizationId":f.org,"userId":f.recipient,"role":"viewer"})).await.unwrap();
    tx.insert(Model::Member,json!({"id":format!("{}-foreign",f.org),"organizationId":"foreign-creator-org","userId":f.recipient,"role":"viewer"})).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        service(store.clone())
            .leave(&f.owner_token, false, &f.org, f.now)
            .await
            .unwrap_err()
            .code,
        "YOU_CANNOT_LEAVE_THE_ORGANIZATION_AS_THE_ONLY_OWNER"
    );
    #[cfg(feature = "http")]
    http_creation(store).await;
}
#[cfg(feature = "http")]
async fn http_creation<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use tower::ServiceExt;
    let f = fixture(store.as_ref(), "http", "owner", false).await;
    let now = f.now;
    let app =
        crate::http::AuthHttp::new(store, "http://localhost:3000".into(), vec![7; 32], vec![])
            .unwrap()
            .with_creator_role(CreatorRole::new("viewer").unwrap())
            .with_organization_controls(crate::organizations::OrganizationControls::default())
            .with_clock(move || now)
            .with_rate_limiting(false);
    let cookie =
        (crate::cookies::SessionCookie { secure: false }).serialize(&f.owner_token, &[7; 32], true);
    let req = Request::builder()
        .method("POST")
        .uri("/organization/create")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, cookie.split(';').next().unwrap())
        .body(Body::from(
            json!({"name":"HTTP","slug":format!("{}-http",f.org),"creatorRole":"owner"})
                .to_string(),
        ))
        .unwrap();
    let response = app.router().oneshot(req).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(body["members"][0]["role"], "viewer");
}
#[tokio::test]
async fn creator_role_full_lifecycle_and_scoped_atomic_guards() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}
