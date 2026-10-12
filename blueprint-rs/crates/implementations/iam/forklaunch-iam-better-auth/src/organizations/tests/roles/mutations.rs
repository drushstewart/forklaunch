use super::*;
pub(super) async fn exercise<S: AuthStore>(
    store: &Arc<S>,
    service: &OrganizationService<S>,
    token: &str,
    org: &str,
    now: DateTime<Utc>,
) {
    let created = service
        .create_organization_role(
            token,
            false,
            &json!({"role":"dynamic-mutable","permission":{"platform":["platform:read"]}}),
            now,
        )
        .await
        .unwrap();
    let id = created["roleData"]["id"].as_str().unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Member,json!({"id":"native-role-assignment","organizationId":org,"userId":"org-native-foreign","role":"viewer, dynamic-mutable ","createdAt":timestamp(now)})).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        service
            .delete_organization_role(token, false, &json!({"roleId":id}), now)
            .await
            .unwrap_err()
            .code,
        "ROLE_IS_ASSIGNED_TO_MEMBERS"
    );
    let denied = service
        .update_organization_role(
            token,
            false,
            &json!({"roleId":id,"data":{"permission":{"platform":["bogus","bogus"]}}}),
            now,
        )
        .await
        .unwrap_err();
    assert_eq!(
        denied.details["missingPermissions"],
        json!(["platform:bogus", "platform:bogus"])
    );
    let updated = service
        .update_organization_role(
            token,
            false,
            &json!({"roleId":id,"data":{"roleName":"RENAMED","permission":{}}}),
            now,
        )
        .await
        .unwrap();
    assert_eq!(updated["roleData"]["role"], "renamed");
    assert!(updated["roleData"].get("updatedAt").is_none());
    let mut tx = store.begin(false).await.unwrap();
    let row = tx.get(Model::OrganizationRole, id).await.unwrap().unwrap();
    assert_eq!(row["updatedAt"], timestamp(now));
    assert_eq!(row["permission"], "{}");
    assert_eq!(
        tx.get(Model::Member, "native-role-assignment")
            .await
            .unwrap()
            .unwrap()["role"],
        "viewer, dynamic-mutable "
    );
    tx.commit().await.unwrap();
    assert_eq!(
        service
            .update_organization_role(
                token,
                false,
                &json!({"roleId":id,"data":{"roleName":"renamed"}}),
                now
            )
            .await
            .unwrap_err()
            .code,
        "ROLE_NAME_IS_ALREADY_TAKEN"
    );
    let request = json!({"roleId":id});
    let (left, right) = tokio::join!(
        service.delete_organization_role(token, false, &request, now),
        service.delete_organization_role(token, false, &request, now)
    );
    match (left, right) {
        (Ok(_), Err(error)) | (Err(error), Ok(_)) => assert_eq!(error.code, "ROLE_NOT_FOUND"),
        _ => panic!("one role deletion must succeed"),
    }
    duplicates(store, service, token, org, now).await;
    let mut tx = store.begin(true).await.unwrap();
    tx.delete(Model::Member, "native-role-assignment")
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
async fn duplicates<S: AuthStore>(
    store: &Arc<S>,
    service: &OrganizationService<S>,
    token: &str,
    org: &str,
    now: DateTime<Utc>,
) {
    let mut tx = store.begin(true).await.unwrap();
    for id in ["native-role-duplicate-a", "native-role-duplicate-b"] {
        tx.insert(Model::OrganizationRole,json!({"id":id,"organizationId":org,"role":"duplicate","permission":"{}","createdAt":timestamp(now)})).await.unwrap();
    }
    tx.insert(
        Model::OrganizationRole,
        json!({"id":"native-role-system","organizationId":org,"role":"system","permission":"{}"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    service.update_organization_role(token,false,&json!({"roleName":"duplicate","data":{"permission":{"platform":["platform:read"]},"roleName":"duplicates-renamed"}}),now).await.unwrap();
    let mut tx = store.begin(false).await.unwrap();
    for id in ["native-role-duplicate-a", "native-role-duplicate-b"] {
        let row = tx.get(Model::OrganizationRole, id).await.unwrap().unwrap();
        assert_eq!(row["role"], "duplicates-renamed");
        assert_eq!(row["permission"], "{\"platform\":[\"platform:read\"]}");
    }
    tx.commit().await.unwrap();
    service
        .delete_organization_role(token, false, &json!({"roleName":"duplicates-renamed"}), now)
        .await
        .unwrap();
    assert_eq!(
        service
            .delete_organization_role(token, false, &json!({"roleName":"system"}), now)
            .await
            .unwrap_err()
            .code,
        "CANNOT_DELETE_A_PRE_DEFINED_ROLE"
    );
    service
        .delete_organization_role(token, false, &json!({"roleId":"native-role-system"}), now)
        .await
        .unwrap();
    let mut tx = store.begin(false).await.unwrap();
    for id in [
        "native-role-duplicate-a",
        "native-role-duplicate-b",
        "native-role-system",
    ] {
        assert!(tx.get(Model::OrganizationRole, id).await.unwrap().is_none());
    }
    tx.commit().await.unwrap();
}
