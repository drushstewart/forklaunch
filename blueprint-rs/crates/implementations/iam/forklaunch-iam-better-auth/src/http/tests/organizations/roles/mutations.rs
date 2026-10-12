use super::*;
pub(super) async fn exercise<S: AuthStore>(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    store: &Arc<S>,
    org: &str,
) {
    let post = |route: &str, body: Value| {
        client
            .post(format!("{base}/api/auth/organization/{route}"))
            .header("cookie", cookie)
            .header("origin", "http://localhost:3000")
            .json(&body)
    };
    assert_eq!(
        post("update-role", json!({"roleName":"dynamic-http","data":{}}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post("delete-role", json!({"roleName":"dynamic-http"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"http-role-mutation-grant","organizationId":org,"role":"owner","permission":"{\"ac\":[\"update\",\"delete\"]}"})).await.unwrap();
    tx.commit().await.unwrap();
    let response = post(
        "update-role",
        json!({"roleName":"dynamic-http","data":{"roleName":"HTTP-RENAMED","permission":{}}}),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("set-cookie").is_none());
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["roleData"]["role"], "http-renamed");
    assert_eq!(body["roleData"]["permission"], json!({}));
    let id = body["roleData"]["id"].as_str().unwrap();
    let mut tx = store.begin(false).await.unwrap();
    let row = tx.get(Model::OrganizationRole, id).await.unwrap().unwrap();
    assert_eq!(row["permission"], "{}");
    assert!(row["updatedAt"].is_string());
    tx.commit().await.unwrap();
    let response = post(
        "update-role",
        json!({"roleId":id,"data":{"permission":{"platform":["bogus"]}}}),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let error: Value = response.json().await.unwrap();
    assert_eq!(error["missingPermissions"], json!(["platform:bogus"]));
    let response = post("delete-role", json!({"roleId":id}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("set-cookie").is_none());
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"success":true})
    );
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.get(Model::OrganizationRole, id).await.unwrap().is_none());
    tx.commit().await.unwrap();
}
