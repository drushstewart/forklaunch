use super::*;
use crate::organizations::{AddMemberInput, MemberRoles};
pub(super) async fn exercise_memberships<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let sessions = SessionService::new(store.clone());
    let owner = sessions
        .create("org-native-owner", "", "", now)
        .await
        .unwrap();
    let other = sessions
        .create("org-native-foreign", "", "", now)
        .await
        .unwrap();
    let token = owner["token"].as_str().unwrap();
    let foreign = other["token"].as_str().unwrap();
    let service = OrganizationService::new(store.clone());
    let org = service
        .create(
            token,
            false,
            &json!({"name":"Team Members","slug":"native-team-member-race"}),
            now,
        )
        .await
        .unwrap();
    let org = org["id"].as_str().unwrap();
    let teams = service
        .list_teams(token, false, &json!({}), now)
        .await
        .unwrap();
    let team = teams[0]["id"].as_str().unwrap();
    service
        .add_member(
            None,
            false,
            AddMemberInput {
                user_id: "org-native-foreign".into(),
                role: MemberRoles::Role("member".into()),
                organization_id: Some(org.into()),
                team_id: None,
            },
            now,
        )
        .await
        .unwrap();
    let body = json!({"teamId":team,"userId":"org-native-foreign"});
    assert_eq!(
        service
            .add_team_member(token, false, &body, now)
            .await
            .unwrap_err()
            .code,
        "YOU_ARE_NOT_ALLOWED_TO_CREATE_A_NEW_TEAM_MEMBER"
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"native-team-members-grant","organizationId":org,"role":"owner","permission":"{\"member\":[\"update\",\"delete\"]}"})).await.unwrap();
    tx.commit().await.unwrap();
    let (a, b) = tokio::join!(
        service.add_team_member(token, false, &body, now),
        service.add_team_member(token, false, &body, now)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a["id"], b["id"]);
    assert!(a.get("membershipKey").is_none());
    assert_count(store.as_ref(), team, 2).await;
    service
        .set_active(foreign, false, &json!({"organizationId":org}), now)
        .await
        .unwrap();
    service
        .set_active_team(foreign, false, &json!({"teamId":team}), now)
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        service.remove_team_member(token, false, &body, now),
        service.remove_team_member(token, false, &body, now)
    );
    assert_ne!(a.is_ok(), b.is_ok());
    assert_eq!(
        a.err().or_else(|| b.err()).unwrap().code,
        "USER_IS_NOT_A_MEMBER_OF_THE_TEAM"
    );
    assert_count(store.as_ref(), team, 1).await;
    assert_eq!(
        sessions
            .get(foreign, false, false, now)
            .await
            .unwrap()
            .unwrap()
            .session["activeTeamId"],
        team
    );
    assert_eq!(
        service
            .list_team_members(foreign, false, &json!({}), now)
            .await
            .unwrap_err()
            .code,
        "USER_IS_NOT_A_MEMBER_OF_THE_TEAM"
    );
}
async fn assert_count<S: AuthStore>(store: &S, team: &str, count: usize) {
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::Team, team).await.unwrap().unwrap()["memberCount"],
        json!(count)
    );
    assert_eq!(
        tx.list(Model::TeamMember)
            .await
            .unwrap()
            .iter()
            .filter(|row| row["teamId"] == team)
            .count(),
        count
    );
    tx.commit().await.unwrap();
}
