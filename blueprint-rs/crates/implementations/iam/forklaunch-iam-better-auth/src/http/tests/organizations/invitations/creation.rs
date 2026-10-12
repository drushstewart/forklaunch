use super::*;
pub(super) async fn exercise<S: AuthStore>(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    store: &Arc<S>,
    org: &str,
) {
    let mut tx = store.begin(true).await.unwrap();
    let mut grant = tx
        .get(Model::OrganizationRole, "http-invitation-cancel-grant")
        .await
        .unwrap()
        .unwrap();
    grant["permission"] = json!("{\"invitation\":[\"create\",\"cancel\"]}");
    tx.replace(Model::OrganizationRole, grant).await.unwrap();
    tx.commit().await.unwrap();
    let post = |body: Value| {
        client
            .post(format!("{base}/api/auth/organization/invite-member"))
            .header("cookie", cookie)
            .header("origin", "http://localhost:3000")
            .json(&body)
    };
    let body = json!({"email":"HTTP-INVITEE@fixture.test","role":["member","viewer"],"organizationId":org});
    let created = post(body.clone()).send().await.unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let invitation: Value = created.json().await.unwrap();
    assert_eq!(invitation["email"], "http-invitee@fixture.test");
    assert_eq!(invitation["role"], "member,viewer");
    assert!(invitation["teamId"].is_null());
    assert_eq!(
        post(body).send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let resent = post(json!({"email":"http-invitee@fixture.test","role":"viewer","resend":true,"teamId":"absent","organizationId":org})).send().await.unwrap();
    assert_eq!(resent.status(), StatusCode::OK);
    let resent: Value = resent.json().await.unwrap();
    assert_eq!(resent["id"], invitation["id"]);
    assert_eq!(resent["role"], "member,viewer");
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::Invitation, invitation["id"].as_str().unwrap())
            .await
            .unwrap()
            .unwrap()["email"],
        "http-invitee@fixture.test"
    );
    tx.commit().await.unwrap();
}
