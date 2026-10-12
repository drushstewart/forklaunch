use super::*;
mod creation;
mod mutations;
pub(super) async fn exercise<S: AuthStore>(store: Arc<S>) {
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
            &json!({"name":"Invitation Reads","slug":"invitation-reads"}),
            now,
        )
        .await
        .unwrap();
    let org = organization["id"].as_str().unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Invitation,json!({"id":"native-invitation-read","organizationId":org,"email":"org-native@fixture.test","inviterId":"org-native-owner","role":"member","status":"pending","createdAt":now.to_rfc3339(),"expiresAt":now.to_rfc3339(),"teamId":null})).await.unwrap();
    let mut user = tx
        .get(Model::User, "org-native-owner")
        .await
        .unwrap()
        .unwrap();
    user["emailVerified"] = json!(true);
    tx.replace(Model::User, user).await.unwrap();
    tx.commit().await.unwrap();
    let found = service
        .get_invitation(token, false, "native-invitation-read", now)
        .await
        .unwrap();
    assert_eq!(found["organizationName"], "Invitation Reads");
    assert_eq!(found["inviterEmail"], "org-native@fixture.test");
    assert_eq!(
        service
            .list_invitations(token, false, Some(org), now)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        service
            .list_user_invitations("", false, Some("ORG-NATIVE@fixture.test"), now)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "native-invitation-read")
    );
    assert_eq!(
        service
            .get_invitation(
                token,
                false,
                "native-invitation-read",
                now + chrono::Duration::milliseconds(1)
            )
            .await
            .unwrap_err()
            .message,
        "Invitation not found!"
    );
    mutations::exercise(store.clone(), &service, token, org, now).await;
    creation::exercise(store.clone(), &service, token, org, now).await;
    sessions.sign_out(token).await.unwrap();
}
