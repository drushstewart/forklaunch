use super::*;
mod additions;
pub(crate) mod controls;
mod creation_policy;
pub(crate) mod invitation_cancel;
pub(crate) mod invitation_delivery;
pub(crate) mod invitation_policy;
mod invitations;
mod listing;
mod members;
mod permissions;
pub(crate) mod policy;
mod roles;
mod team_members;
mod team_mutations;
pub(crate) mod team_policy;
mod teams;
mod window;

pub(crate) async fn exercise_organizations<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    seed_owners(store.as_ref()).await;
    let sessions = SessionService::new(store.clone());
    let session = sessions
        .create("org-native-owner", "", "", now)
        .await
        .unwrap();
    let token = session["token"].as_str().unwrap();
    let foreign = sessions
        .create("org-native-foreign", "", "", now)
        .await
        .unwrap();
    let foreign_token = foreign["token"].as_str().unwrap();
    let service = OrganizationService::new(store.clone());
    let body = json!({"name":"Native Org","slug":"native-org","metadata":{"source":"fixture"},"userId":"org-native-foreign"});
    let (left, right) = tokio::join!(
        service.create(token, false, &body, now),
        service.create(token, false, &body, now)
    );
    let organization = match (left, right) {
        (Ok(organization), Err(error)) | (Err(error), Ok(organization)) => {
            assert_eq!(error.code, "ORGANIZATION_ALREADY_EXISTS");
            organization
        }
        _ => panic!("one organization creation must succeed"),
    };
    assert_eq!(organization["members"][0]["userId"], "org-native-owner");
    assert_eq!(organization["subscription"], "free");
    assert_eq!(organization["metadata"], body["metadata"]);
    assert_eq!(
        service
            .list(token, false, now)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        service
            .list(foreign_token, false, now)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    let graph = service
        .full(token, false, None, None, None, now)
        .await
        .unwrap();
    assert_eq!(graph["metadata"], "{\"source\":\"fixture\"}");
    assert_eq!(graph["teams"].as_array().unwrap().len(), 1);
    assert_eq!(graph["teams"][0]["memberCount"], 1);
    assert!(graph["members"][0]["user"].get("privateField").is_none());
    let mut tx = store.begin(false).await.unwrap();
    let memberships = tx.list(Model::TeamMember).await.unwrap();
    let membership = memberships
        .iter()
        .find(|row| row["userId"] == "org-native-owner")
        .unwrap();
    let pair = serde_json::to_vec(&json!([membership["teamId"], membership["userId"]])).unwrap();
    assert_eq!(
        membership["membershipKey"],
        URL_SAFE_NO_PAD.encode(ring::digest::digest(&ring::digest::SHA256, &pair).as_ref())
    );
    tx.commit().await.unwrap();
    exercise_selection(
        &service,
        &sessions,
        token,
        foreign_token,
        &organization,
        now,
    )
    .await;
    sessions.sign_out(token).await.unwrap();
    assert_eq!(
        service
            .create(token, false, &body, now)
            .await
            .unwrap_err()
            .code,
        "UNAUTHORIZED"
    );
    permissions::exercise_permissions(store.clone()).await;
    members::exercise_members(store.clone()).await;
    additions::exercise_additions(store.clone()).await;
    listing::exercise_listing(store.clone()).await;
    teams::exercise_teams(store.clone()).await;
    team_mutations::exercise_mutations(store.clone()).await;
    team_members::exercise_memberships(store.clone()).await;
    invitations::exercise(store.clone()).await;
    roles::exercise(store).await;
}

async fn exercise_selection<S: AuthStore>(
    service: &OrganizationService<S>,
    sessions: &SessionService<S>,
    token: &str,
    foreign: &str,
    organization: &Value,
    now: DateTime<Utc>,
) {
    let initial = sessions
        .get(token, false, false, now)
        .await
        .unwrap()
        .unwrap();
    let team = initial.session["activeTeamId"].clone();
    assert_eq!(initial.session["activeOrganizationId"], organization["id"]);
    assert_eq!(
        service
            .set_active(token, false, &json!({"organizationId":"unknown"}), now)
            .await
            .err()
            .unwrap()
            .code,
        "USER_IS_NOT_A_MEMBER_OF_THE_ORGANIZATION"
    );
    let view = sessions
        .get(token, false, false, now)
        .await
        .unwrap()
        .unwrap();
    assert!(view.session["activeOrganizationId"].is_null());
    assert_eq!(view.session["activeTeamId"], team);
    assert_eq!(
        service
            .full(foreign, false, organization["id"].as_str(), None, None, now)
            .await
            .unwrap_err()
            .code,
        "USER_IS_NOT_A_MEMBER_OF_THE_ORGANIZATION"
    );
    let selected = service
        .set_active(token, false, &json!({"organizationSlug":"native-org"}), now)
        .await
        .unwrap();
    assert!(selected.cookie);
    assert_eq!(selected.body["id"], organization["id"]);
    assert!(
        service
            .set_active(token, false, &json!({"organizationId":null}), now)
            .await
            .unwrap()
            .cookie
    );
    assert!(
        !service
            .set_active(token, false, &json!({"organizationId":null}), now)
            .await
            .unwrap()
            .cookie
    );
    assert!(
        service
            .full(token, false, None, None, None, now)
            .await
            .unwrap()
            .is_null()
    );
}

#[tokio::test]
async fn native_organization_creation_graph_selection_and_atomic_slug() {
    exercise_organizations(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}

async fn seed_owners<S: AuthStore>(store: &S) {
    let mut tx = store.begin(true).await.unwrap();
    for (id, email) in [
        ("org-native-owner", "org-native@fixture.test"),
        ("org-native-foreign", "org-native-foreign@fixture.test"),
    ] {
        tx.insert(
            Model::User,
            json!({"id":id,"email":email,"name":"Native Org Person","privateField":"not-public"}),
        )
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
}

pub(crate) mod creator_role;

pub(crate) mod access_control;

#[cfg(feature = "postgres")]
pub(crate) mod postgres;

mod authorizers;
