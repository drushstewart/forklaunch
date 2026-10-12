use super::*;
pub(super) async fn exercise<S: AuthStore>(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    store: &Arc<S>,
    org: &str,
) {
    let post = |path: &str, body: Value| {
        client
            .post(format!("{base}/api/auth/organization/{path}"))
            .header("cookie", cookie)
            .header("origin", "http://localhost:3000")
            .json(&body)
    };
    assert_eq!(
        post("create-team", json!({"name":"Denied"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"http-team-mutation-grant","organizationId":org,"role":"owner","permission":"{\"team\":[\"create\",\"update\",\"delete\"]}"})).await.unwrap();
    tx.commit().await.unwrap();
    let created = post("create-team", json!({"name":"HTTP Team","memberCount":999}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let team: Value = created.json().await.unwrap();
    let id = team["id"].as_str().unwrap();
    assert!(team.get("memberCount").is_none());
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::Team, id).await.unwrap().unwrap()["memberCount"],
        0
    );
    tx.commit().await.unwrap();
    let updated = post(
        "update-team",
        json!({"teamId":id,"data":{"name":"HTTP Renamed","organizationId":org}}),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(updated.status(), StatusCode::OK);
    assert_eq!(
        updated.json::<Value>().await.unwrap()["name"],
        "HTTP Renamed"
    );
    let removed = post("remove-team", json!({"teamId":id}))
        .send()
        .await
        .unwrap();
    assert_eq!(removed.status(), StatusCode::OK);
    assert_eq!(
        removed.json::<Value>().await.unwrap()["message"],
        "Team removed successfully."
    );
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.get(Model::Team, id).await.unwrap().is_none());
    tx.commit().await.unwrap();
}
