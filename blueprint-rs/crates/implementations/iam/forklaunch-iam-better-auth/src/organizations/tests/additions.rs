use super::*;
use crate::organizations::{AddMemberInput, MemberRoles};

pub(super) async fn exercise_additions<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let session = SessionService::new(store.clone())
        .create("org-native-owner", "", "", now)
        .await
        .unwrap();
    let token = session["token"].as_str().unwrap();
    let service = OrganizationService::new(store.clone());
    let organization = service
        .create(
            token,
            false,
            &json!({"name":"Add Race","slug":"native-add-race"}),
            now,
        )
        .await
        .unwrap();
    let id = organization["id"].as_str().unwrap();
    let graph = service
        .full(token, false, None, None, None, now)
        .await
        .unwrap();
    let team = graph["teams"][0]["id"].as_str().unwrap();
    let input = addition(id, Some(team), "org-native-foreign");
    let (left, right) = tokio::join!(
        service.add_member(None, false, input.clone(), now),
        service.add_member(None, false, input, now)
    );
    assert_ne!(left.is_ok(), right.is_ok());
    assert_eq!(
        left.err().or_else(|| right.err()).unwrap().code,
        "USER_IS_ALREADY_A_MEMBER_OF_THIS_ORGANIZATION"
    );
    let graph = service
        .full(token, false, None, None, None, now)
        .await
        .unwrap();
    assert_eq!(graph["members"].as_array().unwrap().len(), 2);
    assert_eq!(graph["teams"][0]["memberCount"], 2);
    let mut tx = store.begin(false).await.unwrap();
    let rows = tx.list(Model::TeamMember).await.unwrap();
    assert_eq!(rows.iter().filter(|row| row["teamId"] == team).count(), 2);
    let member = rows
        .iter()
        .find(|row| row["teamId"] == team && row["userId"] == "org-native-foreign")
        .unwrap();
    let pair = serde_json::to_vec(&json!([team, "org-native-foreign"])).unwrap();
    assert_eq!(
        member["membershipKey"],
        URL_SAFE_NO_PAD.encode(ring::digest::digest(&ring::digest::SHA256, &pair).as_ref())
    );
    tx.commit().await.unwrap();
    exercise_limit(&service, store.as_ref(), token, now).await;
}

fn addition(id: &str, team: Option<&str>, user: &str) -> AddMemberInput {
    AddMemberInput {
        user_id: user.into(),
        role: MemberRoles::Roles(vec!["member".into(), " viewer ".into(), "member".into()]),
        organization_id: Some(id.into()),
        team_id: team.map(str::to_owned),
    }
}

async fn exercise_limit<S: AuthStore>(
    service: &OrganizationService<S>,
    store: &S,
    token: &str,
    now: DateTime<Utc>,
) {
    let organization = service
        .create(
            token,
            false,
            &json!({"name":"Limit Race","slug":"native-limit-race"}),
            now,
        )
        .await
        .unwrap();
    let id = organization["id"].as_str().unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User,json!({"id":"native-limit-candidate","email":"limit-candidate@fixture.test","name":"Limit Candidate"})).await.unwrap();
    for i in 0..98 {
        tx.insert(Model::Member,json!({"id":format!("native-limit-member-{i}"),"organizationId":id,"userId":format!("native-limit-user-{i}"),"role":"member","createdAt":now})).await.unwrap();
    }
    tx.commit().await.unwrap();
    let (left, right) = tokio::join!(
        service.add_member(None, false, addition(id, None, "org-native-foreign"), now),
        service.add_member(
            None,
            false,
            addition(id, None, "native-limit-candidate"),
            now
        )
    );
    assert_ne!(left.is_ok(), right.is_ok());
    assert_eq!(
        left.err().or_else(|| right.err()).unwrap().code,
        "ORGANIZATION_MEMBERSHIP_LIMIT_REACHED"
    );
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.list(Model::Member)
            .await
            .unwrap()
            .iter()
            .filter(|row| row["organizationId"] == id)
            .count(),
        100
    );
    tx.commit().await.unwrap();
}
