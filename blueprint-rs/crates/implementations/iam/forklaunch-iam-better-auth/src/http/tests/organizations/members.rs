use super::*;

pub(super) async fn exercise<S: AuthStore>(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    store: &Arc<S>,
    id: &str,
) {
    let post = |path: &str, body: Value| {
        client
            .post(format!("{base}/api/auth/organization/{path}"))
            .header("cookie", cookie)
            .header("origin", "http://localhost:3000")
            .json(&body)
    };
    assert_eq!(
        post("set-active", json!({"organizationId":id}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let member: Value = client
        .get(format!("{base}/api/auth/organization/get-active-member"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(member["role"], "owner");
    assert!(member["user"].get("privateSecret").is_none());
    let role: Value = client
        .get(format!(
            "{base}/api/auth/organization/get-active-member-role"
        ))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(role, json!({"role":"owner"}));
    assert_eq!(
        post(
            "update-member-role",
            json!({"memberId":member["id"],"role":"member"})
        )
        .send()
        .await
        .unwrap()
        .status(),
        StatusCode::BAD_REQUEST
    );
    let updated = post(
        "update-member-role",
        json!({"memberId":member["id"],"role":["owner", "viewer"]}),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(updated.status(), StatusCode::OK);
    assert_eq!(
        updated.json::<Value>().await.unwrap()["role"],
        "owner,viewer"
    );
    assert_eq!(
        post("leave", json!({"organizationId":id}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User, json!({"id":"http-removal-target","email":"http-removal@fixture.test","name":"Removal","privateSecret":"not-public"})).await.unwrap();
    tx.insert(Model::Member, json!({"id":"http-removal-member","organizationId":id,"userId":"http-removal-target","role":"member"})).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"http-member-grant","organizationId":id,"role":"owner","permission":"{\"member\":[\"delete\"]}"})).await.unwrap();
    tx.commit().await.unwrap();
    let removed = post(
        "remove-member",
        json!({"memberIdOrEmail":"HTTP-REMOVAL@FIXTURE.TEST"}),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(removed.status(), StatusCode::OK);
    let removed: Value = removed.json().await.unwrap();
    assert_eq!(
        removed["member"]["user"]["email"],
        "http-removal@fixture.test"
    );
    assert!(removed["member"]["user"].get("privateSecret").is_none());
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.get(Model::Member, "http-removal-member")
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
}
