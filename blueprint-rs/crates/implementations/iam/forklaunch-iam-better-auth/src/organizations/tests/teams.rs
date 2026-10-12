use super::*;
use crate::organizations::{AddMemberInput, MemberRoles};

pub(super) async fn exercise_teams<S: AuthStore>(store: Arc<S>) {
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
    let other_token = other["token"].as_str().unwrap();
    let service = OrganizationService::new(store.clone());
    let org = service
        .create(
            token,
            false,
            &json!({"name":"Native Teams","slug":"native-teams"}),
            now,
        )
        .await
        .unwrap();
    let id = org["id"].as_str().unwrap();
    let teams = service
        .list_teams(token, false, &json!({}), now)
        .await
        .unwrap();
    let team = teams[0]["id"].as_str().unwrap();
    assert!(teams[0].get("memberCount").is_none());
    service
        .add_member(
            None,
            false,
            AddMemberInput {
                user_id: "org-native-foreign".into(),
                role: MemberRoles::Role("member".into()),
                organization_id: Some(id.into()),
                team_id: Some(team.into()),
            },
            now,
        )
        .await
        .unwrap();
    let members = service
        .list_team_members(token, false, &json!({}), now)
        .await
        .unwrap();
    assert_eq!(members.as_array().unwrap().len(), 2);
    assert!(members[0].get("membershipKey").is_none());
    let teams = service
        .list_user_teams(other_token, false, &json!({}), now)
        .await
        .unwrap();
    assert!(
        teams
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == team)
    );
    service
        .set_active(other_token, false, &json!({"organizationId":id}), now)
        .await
        .unwrap();
    let selected = service
        .set_active_team(other_token, true, &json!({"teamId":team}), now)
        .await
        .unwrap();
    assert!(selected.cookie);
    assert_eq!(selected.body["id"], team);
    assert!(selected.body.get("memberCount").is_none());
    assert!(
        service
            .set_active_team(other_token, true, &json!({}), now)
            .await
            .unwrap()
            .cookie
    );
    assert_eq!(
        service
            .list_user_teams(token, false, &json!({"userId":"org-native-foreign"}), now)
            .await
            .unwrap_err()
            .code,
        "YOU_ARE_NOT_ALLOWED_TO_UPDATE_THIS_MEMBER"
    );
    grant_and_read(store.as_ref(), &service, token, id, team, now).await;
    exercise_clear(&service, &sessions, other_token, now).await;
}

async fn grant_and_read<S: AuthStore>(
    store: &S,
    service: &OrganizationService<S>,
    token: &str,
    id: &str,
    team: &str,
    now: DateTime<Utc>,
) {
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"native-team-member-grant","organizationId":id,"role":"owner","permission":"{\"member\":[\"update\"]}"})).await.unwrap();
    tx.commit().await.unwrap();
    let teams = service
        .list_user_teams(token, false, &json!({"userId":"org-native-foreign"}), now)
        .await
        .unwrap();
    assert_eq!(teams.as_array().unwrap().len(), 1);
    assert_eq!(teams[0]["id"], team);
}

async fn exercise_clear<S: AuthStore>(
    service: &OrganizationService<S>,
    sessions: &SessionService<S>,
    token: &str,
    now: DateTime<Utc>,
) {
    service
        .set_active(token, false, &json!({"organizationId":null}), now)
        .await
        .unwrap();
    assert_eq!(
        service
            .list_team_members(token, false, &json!({}), now)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        service
            .set_active_team(token, false, &json!({}), now)
            .await
            .err()
            .unwrap()
            .code,
        "NO_ACTIVE_ORGANIZATION"
    );
    assert!(
        service
            .set_active_team(token, false, &json!({"teamId":null}), now)
            .await
            .unwrap()
            .cookie
    );
    assert!(
        !service
            .set_active_team(token, false, &json!({"teamId":null}), now)
            .await
            .unwrap()
            .cookie
    );
    let view = sessions
        .get(token, false, false, now)
        .await
        .unwrap()
        .unwrap();
    assert!(view.session["activeTeamId"].is_null());
    assert!(view.session["activeOrganizationId"].is_null());
}
