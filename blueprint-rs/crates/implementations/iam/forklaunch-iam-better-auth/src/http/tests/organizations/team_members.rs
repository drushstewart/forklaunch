use super::*;
pub(super) async fn exercise<S: AuthStore>(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    store: &Arc<S>,
    org: &str,
) {
    let teams: Value = client
        .get(format!("{base}/api/auth/organization/list-teams"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let team = teams[0]["id"].as_str().unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Member,json!({"id":"http-team-target-member","organizationId":org,"userId":"http-removal-target","role":"member"})).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"http-team-members-grant","organizationId":org,"role":"owner","permission":"{\"member\":[\"update\",\"delete\"]}"})).await.unwrap();
    tx.commit().await.unwrap();
    let post = |path: &str| {
        client
            .post(format!("{base}/api/auth/organization/{path}"))
            .header("cookie", cookie)
            .header("origin", "http://localhost:3000")
            .json(&json!({"teamId":team,"userId":"http-removal-target"}))
    };
    let added = post("add-team-member").send().await.unwrap();
    assert_eq!(added.status(), StatusCode::OK);
    let added: Value = added.json().await.unwrap();
    assert!(added.get("membershipKey").is_none());
    let again: Value = post("add-team-member")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(again["id"], added["id"]);
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::Team, team).await.unwrap().unwrap()["memberCount"],
        2
    );
    tx.commit().await.unwrap();
    let removed = post("remove-team-member").send().await.unwrap();
    assert_eq!(removed.status(), StatusCode::OK);
    assert_eq!(
        removed.json::<Value>().await.unwrap()["message"],
        "Team member removed successfully."
    );
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::Team, team).await.unwrap().unwrap()["memberCount"],
        1
    );
    assert!(
        tx.get(Model::TeamMember, added["id"].as_str().unwrap())
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
}
