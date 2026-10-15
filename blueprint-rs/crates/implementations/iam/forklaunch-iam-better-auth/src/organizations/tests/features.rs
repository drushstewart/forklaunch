use super::invitation_policy::{Fixture, request};
use super::team_policy::{records, setup, team};
use super::*;
use crate::organizations::{
    AddMemberInput, MemberRoles, OrganizationAccessControl, OrganizationFeatures,
};
fn control() -> OrganizationAccessControl {
    OrganizationAccessControl::new(
        std::collections::BTreeMap::from([("platform".into(), vec!["custom".into()])]),
        std::collections::BTreeMap::from([(
            "owner".into(),
            std::collections::BTreeMap::from([
                ("invitation".into(), vec!["create".into(), "cancel".into()]),
                ("member".into(), vec!["update".into(), "delete".into()]),
                (
                    "organization".into(),
                    vec!["update".into(), "delete".into()],
                ),
            ]),
        )]),
    )
    .unwrap()
}
#[allow(clippy::too_many_lines)]
pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    for teams in [true, false] {
        for dynamic in [true, false] {
            let f = setup(store.as_ref(), &format!("features-{teams}-{dynamic}")).await;
            let feature = OrganizationFeatures::new(teams, dynamic);
            let svc = OrganizationService::new(store.clone());
            let svc = if teams {
                svc.with_organization_features(feature)
                    .with_access_control(control())
            } else {
                svc.with_access_control(control())
                    .with_organization_features(feature)
            };
            let svc = if teams {
                svc
            } else {
                svc.with_team_member_limit_policy(Arc::new(DisabledSeat))
            };
            let id = format!("{}-legacy", f.org);
            team(store.as_ref(), &f, &id, 0).await;
            let mut tx = store.begin(true).await.unwrap();
            let mut grant = tx
                .get(Model::OrganizationRole, &format!("{}-grant", f.org))
                .await
                .unwrap()
                .unwrap();
            grant["permission"] = json!("{\"platform\":[\"custom\"]}");
            tx.replace(Model::OrganizationRole, grant).await.unwrap();
            let mut session = tx
                .find_unique(Model::Session, "token", &f.owner_token)
                .await
                .unwrap()
                .unwrap();
            session["activeTeamId"] = json!(id);
            tx.replace(Model::Session, session).await.unwrap();
            tx.commit().await.unwrap();
            let granted = svc
                .has_permission(
                    &f.owner_token,
                    false,
                    &json!({"organizationId":f.org,"permissions":{"platform":["custom"]}}),
                    f.now,
                )
                .await
                .unwrap();
            assert_eq!(granted["success"], dynamic);
            let full = svc
                .full(&f.owner_token, false, Some(&f.org), None, None, f.now)
                .await
                .unwrap();
            assert_eq!(full.get("teams").is_some(), teams);
            let created = svc
                .create(
                    &f.owner_token,
                    false,
                    &json!({"name":"Created","slug":format!("{}-created",f.org)}),
                    f.now,
                )
                .await
                .unwrap();
            assert_eq!(
                records(
                    store.as_ref(),
                    Model::Team,
                    "organizationId",
                    created["id"].as_str().unwrap()
                )
                .await
                .len(),
                usize::from(teams)
            );
            let mut body = request(&f);
            body["teamId"] = json!("nonexistent");
            let invitation = svc
                .create_invitation(&f.owner_token, false, &body, f.now)
                .await;
            if teams {
                assert_eq!(invitation.unwrap_err().code, "TEAM_NOT_FOUND");
            } else {
                assert!(invitation.unwrap().get("teamId").is_none());
            }
            let error = svc
                .add_member(
                    None,
                    false,
                    AddMemberInput {
                        organization_id: Some(f.org.clone()),
                        user_id: f.owner.clone(),
                        team_id: Some("missing".into()),
                        role: MemberRoles::Role("member".into()),
                    },
                    f.now,
                )
                .await
                .unwrap_err();
            assert_eq!(
                error.code,
                if teams {
                    "USER_IS_ALREADY_A_MEMBER_OF_THIS_ORGANIZATION"
                } else {
                    "HTTP_MESSAGE_ONLY"
                }
            );
            if !teams {
                assert_eq!(error.message, "Teams are not enabled");
                assert_eq!(
                    svc.create_team("", false, &json!({}), f.now)
                        .await
                        .unwrap_err()
                        .code,
                    "ORGANIZATION_FEATURE_DISABLED"
                );
            }
            if !dynamic {
                assert_eq!(
                    svc.create_organization_role("", false, &json!({}), f.now)
                        .await
                        .unwrap_err()
                        .code,
                    "ORGANIZATION_FEATURE_DISABLED"
                );
            }
            dynamic_role(store.clone(), &svc, &f, dynamic).await;
            legacy_invitation(store.clone(), &svc, &f, &id, teams).await;
            #[cfg(feature = "http")]
            http::exercise(store.clone(), &f, feature).await;
        }
    }
}
async fn legacy_invitation<S: AuthStore + 'static>(
    store: Arc<S>,
    svc: &OrganizationService<S>,
    f: &Fixture,
    id: &str,
    teams: bool,
) {
    let invitation = format!("{}-legacy-invite", f.org);
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Invitation,json!({"id":invitation,"organizationId":f.org,"inviterId":f.owner,"email":format!("{}@example.test",f.recipient),"role":"member","teamId":id,"status":"pending","expiresAt":timestamp(f.now+chrono::Duration::hours(1)),"createdAt":timestamp(f.now)})).await.unwrap();
    tx.commit().await.unwrap();
    let get = svc
        .get_invitation(&f.recipient_token, false, &invitation, f.now)
        .await
        .unwrap();
    assert_eq!(get.get("teamId").is_some(), teams);
    let accepted = svc
        .accept_invitation(&f.recipient_token, false, &invitation, f.now)
        .await
        .unwrap();
    assert_eq!(accepted.body["invitation"].get("teamId").is_some(), teams);
    assert_eq!(
        records(store.as_ref(), Model::TeamMember, "teamId", id)
            .await
            .len(),
        usize::from(teams)
    );
    let mut tx = store.begin(true).await.unwrap();
    assert_eq!(
        tx.get(Model::Invitation, &invitation)
            .await
            .unwrap()
            .unwrap()["teamId"],
        id
    );
    if !teams {
        tx.insert(Model::TeamMember,json!({"id":format!("{id}-pair"),"teamId":id,"userId":f.recipient,"createdAt":timestamp(f.now)})).await.unwrap();
    }
    tx.commit().await.unwrap();
    svc.remove_member(
        &f.owner_token,
        false,
        &json!({"organizationId":f.org,"memberIdOrEmail":accepted.body["member"]["id"]}),
        f.now,
    )
    .await
    .unwrap();
    assert_eq!(
        records(store.as_ref(), Model::TeamMember, "teamId", id)
            .await
            .len(),
        usize::from(!teams)
    );
    let on = OrganizationService::new(store.clone());
    let list = on
        .list_invitations(&f.owner_token, false, Some(&f.org), f.now)
        .await
        .unwrap();
    assert_eq!(
        list.as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == invitation)
            .unwrap()["teamId"],
        id
    );
}
#[tokio::test]
async fn organization_feature_topology_schema_and_stored_state() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}
#[cfg(feature = "http")]
mod http;

async fn dynamic_role<S: AuthStore>(
    store: Arc<S>,
    svc: &OrganizationService<S>,
    f: &Fixture,
    enabled: bool,
) {
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":format!("{}-custom",f.org),"organizationId":f.org,"role":"custom","permission":"{}","createdAt":timestamp(f.now)})).await.unwrap();
    tx.commit().await.unwrap();
    let mut body = request(f);
    body["role"] = json!("custom");
    body["email"] = json!(format!("{}-custom@example.test", f.org));
    let result = svc
        .create_invitation(&f.owner_token, false, &body, f.now)
        .await;
    if enabled {
        assert_eq!(result.unwrap()["role"], "custom");
    } else {
        let e = result.unwrap_err();
        assert_eq!(e.code, "HTTP_MESSAGE_ONLY");
        assert_eq!(e.message, "ROLE_NOT_FOUND: custom");
    }
}

struct DisabledSeat;
impl crate::organizations::TeamMemberLimitPolicy for DisabledSeat {
    fn evaluate<'a>(
        &'a self,
        _: crate::organizations::TeamMemberLimitContext<'a>,
    ) -> crate::organizations::TeamLimitFuture<'a> {
        panic!("disabled team callbacks must not execute")
    }
}
