use super::*;

pub(super) async fn exercise_mutations<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let sessions = SessionService::new(store.clone());
    let owner = sessions
        .create("org-native-owner", "", "", now)
        .await
        .unwrap();
    let token = owner["token"].as_str().unwrap();
    let service = OrganizationService::new(store.clone());
    let org = service
        .create(
            token,
            false,
            &json!({"name":"Team Delete Race","slug":"native-team-delete-race"}),
            now,
        )
        .await
        .unwrap();
    let org = org["id"].as_str().unwrap();
    let teams = service
        .list_teams(token, false, &json!({}), now)
        .await
        .unwrap();
    let left = teams[0]["id"].as_str().unwrap();
    assert_eq!(
        service
            .create_team(token, false, &json!({"name":"Denied"}), now)
            .await
            .unwrap_err()
            .code,
        "YOU_ARE_NOT_ALLOWED_TO_CREATE_TEAMS_IN_THIS_ORGANIZATION"
    );
    grant(store.as_ref(), org).await;
    let right = service
        .create_team(
            token,
            false,
            &json!({"name":"Second","memberCount":999,"id":"injected"}),
            now,
        )
        .await
        .unwrap();
    let right = right["id"].as_str().unwrap();
    assert_ne!(right, "injected");
    let updated = service.update_team(token,false,&json!({"teamId":right,"data":{"name":"Renamed","organizationId":org,"memberCount":999}}),now+chrono::Duration::seconds(1)).await.unwrap();
    assert_eq!(updated["name"], "Renamed");
    assert!(updated.get("memberCount").is_none());
    assert_eq!(
        service
            .remove_team(token, false, &json!({"teamId":left}), now)
            .await
            .unwrap_err()
            .code,
        "YOU_ARE_NOT_ALLOWED_TO_DELETE_THIS_TEAM"
    );
    seed_invitations(store.as_ref(), org, left, right, now).await;
    service
        .set_active_team(token, false, &json!({"teamId":null}), now)
        .await
        .unwrap();
    let left_body = json!({"teamId":left});
    let right_body = json!({"teamId":right});
    let (a, b) = tokio::join!(
        service.remove_team(token, false, &left_body, now),
        service.remove_team(token, false, &right_body, now)
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let deleted = if a.is_ok() { left } else { right };
    assert_eq!(
        a.err().or_else(|| b.err()).unwrap().code,
        "UNABLE_TO_REMOVE_LAST_TEAM"
    );
    assert_cleanup(store.as_ref(), org, left, right, deleted).await;
}

async fn grant<S: AuthStore>(store: &S, org: &str) {
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"native-team-mutation-grant","organizationId":org,"role":"owner","permission":"{\"team\":[\"create\",\"update\",\"delete\"]}"})).await.unwrap();
    tx.commit().await.unwrap();
}
async fn seed_invitations<S: AuthStore>(
    store: &S,
    org: &str,
    left: &str,
    right: &str,
    now: DateTime<Utc>,
) {
    let mut tx = store.begin(true).await.unwrap();
    for (id, expiry) in [("live", now + chrono::Duration::hours(1)), ("expired", now)] {
        tx.insert(Model::Invitation,json!({"id":format!("native-team-invite-{id}"),"organizationId":org,"email":format!("team-{id}@fixture.test"),"status":"pending","expiresAt":expiry,"teamId":format!("{left},{right},{left}")})).await.unwrap();
    }
    tx.commit().await.unwrap();
}
async fn assert_cleanup<S: AuthStore>(
    store: &S,
    org: &str,
    left: &str,
    right: &str,
    deleted: &str,
) {
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.list(Model::Team)
            .await
            .unwrap()
            .iter()
            .filter(|row| row["organizationId"] == org)
            .count(),
        1
    );
    assert!(tx.get(Model::Team, deleted).await.unwrap().is_none());
    assert!(
        tx.list(Model::TeamMember)
            .await
            .unwrap()
            .iter()
            .all(|row| row["teamId"] != deleted)
    );
    let original = format!("{left},{right},{left}");
    let expected = original
        .split(',')
        .filter(|id| *id != deleted)
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        tx.get(Model::Invitation, "native-team-invite-live")
            .await
            .unwrap()
            .unwrap()["teamId"],
        expected
    );
    assert_eq!(
        tx.get(Model::Invitation, "native-team-invite-expired")
            .await
            .unwrap()
            .unwrap()["teamId"],
        original
    );
    tx.commit().await.unwrap();
}
