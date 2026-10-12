mod mutations;
use super::*;
pub(super) async fn exercise<S: AuthStore>(store: Arc<S>) {
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
            &json!({"name":"Dynamic Roles","slug":"dynamic-roles"}),
            now,
        )
        .await
        .unwrap();
    let org = organization["id"].as_str().unwrap();
    assert_eq!(
        service
            .list_organization_roles(token, false, None, now)
            .await
            .unwrap_err()
            .code,
        "YOU_ARE_NOT_ALLOWED_TO_LIST_A_ROLE"
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"native-role-read-grant","organizationId":org,"role":"owner","permission":"{\"ac\":[\"read\",\"create\",\"update\",\"delete\"]}","createdAt":timestamp(now)})).await.unwrap();
    tx.commit().await.unwrap();
    creations(&store, &service, token, now).await;
    mutations::exercise(&store, &service, token, org, now).await;
    let mut tx = store.begin(true).await.unwrap();
    for i in 0..105 {
        tx.insert(Model::OrganizationRole,json!({"id":format!("native-role-{i}"),"organizationId":org,"role":format!("MiXeD-{i}"),"permission":if i==104 {"{"} else {"{\"platform\":[\"platform:read\"]}"},"createdAt":timestamp(now)})).await.unwrap();
    }
    tx.commit().await.unwrap();
    assert_eq!(
        service
            .list_organization_roles(token, false, None, now)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        100
    );
    let query = json!({"roleName":"MiXeD-103","roleId":"native-role-1"});
    let role = service
        .get_organization_role(token, false, &query, now)
        .await
        .unwrap();
    assert_eq!(role["id"], "native-role-103");
    assert_eq!(role["permission"], json!({"platform":["platform:read"]}));
    assert_eq!(
        service
            .get_organization_role(token, false, &json!({"roleName":"mixed-103"}), now)
            .await
            .unwrap_err()
            .code,
        "ROLE_NOT_FOUND"
    );
    assert_eq!(
        service
            .get_organization_role(token, false, &json!({"roleId":"native-role-104"}), now)
            .await
            .unwrap_err()
            .code,
        "HTTP_INTERNAL_EMPTY"
    );
    assert_eq!(
        service
            .list_organization_roles(token, false, Some(""), now)
            .await
            .unwrap_err()
            .code,
        "NO_ACTIVE_ORGANIZATION"
    );
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::OrganizationRole, "native-role-103")
            .await
            .unwrap()
            .unwrap()["permission"],
        "{\"platform\":[\"platform:read\"]}"
    );
    tx.commit().await.unwrap();
}

async fn creations<S: AuthStore>(
    store: &Arc<S>,
    service: &OrganizationService<S>,
    token: &str,
    now: DateTime<Utc>,
) {
    let request = json!({"role":"MiXeD-Created", "permission":{"platform":["platform:read","platform:write"]}});
    let (left, right) = tokio::join!(
        service.create_organization_role(token, false, &request, now),
        service.create_organization_role(token, false, &request, now)
    );
    let created = match (left, right) {
        (Ok(created), Err(error)) | (Err(error), Ok(created)) => {
            assert_eq!(error.code, "ROLE_NAME_IS_ALREADY_TAKEN");
            created
        }
        _ => panic!("one duplicate role creation must succeed"),
    };
    assert_eq!(created["roleData"]["role"], "mixed-created");
    assert_eq!(created["statements"], request["permission"]);
    let denied = service
        .create_organization_role(
            token,
            false,
            &json!({"role":"denied", "permission":{"platform":["bogus","bogus",""]}}),
            now,
        )
        .await
        .unwrap_err();
    assert_eq!(denied.code, "YOU_ARE_NOT_ALLOWED_TO_CREATE_A_ROLE");
    assert_eq!(
        denied.details["missingPermissions"],
        json!(["platform:bogus", "platform:bogus", "platform:"])
    );
    assert_eq!(
        service
            .create_organization_role(
                token,
                false,
                &json!({"role":"OWNER", "permission":{"team":[]}}),
                now
            )
            .await
            .unwrap_err()
            .code,
        "ROLE_NAME_IS_ALREADY_TAKEN"
    );
    let member_role = service
        .create_organization_role(
            token,
            false,
            &json!({"role":"member", "permission":{}}),
            now,
        )
        .await
        .unwrap();
    let mut tx = store.begin(false).await.unwrap();
    let persisted = tx
        .get(
            Model::OrganizationRole,
            created["roleData"]["id"].as_str().unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        persisted["permission"],
        crate::ecmascript::stringify(&request["permission"])
    );
    tx.commit().await.unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.delete(
        Model::OrganizationRole,
        created["roleData"]["id"].as_str().unwrap(),
    )
    .await
    .unwrap();
    tx.delete(
        Model::OrganizationRole,
        member_role["roleData"]["id"].as_str().unwrap(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}
