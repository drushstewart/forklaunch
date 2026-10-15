//! Transaction-local organization session expiry, independent of the initial lookup.
use super::*;
use crate::organizations::OrganizationService;

async fn snapshot<S: AuthStore>(store: &S) -> Vec<Vec<Value>> {
    let mut tx = store.begin(false).await.unwrap();
    let mut result = Vec::new();
    for model in [
        Model::User,
        Model::Session,
        Model::Organization,
        Model::Member,
        Model::Team,
        Model::TeamMember,
        Model::Invitation,
        Model::OrganizationRole,
    ] {
        result.push(tx.list(model).await.unwrap());
    }
    tx.commit().await.unwrap();
    result
}

async fn operation<S: AuthStore>(
    service: &OrganizationService<S>,
    case: usize,
    token: &str,
    now: DateTime<Utc>,
) -> BlueprintResult<()> {
    match case {
        0 => service.create(token,false,&json!({"name":"Late","slug":"clock-org-late"}),now).await.map(|_|()),
        1 => service.set_active(token,false,&json!({"organizationId":null}),now).await.map(|_|()),
        2 => service.full(token,false,Some("clock-org-existing"),None,None,now).await.map(|_|()),
        3 => service.list_organization_roles(token,false,Some("clock-org-existing"),now).await.map(|_|()),
        4 => service.reject_invitation(token,false,"clock-org-invite",now).await.map(|_|()),
        5 => service.create_team(token,false,&json!({"organizationId":"clock-org-existing","name":"Late"}),now).await.map(|_|()),
        6 => service.remove_member(token,false,&json!({"organizationId":"clock-org-existing","memberIdOrEmail":"clock-org-member"}),now).await.map(|_|()),
        _ => unreachable!(),
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) async fn lifecycle<S: AuthStore + 'static>(inner: Arc<S>) {
    let start = 1_800_000_000_000;
    let now = DateTime::from_timestamp_millis(start).unwrap();
    let clock = Arc::new(AtomicI64::new(start));
    let advance = Advance {
        clock: clock.clone(),
        armed: Arc::new(std::sync::Mutex::new(None)),
    };
    let store = Arc::new(Store {
        inner: inner.clone(),
        advance: advance.clone(),
    });
    let policy = crate::sessions::SessionPolicy::new(120, 30, 60, true).unwrap();
    let mut tx = inner.begin(true).await.unwrap();
    tx.insert(Model::User,json!({"id":"clock-org-owner","email":"clock-org-owner@example.test","name":"Clock Org","emailVerified":true})).await.unwrap();
    tx.commit().await.unwrap();
    let session = SessionService::new(inner.clone())
        .with_session_policy(policy)
        .create("clock-org-owner", "", "", now)
        .await
        .unwrap();
    let token = session["token"].as_str().unwrap();
    let explicit = OrganizationService::new(store.clone()).with_session_policy(policy);
    let service = OrganizationService::new(store.clone())
        .with_session_policy(policy)
        .with_time_source(crate::time_source::TimeSource::http({
            let clock = clock.clone();
            Arc::new(move || DateTime::from_timestamp_millis(clock.load(Ordering::SeqCst)).unwrap())
        }));
    // Seed a real graph; subsequent failed operations must preserve every row.
    explicit
        .create(
            token,
            false,
            &json!({"name":"Clock Org","slug":"clock-org-initial"}),
            now,
        )
        .await
        .unwrap();
    let baseline = snapshot(inner.as_ref()).await;
    for event in [
        Event::Begin(true),
        Event::Find(Model::Session),
        Event::Get(Model::User),
    ] {
        for case in 0..7 {
            clock.store(start, Ordering::SeqCst);
            // Skip the initial session check. Advance only the operation's own
            // transaction acquisition/session lookup/user lookup.
            advance.arm_after(event, start + 120_001, 1);
            let error = operation(&service, case, token, now).await.unwrap_err();
            assert_eq!(error.code, "UNAUTHORIZED", "case {case}");
            assert_eq!(snapshot(inner.as_ref()).await, baseline, "case {case}");
        }
    }
    // Direct domain APIs continue honoring explicit now even if storage work
    // advances the unrelated wall clock beyond expiry.
    clock.store(start, Ordering::SeqCst);
    advance.arm_after(Event::Get(Model::User), start + 120_001, 1);
    explicit
        .create(
            token,
            false,
            &json!({"name":"Explicit","slug":"clock-org-explicit"}),
            now,
        )
        .await
        .unwrap();
    // Existing session semantics admit exact expiry equality at the fresh check.
    clock.store(start, Ordering::SeqCst);
    advance.arm_after(Event::Get(Model::User), start + 120_000, 1);
    service
        .create(
            token,
            false,
            &json!({"name":"Equality","slug":"clock-org-equality"}),
            now,
        )
        .await
        .unwrap();
    let baseline = snapshot(inner.as_ref()).await;
    clock.store(start, Ordering::SeqCst);
    let app = AuthHttp::new(store, "http://localhost:3000".into(), vec![7; 32], vec![])
        .unwrap()
        .with_rate_limiting(false)
        .with_session_policy(policy)
        .with_clock({
            let clock = clock.clone();
            move || DateTime::from_timestamp_millis(clock.load(Ordering::SeqCst)).unwrap()
        });
    let cookie = app.cookie.serialize(token, &app.secret, true);
    let app = app.router();
    // HTTP has its own route-level lookup before the domain service's initial
    // lookup; target the third User read, inside the operation transaction.
    advance.arm_after(Event::Get(Model::User), start + 120_001, 2);
    let request = Request::builder()
        .method("POST")
        .uri("/organization/create")
        .header("origin", "http://localhost:3000")
        .header("content-type", "application/json")
        .header("cookie", cookie)
        .body(Body::from(
            json!({"name":"Late HTTP","slug":"clock-org-http-late"}).to_string(),
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    // This route's existing unauthorized projection is deliberately empty.
    assert!(
        to_bytes(response.into_body(), 1024)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(snapshot(inner.as_ref()).await, baseline);
}

#[tokio::test]
async fn organization_clock_secondary_checks_reject_delays_without_mutation() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
