use super::*;

pub(super) async fn exercise_members<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let sessions = SessionService::new(store.clone());
    let left = sessions
        .create("org-native-owner", "", "", now)
        .await
        .unwrap();
    let right = sessions
        .create("org-native-foreign", "", "", now)
        .await
        .unwrap();
    let left_token = left["token"].as_str().unwrap();
    let right_token = right["token"].as_str().unwrap();
    let service = OrganizationService::new(store.clone());
    let organization = service
        .create(
            left_token,
            false,
            &json!({"name":"Member Race","slug":"native-member-race"}),
            now,
        )
        .await
        .unwrap();
    let id = organization["id"].as_str().unwrap();
    let graph = service
        .full(left_token, false, None, None, None, now)
        .await
        .unwrap();
    let team = graph["teams"][0]["id"].as_str().unwrap();
    seed_second_owner(store.as_ref(), id, team, now).await;
    service
        .set_active(right_token, false, &json!({"organizationId":id}), now)
        .await
        .unwrap();
    let member = service
        .active_member(right_token, false, now)
        .await
        .unwrap();
    assert_eq!(member["user"]["email"], "org-native-foreign@fixture.test");
    assert!(member["user"].get("privateField").is_none());
    let updated = service
        .update_member_role(
            left_token,
            false,
            &json!({"memberId":"race-second-owner","role":["owner", "viewer", "viewer"]}),
            now,
        )
        .await
        .unwrap();
    assert_eq!(updated["role"], "owner,viewer,viewer");
    assert!(updated.get("user").is_none());
    let (left_result, right_result) = tokio::join!(
        service.leave(left_token, false, id, now),
        service.leave(right_token, false, id, now)
    );
    assert_ne!(left_result.is_ok(), right_result.is_ok());
    let error = left_result
        .as_ref()
        .err()
        .or_else(|| right_result.as_ref().err())
        .unwrap();
    assert_eq!(
        error.code,
        "YOU_CANNOT_LEAVE_THE_ORGANIZATION_AS_THE_ONLY_OWNER"
    );
    let (departed, survivor, user) = if left_result.is_ok() {
        (left_token, right_token, "org-native-owner")
    } else {
        (right_token, left_token, "org-native-foreign")
    };
    let departed_view = sessions
        .get(departed, false, false, now)
        .await
        .unwrap()
        .unwrap();
    assert!(departed_view.session["activeOrganizationId"].is_null());
    if left_result.is_ok() {
        assert_eq!(departed_view.session["activeTeamId"], team);
    }
    assert_eq!(
        service.active_member(survivor, false, now).await.unwrap()["organizationId"],
        id
    );
    assert_remaining(store.as_ref(), id, team, user).await;
}

async fn seed_second_owner<S: AuthStore>(store: &S, id: &str, team: &str, now: DateTime<Utc>) {
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Member, json!({"id":"race-second-owner","organizationId":id,"userId":"org-native-foreign","role":"owner","createdAt":now})).await.unwrap();
    tx.insert(Model::TeamMember, json!({"id":"race-second-team-member","teamId":team,"userId":"org-native-foreign","membershipKey":"native-member-race-pair","createdAt":now})).await.unwrap();
    let mut row = tx.get(Model::Team, team).await.unwrap().unwrap();
    row["memberCount"] = json!(2);
    tx.replace(Model::Team, row).await.unwrap();
    tx.commit().await.unwrap();
}

async fn assert_remaining<S: AuthStore>(store: &S, id: &str, team: &str, user: &str) {
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::Team, team).await.unwrap().unwrap()["memberCount"],
        1
    );
    assert!(
        tx.list(Model::TeamMember)
            .await
            .unwrap()
            .iter()
            .all(|row| row["teamId"] != team || row["userId"] != user)
    );
    assert_eq!(
        tx.list(Model::Member)
            .await
            .unwrap()
            .iter()
            .filter(|row| row["organizationId"] == id)
            .count(),
        1
    );
    tx.commit().await.unwrap();
}
