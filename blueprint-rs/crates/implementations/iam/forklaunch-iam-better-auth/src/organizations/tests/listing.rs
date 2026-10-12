use super::*;
use crate::organizations::{AddMemberInput, MemberRoles};

pub(super) async fn exercise_listing<S: AuthStore>(store: Arc<S>) {
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
            &json!({"name":"Native List","slug":"native-list"}),
            now,
        )
        .await
        .unwrap();
    let id = organization["id"].as_str().unwrap();
    seed_listing(store.as_ref(), &service, id, now).await;
    let page = service
        .list_members(token, false, &json!({"sortBy":"role","limit":"3"}), now)
        .await
        .unwrap();
    assert_eq!(page["total"], 4);
    let roles: Vec<_> = page["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| member["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["a", "A", "á"]);
    assert!(page["members"][0]["user"].get("privateField").is_none());
    let page = service.list_members(token, false, &json!({"organizationId":"unknown","organizationSlug":"native-list","sortBy":"role","limit":"-1","filterField":"role","filterValue":["A","a","á"],"filterOperator":"in"}), now).await.unwrap();
    assert_eq!(page["total"], 3);
    assert_eq!(page["members"].as_array().unwrap().len(), 2);
    assert_eq!(page["members"][1]["role"], "A");
    let page = service
        .list_members(token, false, &json!({"offset":"-1","limit":"1"}), now)
        .await
        .unwrap();
    assert_eq!(page["members"][0]["role"], "á");
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Member,json!({"id":"native-list-orphan","organizationId":id,"userId":"missing-list-user","role":"member","createdAt":now})).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        service
            .list_members(token, false, &json!({}), now)
            .await
            .unwrap_err()
            .code,
        "HTTP_INTERNAL_EMPTY"
    );
    let page = service
        .list_members(token, false, &json!({"limit":"-1"}), now)
        .await
        .unwrap();
    assert_eq!(page["total"], 5);
    assert_eq!(page["members"].as_array().unwrap().len(), 4);
}

async fn seed_listing<S: AuthStore>(
    store: &S,
    service: &OrganizationService<S>,
    id: &str,
    now: DateTime<Utc>,
) {
    let mut tx = store.begin(true).await.unwrap();
    for i in 0..3 {
        tx.insert(Model::User,json!({"id":format!("native-list-user-{i}"),"email":format!("list-{i}@fixture.test"),"name":"List User","privateField":"hidden"})).await.unwrap();
    }
    tx.commit().await.unwrap();
    for (i, role) in ["A", "a", "á"].into_iter().enumerate() {
        let member = service
            .add_member(
                None,
                false,
                AddMemberInput {
                    user_id: format!("native-list-user-{i}"),
                    role: MemberRoles::Role(role.into()),
                    organization_id: Some(id.into()),
                    team_id: None,
                },
                now,
            )
            .await
            .unwrap();
        assert_eq!(member["role"], role);
    }
}
