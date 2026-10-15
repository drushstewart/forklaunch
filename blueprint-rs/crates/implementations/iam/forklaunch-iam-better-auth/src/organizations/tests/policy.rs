use super::*;
use crate::organizations::{AddMemberInput, MemberRoles, OrganizationPolicy};

struct Fixture {
    owner: String,
    target: String,
    second: String,
    org: String,
    token: String,
    now: DateTime<Utc>,
}
async fn seed<S: AuthStore>(store: &S, label: &str, count: usize, all_owned: bool) -> Fixture {
    let prefix = format!("org-policy-{label}");
    let owner = format!("{prefix}-owner");
    let target = format!("{prefix}-target");
    let second = format!("{prefix}-second");
    let org = format!("{prefix}-org");
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    for id in [&owner, &target, &second] {
        tx.insert(Model::User,json!({"id":id,"email":format!("{id}@example.test"),"name":"Policy User","emailVerified":true})).await.unwrap();
    }
    tx.insert(
        Model::Organization,
        json!({"id":org,"slug":prefix,"name":"Policy Org","createdAt":timestamp(now)}),
    )
    .await
    .unwrap();
    for index in 0..count {
        let user = if all_owned || index == 0 {
            owner.clone()
        } else {
            format!("{prefix}-other-{index}")
        };
        tx.insert(Model::Member,json!({"id":format!("{prefix}-member-{index}"),"organizationId":org,"userId":user,"role":"viewer","createdAt":timestamp(now)})).await.unwrap();
    }
    let session = crate::sessions::new_session(&owner, "", "", now).unwrap();
    let token = session["token"].as_str().unwrap().to_owned();
    tx.insert(Model::Session, session).await.unwrap();
    tx.commit().await.unwrap();
    Fixture {
        owner,
        target,
        second,
        org,
        token,
        now,
    }
}
fn input(f: &Fixture, user: &str) -> AddMemberInput {
    AddMemberInput {
        user_id: user.to_owned(),
        role: MemberRoles::Role("member".into()),
        organization_id: Some(f.org.clone()),
        team_id: None,
    }
}
async fn members<S: AuthStore>(store: &S, org: &str) -> Vec<Value> {
    let mut tx = store.begin(false).await.unwrap();
    let rows = crate::store::find_all(&mut tx, Model::Member, "organizationId", org)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    rows
}

#[allow(clippy::too_many_lines)]
pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    assert_eq!(
        OrganizationPolicy::new(true, None, 0),
        OrganizationPolicy::default()
    );
    for (index, policy, count, expected) in [
        (
            0,
            OrganizationPolicy::new(false, Some(0), 100),
            1,
            Some("YOU_ARE_NOT_ALLOWED_TO_CREATE_A_NEW_ORGANIZATION"),
        ),
        (
            1,
            OrganizationPolicy::new(true, Some(0), 100),
            0,
            Some("YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_ORGANIZATIONS"),
        ),
        (2, OrganizationPolicy::new(true, Some(1), 100), 0, None),
        (
            3,
            OrganizationPolicy::new(true, Some(1), 100),
            1,
            Some("YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_ORGANIZATIONS"),
        ),
        (4, OrganizationPolicy::new(true, Some(2), 100), 1, None),
        (5, OrganizationPolicy::new(true, Some(101), 100), 102, None),
        (
            6,
            OrganizationPolicy::new(true, Some(100), 100),
            102,
            Some("YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_ORGANIZATIONS"),
        ),
    ] {
        let f = seed(store.as_ref(), &format!("create-{index}"), count, true).await;
        let service = OrganizationService::new(store.clone()).with_organization_policy(policy);
        let before = members(store.as_ref(), &f.org).await;
        let body = json!({"name":"Created","slug":format!("org-policy-new-{index}")});
        let result = service.create(&f.token, false, &body, f.now).await;
        if let Some(code) = expected {
            assert_eq!(result.unwrap_err().code, code);
            assert_eq!(members(store.as_ref(), &f.org).await, before);
        } else {
            assert_eq!(result.unwrap()["name"], "Created");
        }
    }
    // Existing duplicate slug loses to both creation policy checks.
    let f = seed(store.as_ref(), "order", 1, true).await;
    for (allow, code) in [
        (false, "YOU_ARE_NOT_ALLOWED_TO_CREATE_A_NEW_ORGANIZATION"),
        (true, "YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_ORGANIZATIONS"),
    ] {
        let service = OrganizationService::new(store.clone())
            .with_organization_policy(OrganizationPolicy::new(allow, Some(0), 100));
        assert_eq!(
            service
                .create(
                    &f.token,
                    false,
                    &json!({"name":"Duplicate","slug":"org-policy-order"}),
                    f.now
                )
                .await
                .unwrap_err()
                .code,
            code
        );
        assert_eq!(
            service
                .create(
                    "invalid",
                    false,
                    &json!({"name":"Unauthorized","slug":"org-policy-order"}),
                    f.now
                )
                .await
                .unwrap_err()
                .code,
            "UNAUTHORIZED"
        );
    }
    // Foreign memberships cannot consume this user's limit; orphan rows do.
    let f = seed(store.as_ref(), "scope", 0, false).await;
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::Member,
        json!({"id":"org-policy-foreign","userId":f.target,"organizationId":f.org,"role":"owner"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let service = OrganizationService::new(store.clone())
        .with_organization_policy(OrganizationPolicy::new(true, Some(1), 100));
    service
        .create(
            &f.token,
            false,
            &json!({"name":"Scoped","slug":"org-policy-scoped-new"}),
            f.now,
        )
        .await
        .unwrap();
    let f = seed(store.as_ref(), "orphan", 1, true).await;
    let mut tx = store.begin(true).await.unwrap();
    tx.delete(Model::Organization, &f.org).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        service
            .create(
                &f.token,
                false,
                &json!({"name":"Orphan","slug":"org-policy-orphan-new"}),
                f.now
            )
            .await
            .unwrap_err()
            .code,
        "YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_ORGANIZATIONS"
    );
    // Scalar membership limits retain existing missing/duplicate/scope ordering.
    for (index, limit, count, expected) in [
        (0, 1, 1, false),
        (1, 0, 1, true),
        (2, 100, 99, true),
        (3, 100, 100, false),
    ] {
        let f = seed(store.as_ref(), &format!("members-{index}"), count, false).await;
        let service = OrganizationService::new(store.clone())
            .with_organization_policy(OrganizationPolicy::new(true, None, limit));
        let before = members(store.as_ref(), &f.org).await;
        let result = service
            .add_member(None, false, input(&f, &f.target), f.now)
            .await;
        if expected {
            assert_eq!(result.unwrap()["userId"], f.target);
        } else {
            assert_eq!(
                result.unwrap_err().code,
                "ORGANIZATION_MEMBERSHIP_LIMIT_REACHED"
            );
            assert_eq!(members(store.as_ref(), &f.org).await, before);
        }
    }
    let f = seed(store.as_ref(), "member-order", 1, false).await;
    let service = OrganizationService::new(store.clone())
        .with_organization_policy(OrganizationPolicy::new(true, None, 1));
    assert_eq!(
        service
            .add_member(None, false, input(&f, "missing"), f.now)
            .await
            .unwrap_err()
            .code,
        "USER_NOT_FOUND"
    );
    assert_eq!(
        service
            .add_member(None, false, input(&f, &f.owner), f.now)
            .await
            .unwrap_err()
            .code,
        "USER_IS_ALREADY_A_MEMBER_OF_THIS_ORGANIZATION"
    );
    let mut absent = input(&f, &f.target);
    absent.organization_id = Some("org-policy-missing".into());
    assert_eq!(
        service
            .add_member(None, false, absent, f.now)
            .await
            .unwrap_err()
            .code,
        "ORGANIZATION_NOT_FOUND"
    );
    // Low policies affect user hydration, not the independent member join limit.
    let f = seed(store.as_ref(), "hydrate", 2, false).await;
    let mut tx = store.begin(true).await.unwrap();
    let mut row = tx
        .get(Model::Member, "org-policy-hydrate-member-1")
        .await
        .unwrap()
        .unwrap();
    row["userId"] = json!(f.target);
    tx.replace(Model::Member, row).await.unwrap();
    tx.commit().await.unwrap();
    let low = OrganizationService::new(store.clone())
        .with_organization_policy(OrganizationPolicy::new(true, None, 1));
    assert_eq!(
        low.full(&f.token, false, Some(&f.org), None, None, f.now)
            .await
            .unwrap_err()
            .code,
        "better_auth_storage_error"
    );
    assert_eq!(
        low.full(&f.token, false, Some(&f.org), None, Some(1), f.now)
            .await
            .unwrap()["members"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let fallback = OrganizationService::new(store.clone())
        .with_organization_policy(OrganizationPolicy::new(true, None, 0));
    assert_eq!(
        fallback
            .full(&f.token, false, Some(&f.org), None, None, f.now)
            .await
            .unwrap()["members"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    concurrency(store.clone()).await;
    invitation_policy(store.clone()).await;
    #[cfg(feature = "http")]
    http_policy(store).await;
}

async fn concurrency<S: AuthStore>(store: Arc<S>) {
    let f = seed(store.as_ref(), "create-race", 0, false).await;
    let service = OrganizationService::new(store.clone())
        .with_organization_policy(OrganizationPolicy::new(true, Some(1), 100));
    let a = json!({"name":"Race A","slug":"org-policy-race-a"});
    let b = json!({"name":"Race B","slug":"org-policy-race-b"});
    let (a, b) = tokio::join!(
        service.create(&f.token, false, &a, f.now),
        service.create(&f.token, false, &b, f.now)
    );
    let ((Ok(_), Err(error)) | (Err(error), Ok(_))) = (a, b) else {
        panic!("one organization admission must win");
    };
    assert_eq!(
        error.code,
        "YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_ORGANIZATIONS"
    );
    let f = seed(store.as_ref(), "member-race", 1, false).await;
    let service = OrganizationService::new(store.clone())
        .with_organization_policy(OrganizationPolicy::new(true, None, 2));
    let (a, b) = tokio::join!(
        service.add_member(None, false, input(&f, &f.target), f.now),
        service.add_member(None, false, input(&f, &f.second), f.now)
    );
    let ((Ok(_), Err(error)) | (Err(error), Ok(_))) = (a, b) else {
        panic!("one membership admission must win");
    };
    assert_eq!(error.code, "ORGANIZATION_MEMBERSHIP_LIMIT_REACHED");
    assert_eq!(members(store.as_ref(), &f.org).await.len(), 2);
}

async fn invitation_policy<S: AuthStore>(store: Arc<S>) {
    for (index, limit, expected) in [(0, 1, false), (1, 0, true)] {
        let f = seed(store.as_ref(), &format!("invite-{index}"), 1, false).await;
        let id = format!("org-policy-invitation-{index}");
        let mut tx = store.begin(true).await.unwrap();
        let email = format!("{}@example.test", f.owner); // Recipient is not already a member.
        let mut rows = crate::store::find_all(&mut tx, Model::Member, "organizationId", &f.org)
            .await
            .unwrap();
        rows[0]["userId"] = json!(f.target);
        tx.replace(Model::Member, rows.remove(0)).await.unwrap();
        tx.insert(Model::Invitation,json!({"id":id,"email":email,"organizationId":f.org,"inviterId":f.target,"role":"member","status":"pending","expiresAt":timestamp(f.now+chrono::Duration::hours(1)),"createdAt":timestamp(f.now)})).await.unwrap();
        tx.commit().await.unwrap();
        let service = OrganizationService::new(store.clone())
            .with_organization_policy(OrganizationPolicy::new(true, None, limit));
        let result = service.accept_invitation(&f.token, false, &id, f.now).await;
        if expected {
            assert!(result.is_ok());
        } else {
            assert_eq!(
                result.err().unwrap().code,
                "ORGANIZATION_MEMBERSHIP_LIMIT_REACHED"
            );
        }
        let mut tx = store.begin(false).await.unwrap();
        assert_eq!(
            tx.get(Model::Invitation, &id).await.unwrap().unwrap()["status"],
            if expected { "accepted" } else { "pending" }
        );
        tx.commit().await.unwrap();
    }
}

#[cfg(feature = "http")]
async fn http_policy<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use tower::ServiceExt;
    let f = seed(store.as_ref(), "http", 1, true).await;
    for (allow, code) in [
        (false, "YOU_ARE_NOT_ALLOWED_TO_CREATE_A_NEW_ORGANIZATION"),
        (true, "YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_ORGANIZATIONS"),
    ] {
        let app = crate::http::AuthHttp::new(
            store.clone(),
            "http://localhost:3000".into(),
            vec![7; 32],
            vec![],
        )
        .unwrap()
        .with_organization_policy(OrganizationPolicy::new(allow, Some(0), 100))
        .with_rate_limiting(false)
        .with_clock(move || f.now);
        let cookie =
            (crate::cookies::SessionCookie { secure: false }).serialize(&f.token, &[7; 32], true);
        let request=Request::builder().method("POST").uri("/organization/create").header("origin","http://localhost:3000").header("content-type","application/json").header("cookie",cookie).body(Body::from(json!({"name":"Denied","slug":"org-policy-http-new","allowUserToCreateOrganization":true,"organizationLimit":999}).to_string())).unwrap();
        let response = app.router().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(!response.headers().contains_key(header::SET_COOKIE));
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(body["code"], code);
    }
}
#[tokio::test]
async fn organization_scalar_policy_defaults_order_scope_concurrency() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}
