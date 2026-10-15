use super::*;
use crate::organizations::InvitationPolicy;

struct Fixture {
    org: String,
    owner: String,
    recipient: String,
    owner_token: String,
    recipient_token: String,
    now: DateTime<Utc>,
}
async fn seed<S: AuthStore>(store: &S, label: &str) -> Fixture {
    let org = format!("invite-policy-{label}");
    let owner = format!("{org}-owner");
    let recipient = format!("{org}-recipient");
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    for (id, verified) in [(&owner, true), (&recipient, false)] {
        tx.insert(Model::User,json!({"id":id,"name":"Invitation User","email":format!("{id}@example.test"),"emailVerified":verified})).await.unwrap();
    }
    tx.insert(
        Model::Organization,
        json!({"id":org,"slug":org,"name":"Invitation Org","createdAt":timestamp(now)}),
    )
    .await
    .unwrap();
    tx.insert(Model::Member,json!({"id":format!("{org}-member"),"organizationId":org,"userId":owner,"role":"owner","createdAt":timestamp(now)})).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":format!("{org}-grant"),"organizationId":org,"role":"owner","permission":"{\"invitation\":[\"create\"]}","createdAt":timestamp(now)})).await.unwrap();
    let os = crate::sessions::new_session(&owner, "", "", now).unwrap();
    let rs = crate::sessions::new_session(&recipient, "", "", now).unwrap();
    let owner_token = text(&os, "token").unwrap().to_owned();
    let recipient_token = text(&rs, "token").unwrap().to_owned();
    tx.insert(Model::Session, os).await.unwrap();
    tx.insert(Model::Session, rs).await.unwrap();
    tx.commit().await.unwrap();
    Fixture {
        org,
        owner,
        recipient,
        owner_token,
        recipient_token,
        now,
    }
}
fn request(f: &Fixture) -> Value {
    json!({"email":format!("{}@example.test",f.recipient),"organizationId":f.org,"role":"member"})
}
async fn rows<S: AuthStore>(store: &S, org: &str) -> Vec<Value> {
    let mut tx = store.begin(false).await.unwrap();
    let rows = crate::store::find_all(&mut tx, Model::Invitation, "organizationId", org)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    rows
}
async fn insert<S: AuthStore>(store: &S, f: &Fixture, index: usize, expiry: DateTime<Utc>) {
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Invitation,json!({"id":format!("{}-pending-{index}",f.org),"email":format!("pending-{index}@example.test"),"organizationId":f.org,"inviterId":f.owner,"role":"member","status":"pending","createdAt":timestamp(f.now),"expiresAt":timestamp(expiry)})).await.unwrap();
    tx.commit().await.unwrap();
}

#[allow(clippy::too_many_lines)]
pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    assert_eq!(
        InvitationPolicy::new(0, 100, true),
        InvitationPolicy::default()
    );
    assert!(
        InvitationPolicy::new(u32::MAX, 100, true)
            .expiry(DateTime::<Utc>::MAX_UTC)
            .is_err()
    );
    for (index, seconds, expected) in [
        (0, 0, 172_800),
        (1, 2, 2),
        (2, u32::MAX, i64::from(u32::MAX)),
    ] {
        let f = seed(store.as_ref(), &format!("expiry-{index}")).await;
        let service = OrganizationService::new(store.clone())
            .with_invitation_policy(InvitationPolicy::new(seconds, 100, true));
        let result = service
            .create_invitation(&f.owner_token, false, &request(&f), f.now)
            .await
            .unwrap();
        assert_eq!(
            result["expiresAt"],
            timestamp(f.now + chrono::Duration::seconds(expected))
        );
        let mut body = request(&f);
        body["resend"] = json!(true);
        let resent = service
            .create_invitation(
                &f.owner_token,
                false,
                &body,
                f.now + chrono::Duration::seconds(1),
            )
            .await
            .unwrap();
        assert_eq!(resent["id"], result["id"]);
        assert_eq!(
            resent["expiresAt"],
            timestamp(f.now + chrono::Duration::seconds(expected + 1))
        );
    }
    for (index, count, expired, limit, expected) in [
        (0, 0, 0, 0, false),
        (1, 1, 0, 1, false),
        (2, 101, 0, 101, true),
        (3, 101, 100, 1, true),
    ] {
        let f = seed(store.as_ref(), &format!("quota-{index}")).await;
        for i in 0..count {
            insert(
                store.as_ref(),
                &f,
                i,
                f.now + chrono::Duration::seconds(if i < expired { -1 } else { 3600 }),
            )
            .await;
        }
        let service = OrganizationService::new(store.clone())
            .with_invitation_policy(InvitationPolicy::new(2, limit, true));
        let before = rows(store.as_ref(), &f.org).await;
        let result = service
            .create_invitation(&f.owner_token, false, &request(&f), f.now)
            .await;
        if expected {
            assert!(result.is_ok());
        } else {
            assert_eq!(result.err().unwrap().code, "INVITATION_LIMIT_REACHED");
            assert_eq!(before, rows(store.as_ref(), &f.org).await);
        }
    }
    let f = seed(store.as_ref(), "resend").await;
    let initial = OrganizationService::new(store.clone())
        .create_invitation(&f.owner_token, false, &request(&f), f.now)
        .await
        .unwrap();
    let service = OrganizationService::new(store.clone())
        .with_invitation_policy(InvitationPolicy::new(10, 0, false));
    assert_eq!(
        service
            .create_invitation(&f.owner_token, false, &request(&f), f.now)
            .await
            .err()
            .unwrap()
            .code,
        "USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION"
    );
    let mut body = request(&f);
    body["resend"] = json!(true);
    let resent = service
        .create_invitation(
            &f.owner_token,
            false,
            &body,
            f.now + chrono::Duration::seconds(1),
        )
        .await
        .unwrap();
    assert_eq!(resent["id"], initial["id"]);
    assert_eq!(
        resent["expiresAt"],
        timestamp(f.now + chrono::Duration::seconds(11))
    );
    assert_eq!(rows(store.as_ref(), &f.org).await.len(), 1);
    recipient_policy(store.clone()).await;
    let f = seed(store.as_ref(), "concurrent").await;
    let service = OrganizationService::new(store.clone())
        .with_invitation_policy(InvitationPolicy::new(2, 1, true));
    let a = request(&f);
    let mut b = a.clone();
    b["email"] = json!("another-invite@example.test");
    let (a, b) = tokio::join!(
        service.create_invitation(&f.owner_token, false, &a, f.now),
        service.create_invitation(&f.owner_token, false, &b, f.now)
    );
    let ((Ok(_), Err(error)) | (Err(error), Ok(_))) = (a, b) else {
        panic!("one invitation must win")
    };
    assert_eq!(error.code, "INVITATION_LIMIT_REACHED");
    assert_eq!(rows(store.as_ref(), &f.org).await.len(), 1);
    #[cfg(feature = "http")]
    http_policy(store).await;
}

async fn recipient_policy<S: AuthStore>(store: Arc<S>) {
    for (index, require) in [(0, true), (1, false)] {
        for operation in ["get", "accept", "reject"] {
            let f = seed(store.as_ref(), &format!("recipient-{index}-{operation}")).await;
            let service = OrganizationService::new(store.clone())
                .with_invitation_policy(InvitationPolicy::new(2, 100, require));
            let initial = service
                .create_invitation(&f.owner_token, false, &request(&f), f.now)
                .await
                .unwrap();
            let id = text(&initial, "id").unwrap();
            let before = rows(store.as_ref(), &f.org).await;
            let result = match operation {
                "get" => {
                    service
                        .get_invitation(&f.recipient_token, false, id, f.now)
                        .await
                }
                "accept" => service
                    .accept_invitation(&f.recipient_token, false, id, f.now)
                    .await
                    .map(|r| r.body),
                _ => {
                    service
                        .reject_invitation(&f.recipient_token, false, id, f.now)
                        .await
                }
            };
            if require {
                assert!(
                    result
                        .err()
                        .unwrap()
                        .code
                        .starts_with("EMAIL_VERIFICATION_REQUIRED")
                );
                assert_eq!(before, rows(store.as_ref(), &f.org).await);
            } else {
                assert!(result.is_ok());
            }
            assert_eq!(
                service
                    .list_user_invitations(&f.recipient_token, false, None, f.now)
                    .await
                    .err()
                    .unwrap()
                    .code,
                "EMAIL_VERIFICATION_REQUIRED_FOR_INVITATION"
            );
        }
    }
    let f = seed(store.as_ref(), "equality").await;
    let service = OrganizationService::new(store.clone())
        .with_invitation_policy(InvitationPolicy::new(2, 100, false));
    let initial = service
        .create_invitation(&f.owner_token, false, &request(&f), f.now)
        .await
        .unwrap();
    let id = text(&initial, "id").unwrap();
    assert!(
        service
            .get_invitation(
                &f.recipient_token,
                false,
                id,
                f.now + chrono::Duration::seconds(2)
            )
            .await
            .is_ok()
    );
    assert!(
        service
            .get_invitation(
                &f.recipient_token,
                false,
                id,
                f.now + chrono::Duration::milliseconds(2001)
            )
            .await
            .is_err()
    );
}

#[cfg(feature = "http")]
async fn http_policy<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use tower::ServiceExt;
    let f = seed(store.as_ref(), "http").await;
    let now = f.now;
    let initial = OrganizationService::new(store.clone())
        .create_invitation(&f.owner_token, false, &request(&f), now)
        .await
        .unwrap();
    let app =
        crate::http::AuthHttp::new(store, "http://localhost:3000".into(), vec![7; 32], vec![])
            .unwrap()
            .with_invitation_policy(InvitationPolicy::new(2, 0, false))
            .with_organization_policy(crate::organizations::OrganizationPolicy::new(
                false, None, 100,
            ))
            .with_rate_limiting(false)
            .with_clock(move || now);
    let cookie = (crate::cookies::SessionCookie { secure: false }).serialize(
        &f.recipient_token,
        &[7; 32],
        true,
    );
    let req = Request::builder()
        .uri(format!(
            "/organization/get-invitation?id={}",
            text(&initial, "id").unwrap()
        ))
        .header("cookie", cookie)
        .body(Body::empty())
        .unwrap();
    let response = app.router().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body["id"], initial["id"]);
}
#[tokio::test]
async fn invitation_scalar_policy_expiry_quota_recipient_and_atomicity() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}
