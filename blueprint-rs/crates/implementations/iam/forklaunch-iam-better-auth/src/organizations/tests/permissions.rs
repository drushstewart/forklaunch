use super::*;
pub(super) async fn exercise_permissions<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let sessions = SessionService::new(store.clone());
    let session = sessions
        .create("org-native-owner", "", "", now)
        .await
        .unwrap();
    let token = session["token"].as_str().unwrap();
    let service = OrganizationService::new(store.clone());
    let organization = service
        .create(
            token,
            false,
            &json!({"name":"Permission Org","slug":"native-permission-org"}),
            now,
        )
        .await
        .unwrap();
    let id = organization["id"].as_str().unwrap();
    let change =
        json!({"data":{"name":"Changed","metadata":{"source":"permission-test"},"id":"injected"}});
    assert_eq!(
        service
            .update(token, false, &change, now)
            .await
            .unwrap_err()
            .code,
        "YOU_ARE_NOT_ALLOWED_TO_UPDATE_THIS_ORGANIZATION"
    );
    assert_eq!(
        service
            .has_permission(
                token,
                false,
                &json!({"permissions":{"platform":["platform:read"]}}),
                now
            )
            .await
            .unwrap()["success"],
        true
    );
    assert_eq!(
        service
            .has_permission(
                token,
                false,
                &json!({"permissions":{"organization":["update"]}}),
                now
            )
            .await
            .unwrap()["success"],
        false
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"native-org-grant","organizationId":id,"role":"owner","permission":"{\"organization\":[\"update\",\"delete\"]}"})).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"native-foreign-bad-grant","organizationId":"other-org","role":"owner","permission":"{"})).await.unwrap();
    tx.insert(Model::Invitation,json!({"id":"native-permission-invite","organizationId":id,"email":"native-invited@fixture.test","status":"pending"})).await.unwrap();
    tx.commit().await.unwrap();
    let updated = service.update(token, false, &change, now).await.unwrap();
    assert_eq!(updated["id"], id);
    assert_eq!(updated["name"], "Changed");
    assert_eq!(updated["metadata"], change["data"]["metadata"]);
    assert_eq!(
        service
            .metadata(token, false, None, None, now)
            .await
            .unwrap()["metadata"],
        "{\"source\":\"permission-test\"}"
    );
    assert_eq!(
        service
            .update(token, false, &json!({"data":{"slug":"native-org"}}), now)
            .await
            .unwrap_err()
            .code,
        "ORGANIZATION_SLUG_ALREADY_TAKEN"
    );
    assert_eq!(service.has_permission(token,false,&json!({"permissions":{"platform":["platform:read"],"organization":["update","delete"]}}),now).await.unwrap()["success"],true);
    exercise_delete(store, &service, &sessions, token, id, now).await;
}
async fn exercise_delete<S: AuthStore>(
    store: Arc<S>,
    service: &OrganizationService<S>,
    sessions: &SessionService<S>,
    token: &str,
    id: &str,
    now: DateTime<Utc>,
) {
    let secondary = sessions
        .create("org-native-owner", "", "", now)
        .await
        .unwrap();
    let secondary_token = secondary["token"].as_str().unwrap();
    service
        .set_active(secondary_token, false, &json!({"organizationId":id}), now)
        .await
        .unwrap();
    let (left, right) = tokio::join!(
        service.delete(token, false, id, now),
        service.delete(token, false, id, now)
    );
    assert_ne!(left.is_ok(), right.is_ok());
    assert_eq!(
        left.err().or_else(|| right.err()).unwrap().code,
        "USER_IS_NOT_A_MEMBER_OF_THE_ORGANIZATION"
    );
    assert!(
        sessions
            .get(token, false, false, now)
            .await
            .unwrap()
            .unwrap()
            .session["activeOrganizationId"]
            .is_null()
    );
    assert_eq!(
        sessions
            .get(secondary_token, false, false, now)
            .await
            .unwrap()
            .unwrap()
            .session["activeOrganizationId"],
        id
    );
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.get(Model::Organization, id).await.unwrap().is_none());
    assert!(
        tx.list(Model::Member)
            .await
            .unwrap()
            .iter()
            .all(|row| row["organizationId"] != id)
    );
    assert!(
        tx.get(Model::Invitation, "native-permission-invite")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tx.get(Model::OrganizationRole, "native-org-grant")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        tx.list(Model::Team)
            .await
            .unwrap()
            .iter()
            .any(|row| row["organizationId"] == id)
    );
    tx.commit().await.unwrap();
}
