use super::invitation_policy::{Fixture, seed};
use super::*;
use crate::organizations::OrganizationControls;
use std::sync::atomic::{AtomicUsize, Ordering};
struct CountStore<S> {
    inner: Arc<S>,
    begins: Arc<AtomicUsize>,
}
impl<S: AuthStore> AuthStore for CountStore<S> {
    type Transaction<'a>
        = S::Transaction<'a>
    where
        Self: 'a;
    async fn begin(&self, write: bool) -> BlueprintResult<Self::Transaction<'_>> {
        self.begins.fetch_add(1, Ordering::SeqCst);
        self.inner.begin(write).await
    }
}
async fn setup<S: AuthStore>(store: &S, label: &str) -> Fixture {
    let f = seed(store, &format!("controls-{label}")).await;
    let mut tx = store.begin(true).await.unwrap();
    let mut grant = tx
        .get(Model::OrganizationRole, &format!("{}-grant", f.org))
        .await
        .unwrap()
        .unwrap();
    grant["permission"] = json!("{\"ac\":[\"create\"],\"organization\":[\"delete\"]}");
    tx.replace(Model::OrganizationRole, grant).await.unwrap();
    tx.commit().await.unwrap();
    f
}
#[allow(clippy::too_many_lines)]
pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    let f = setup(store.as_ref(), "disabled").await;
    let begins = Arc::new(AtomicUsize::new(0));
    let counted = Arc::new(CountStore {
        inner: store.clone(),
        begins: begins.clone(),
    });
    let svc = OrganizationService::new(counted.clone())
        .with_organization_controls(OrganizationControls::new(true, None));
    for token in ["", f.owner_token.as_str()] {
        assert_eq!(
            svc.delete(token, false, &f.org, f.now)
                .await
                .unwrap_err()
                .code,
            "ORGANIZATION_DELETION_DISABLED"
        );
    }
    assert_eq!(begins.load(Ordering::SeqCst), 0);
    #[cfg(feature = "http")]
    http_disabled(counted, begins, &f).await;
    for (i, limit, role, permission, code) in [
        (0, Some(0), "new", json!({}), Some("TOO_MANY_ROLES")),
        (1, Some(1), "new", json!({}), Some("TOO_MANY_ROLES")),
        (2, Some(2), "new", json!({}), None),
        (
            3,
            Some(0),
            "viewer",
            json!({}),
            Some("ROLE_NAME_IS_ALREADY_TAKEN"),
        ),
        (
            4,
            Some(0),
            "new",
            json!({"invalid":["bad"]}),
            Some("TOO_MANY_ROLES"),
        ),
        (5, None, "new", json!({}), None),
    ] {
        let f = setup(store.as_ref(), &format!("quota-{i}")).await;
        let svc = OrganizationService::new(store.clone())
            .with_organization_controls(OrganizationControls::new(false, limit));
        let out = svc
            .create_organization_role(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"role":role,"permission":permission}),
                f.now,
            )
            .await;
        if let Some(code) = code {
            assert_eq!(out.unwrap_err().code, code);
        } else {
            assert!(out.is_ok());
        }
    }
    for limit in [2, 3] {
        let f = setup(store.as_ref(), &format!("existing-{limit}")).await;
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(Model::OrganizationRole,json!({"id":format!("{}-existing",f.org),"organizationId":f.org,"role":"existing","permission":"{}"})).await.unwrap();
        tx.commit().await.unwrap();
        let svc = OrganizationService::new(store.clone())
            .with_organization_controls(OrganizationControls::new(false, Some(limit)));
        let out = svc
            .create_organization_role(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"role":"existing","permission":{}}),
                f.now,
            )
            .await;
        assert_eq!(
            out.unwrap_err().code,
            if limit == 2 {
                "TOO_MANY_ROLES"
            } else {
                "ROLE_NAME_IS_ALREADY_TAKEN"
            }
        );
    }
    #[cfg(feature = "http")]
    http_quota(store.clone()).await;
    // Count is not a first-100 hydrated permission window; foreign rows cannot exhaust it.
    let f = setup(store.as_ref(), "count").await;
    let mut tx = store.begin(true).await.unwrap();
    for i in 0..101 {
        tx.insert(Model::OrganizationRole,json!({"id":format!("{}-extra-{i}",f.org),"organizationId":f.org,"role":format!("extra-{i}"),"permission":"{}"})).await.unwrap();
    }
    tx.insert(Model::OrganizationRole,json!({"id":format!("{}-foreign",f.org),"organizationId":"other-controls-org","role":"foreign","permission":"{}"})).await.unwrap();
    tx.commit().await.unwrap();
    let body = json!({"organizationId":f.org,"role":"new","permission":{}});
    let svc = OrganizationService::new(store.clone())
        .with_organization_controls(OrganizationControls::new(false, Some(102)));
    assert_eq!(
        svc.create_organization_role(&f.owner_token, false, &body, f.now)
            .await
            .unwrap_err()
            .code,
        "TOO_MANY_ROLES"
    );
    let svc = svc.with_organization_controls(OrganizationControls::new(false, Some(103)));
    assert!(
        svc.create_organization_role(&f.owner_token, false, &body, f.now)
            .await
            .is_ok()
    );
    // One final slot and two concurrent requests produce exactly one insertion.
    let f = setup(store.as_ref(), "race").await;
    let svc = OrganizationService::new(store.clone())
        .with_organization_controls(OrganizationControls::new(false, Some(2)));
    let a = json!({"organizationId":f.org,"role":"first","permission":{}});
    let b = json!({"organizationId":f.org,"role":"second","permission":{}});
    let (a, b) = tokio::join!(
        svc.create_organization_role(&f.owner_token, false, &a, f.now),
        svc.create_organization_role(&f.owner_token, false, &b, f.now)
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.find_window(Model::OrganizationRole, "organizationId", &f.org, 0, 1)
            .await
            .unwrap()
            .total,
        2
    );
    tx.commit().await.unwrap();
    // Defaults still allow authorized deletion and preserve its existing cleanup behavior.
    let f = setup(store.as_ref(), "delete-default").await;
    assert!(
        OrganizationService::new(store.clone())
            .delete(&f.owner_token, false, &f.org, f.now)
            .await
            .is_ok()
    );
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.get(Model::Organization, &f.org).await.unwrap().is_none());
    tx.commit().await.unwrap();
}
#[cfg(feature = "http")]
async fn http_disabled<S: AuthStore + 'static>(
    store: Arc<CountStore<S>>,
    begins: Arc<AtomicUsize>,
    f: &Fixture,
) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use tower::ServiceExt;
    for (payload, received) in [
        (
            json!({"organizationId":f.org,"disableOrganizationDeletion":false}),
            None,
        ),
        (json!({"organizationId":42}), Some("number")),
        (json!({"organizationId":null}), Some("null")),
        (json!({}), Some("undefined")),
        (json!({"organizationId":""}), None),
    ] {
        for authenticated in [false, true] {
            let now = f.now;
            let app = crate::http::AuthHttp::new(
                store.clone(),
                "http://localhost:3000".into(),
                vec![7; 32],
                vec![],
            )
            .unwrap()
            .with_organization_controls(OrganizationControls::new(true, Some(0)))
            .with_team_policy(crate::organizations::TeamPolicy::default())
            .with_clock(move || now)
            .with_rate_limiting(false);
            let cookie = (crate::cookies::SessionCookie { secure: false }).serialize(
                &f.owner_token,
                &[7; 32],
                true,
            );
            let mut req = Request::builder()
                .method("POST")
                .uri("/organization/delete")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, "http://localhost:3000");
            if authenticated {
                req = req.header(header::COOKIE, cookie.split(';').next().unwrap());
            }
            let req = req.body(Body::from(payload.to_string())).unwrap();
            let response = app.router().oneshot(req).await.unwrap();
            assert_eq!(
                response.status().as_u16(),
                if received.is_some() { 400 } else { 404 }
            );
            assert!(!response.headers().contains_key(header::SET_COOKIE));
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap())
                    .unwrap();
            if let Some(received) = received {
                assert_eq!(
                    body,
                    json!({"code":"VALIDATION_ERROR","message":format!("[body.organizationId] Invalid input: expected string, received {received}")})
                );
            } else {
                assert_eq!(body["code"], "ORGANIZATION_DELETION_DISABLED");
            }
            assert_eq!(begins.load(Ordering::SeqCst), 0);
        }
    }
}
#[cfg(feature = "http")]
async fn http_quota<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use tower::ServiceExt;
    let f = setup(store.as_ref(), "http-quota").await;
    let now = f.now;
    let app = crate::http::AuthHttp::new(
        store.clone(),
        "http://localhost:3000".into(),
        vec![7; 32],
        vec![],
    )
    .unwrap()
    .with_organization_controls(OrganizationControls::new(false, Some(0)))
    .with_team_policy(crate::organizations::TeamPolicy::default())
    .with_clock(move || now)
    .with_rate_limiting(false);
    let cookie =
        (crate::cookies::SessionCookie { secure: false }).serialize(&f.owner_token, &[7; 32], true);
    let req=Request::builder().method("POST").uri("/organization/create-role").header(header::CONTENT_TYPE,"application/json").header(header::ORIGIN,"http://localhost:3000").header(header::COOKIE,cookie.split(';').next().unwrap()).body(Body::from(json!({"organizationId":f.org,"role":"new","permission":{},"maximumRolesPerOrganization":999}).to_string())).unwrap();
    let response = app.router().oneshot(req).await.unwrap();
    assert_eq!(response.status().as_u16(), 400);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(
        body,
        json!({"code":"TOO_MANY_ROLES","message":"This organization has too many roles"})
    );
}

#[tokio::test]
async fn organization_controls_presession_deletion_and_atomic_role_quota() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}
