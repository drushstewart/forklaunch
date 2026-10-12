use super::*;
pub(super) async fn exercise<S: AuthStore>(
    store: Arc<S>,
    service: &OrganizationService<S>,
    org: &str,
    now: DateTime<Utc>,
) {
    let session = SessionService::new(store.clone())
        .create("org-native-foreign", "", "", now)
        .await
        .unwrap();
    let token = session["token"].as_str().unwrap();
    let mut tx = store.begin(true).await.unwrap();
    let mut user = tx
        .get(Model::User, "org-native-foreign")
        .await
        .unwrap()
        .unwrap();
    user["emailVerified"] = json!(true);
    tx.replace(Model::User, user).await.unwrap();
    let team = tx
        .list(Model::Team)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row["organizationId"] == org)
        .unwrap();
    let team_id = team["id"].as_str().unwrap();
    tx.insert(Model::Team,json!({"id":"native-accept-rollback-team","name":"Rollback","organizationId":org,"memberCount":0,"createdAt":timestamp(now)})).await.unwrap();
    for (id, teams) in [
        ("native-accept-bad", "native-accept-rollback-team,absent"),
        ("native-accept-good", team_id),
        ("native-accept-none", ""),
    ] {
        tx.insert(Model::Invitation,json!({"id":id,"email":"ORG-NATIVE-FOREIGN@fixture.test","role":"custom, unknown","organizationId":org,"inviterId":"org-native-owner","status":"pending","expiresAt":timestamp(now),"createdAt":timestamp(now),"teamId":teams})).await.unwrap();
    }
    tx.commit().await.unwrap();
    assert_eq!(
        service
            .accept_invitation(token, true, "native-accept-bad", now)
            .await
            .err()
            .unwrap()
            .code,
        "TEAM_NOT_FOUND"
    );
    rollback(&store, &session).await;
    let (a, b) = tokio::join!(
        service.accept_invitation(token, true, "native-accept-good", now),
        service.accept_invitation(token, true, "native-accept-good", now)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let (success, error) = if let Ok(success) = a {
        (success, b.err().unwrap())
    } else {
        (b.ok().unwrap(), a.err().unwrap())
    };
    assert!(success.cookie);
    assert_eq!(success.body["member"]["role"], "custom, unknown");
    assert_eq!(error.code, "INVITATION_NOT_FOUND");
    let none = service
        .accept_invitation(token, true, "native-accept-none", now)
        .await
        .unwrap();
    assert!(!none.cookie);
    let mut tx = store.begin(false).await.unwrap();
    let current = tx
        .get(Model::Session, session["id"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current["activeOrganizationId"], org);
    assert_eq!(current["activeTeamId"], team_id);
    assert_eq!(
        tx.list(Model::Member)
            .await
            .unwrap()
            .iter()
            .filter(|row| row["organizationId"] == org && row["userId"] == "org-native-foreign")
            .count(),
        2
    );
    assert_eq!(
        tx.get(Model::Team, team_id).await.unwrap().unwrap()["memberCount"],
        team["memberCount"].as_i64().unwrap() + 1
    );
    tx.commit().await.unwrap();
    admission(store, service, token, org, now).await;
}
async fn admission<S: AuthStore>(
    store: Arc<S>,
    service: &OrganizationService<S>,
    token: &str,
    org: &str,
    now: DateTime<Utc>,
) {
    let mut tx = store.begin(true).await.unwrap();
    let count = tx
        .list(Model::Member)
        .await
        .unwrap()
        .iter()
        .filter(|row| row["organizationId"] == org)
        .count();
    for i in count..99 {
        tx.insert(Model::Member,json!({"id":format!("native-accept-limit-member-{i}"),"organizationId":org,"userId":"org-native-owner","role":"member","createdAt":timestamp(now)})).await.unwrap();
    }
    for id in ["native-accept-slot-a", "native-accept-slot-b"] {
        tx.insert(Model::Invitation,json!({"id":id,"email":"org-native-foreign@fixture.test","role":"member","organizationId":org,"inviterId":"org-native-owner","status":"pending","expiresAt":timestamp(now),"createdAt":timestamp(now),"teamId":null})).await.unwrap();
    }
    tx.commit().await.unwrap();
    let (a, b) = tokio::join!(
        service.accept_invitation(token, false, "native-accept-slot-a", now),
        service.accept_invitation(token, false, "native-accept-slot-b", now)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(
        a.err().or_else(|| b.err()).unwrap().code,
        "ORGANIZATION_MEMBERSHIP_LIMIT_REACHED"
    );
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.list(Model::Member)
            .await
            .unwrap()
            .iter()
            .filter(|row| row["organizationId"] == org)
            .count(),
        100
    );
    let states = tx
        .list(Model::Invitation)
        .await
        .unwrap()
        .into_iter()
        .filter(|row| row["id"] == "native-accept-slot-a" || row["id"] == "native-accept-slot-b")
        .collect::<Vec<_>>();
    assert_eq!(
        states
            .iter()
            .filter(|row| row["status"] == "accepted")
            .count(),
        1
    );
    assert_eq!(
        states
            .iter()
            .filter(|row| row["status"] == "pending")
            .count(),
        1
    );
    tx.commit().await.unwrap();
}

async fn rollback<S: AuthStore>(store: &Arc<S>, session: &Value) {
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::Invitation, "native-accept-bad")
            .await
            .unwrap()
            .unwrap()["status"],
        "pending"
    );
    assert_eq!(
        tx.get(Model::Team, "native-accept-rollback-team")
            .await
            .unwrap()
            .unwrap()["memberCount"],
        0
    );
    assert!(
        !tx.list(Model::TeamMember)
            .await
            .unwrap()
            .iter()
            .any(|row| row["teamId"] == "native-accept-rollback-team")
    );
    assert_eq!(
        tx.get(Model::Session, session["id"].as_str().unwrap())
            .await
            .unwrap()
            .unwrap(),
        *session
    );
    tx.commit().await.unwrap();
}
