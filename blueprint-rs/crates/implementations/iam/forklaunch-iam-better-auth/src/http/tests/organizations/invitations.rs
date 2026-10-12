use super::*;
mod mutations;
pub(super) async fn exercise<S: AuthStore>(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    store: &Arc<S>,
    org: &str,
) {
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Invitation,json!({"id":"http-invitation-read","organizationId":org,"email":"http-owner@fixture.test","inviterId":"http-owner","role":"viewer","status":"pending","createdAt":"2027-01-15T08:00:00.000Z","expiresAt":"2027-01-16T08:00:00.000Z","teamId":null})).await.unwrap();
    tx.commit().await.unwrap();
    let get = |path: &str| {
        client
            .get(format!("{base}/api/auth/organization/{path}"))
            .header("cookie", cookie)
    };
    let unverified = get("get-invitation?id=http-invitation-read")
        .send()
        .await
        .unwrap();
    assert_eq!(unverified.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        unverified.json::<Value>().await.unwrap()["code"],
        "EMAIL_VERIFICATION_REQUIRED_FOR_INVITATION"
    );
    let mut tx = store.begin(true).await.unwrap();
    let mut user = tx.get(Model::User, "http-owner").await.unwrap().unwrap();
    user["emailVerified"] = json!(true);
    tx.replace(Model::User, user).await.unwrap();
    tx.commit().await.unwrap();
    let invitation = get("get-invitation?id=http-invitation-read")
        .send()
        .await
        .unwrap();
    assert_eq!(invitation.status(), StatusCode::OK);
    let invitation: Value = invitation.json().await.unwrap();
    assert_eq!(invitation["organizationId"], org);
    assert_eq!(invitation["inviterEmail"], "http-owner@fixture.test");
    assert_eq!(invitation["organizationName"], "HTTP Organization");
    for path in [
        format!("list-invitations?organizationId={org}"),
        "list-user-invitations".into(),
    ] {
        let response = get(&path).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let rows: Value = response.json().await.unwrap();
        assert!(
            rows.as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == "http-invitation-read")
        );
    }
    assert_eq!(
        get("list-user-invitations?email=other@fixture.test")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::Invitation, "http-invitation-read")
            .await
            .unwrap()
            .unwrap()["status"],
        "pending"
    );
    tx.commit().await.unwrap();
    mutations::exercise(client, base, cookie, store, org).await;
}
