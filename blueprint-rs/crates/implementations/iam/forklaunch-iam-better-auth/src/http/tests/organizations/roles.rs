use super::*;
pub(super) async fn exercise<S: AuthStore>(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    store: &Arc<S>,
    org: &str,
) {
    let get = |path: &str| {
        client
            .get(format!("{base}/api/auth/organization/{path}"))
            .header("cookie", cookie)
    };
    assert_eq!(
        get("list-roles").send().await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"http-role-read-grant","organizationId":org,"role":"owner","permission":"{\"ac\":[\"read\"]}"})).await.unwrap();
    tx.commit().await.unwrap();
    let list = get("list-roles").send().await.unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let list: Value = list.json().await.unwrap();
    assert!(
        list.as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "http-role-read-grant"
                && row["permission"] == json!({"ac":["read"]}))
    );
    let role = get("get-role?roleId=http-role-read-grant")
        .send()
        .await
        .unwrap();
    assert_eq!(role.status(), StatusCode::OK);
    let role: Value = role.json().await.unwrap();
    assert_eq!(role["organizationId"], org);
    assert_eq!(role["permission"], json!({"ac":["read"]}));
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::OrganizationRole, "http-role-read-grant")
            .await
            .unwrap()
            .unwrap()["permission"],
        "{\"ac\":[\"read\"]}"
    );
    tx.commit().await.unwrap();
    let post = || {
        client
            .post(format!("{base}/api/auth/organization/create-role"))
            .header("cookie", cookie)
            .header("origin", "http://localhost:3000")
    };
    assert_eq!(
        post()
            .json(&json!({"role":"dynamic-http", "permission":{}}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"http-role-create-grant","organizationId":org,"role":"owner","permission":"{\"ac\":[\"create\"]}"})).await.unwrap();
    tx.commit().await.unwrap();
    let response = post().json(&json!({"role":"DYNAMIC-HTTP", "permission":{"platform":["platform:read"]}, "additionalFields":{"role":"owner"}})).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get_all("set-cookie")
            .iter()
            .next()
            .is_none()
    );
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["roleData"]["role"], "dynamic-http");
    assert_eq!(body["statements"], json!({"platform":["platform:read"]}));
    let response = post()
        .json(&json!({"role":"denied", "permission":{"platform":["bogus","bogus"]}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body: Value = response.json().await.unwrap();
    assert_eq!(
        body["missingPermissions"],
        json!(["platform:bogus", "platform:bogus"])
    );
}
