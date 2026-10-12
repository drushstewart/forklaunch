use super::*;
mod invitations;
mod members;
mod team_members;
mod team_mutations;
mod teams;
pub(super) async fn exercise<S: AuthStore>(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    store: &Arc<S>,
) {
    let post = |path: &str, body: Value| {
        client
            .post(format!("{base}/api/auth/organization/{path}"))
            .header("cookie", cookie)
            .header("origin", "http://localhost:3000")
            .json(&body)
    };
    let created = post(
        "create",
        json!({"name":"HTTP Organization","slug":"http-org","metadata":{"source":"live-http"}}),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let organization: Value = created.json().await.unwrap();
    assert_eq!(organization["members"][0]["userId"], "http-owner");
    assert_eq!(organization["members"][0]["role"], "owner");
    let id = organization["id"].as_str().unwrap();
    let graph: Value = client
        .get(format!(
            "{base}/api/auth/organization/get-full-organization"
        ))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(graph["id"], id);
    assert_eq!(graph["teams"][0]["memberCount"], 1);
    assert!(graph["members"][0]["user"].get("privateSecret").is_none());
    let list: Value = client
        .get(format!("{base}/api/auth/organization/list"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list[0]["id"], id);
    exercise_http_organization_jwt(client, base, cookie, id, &graph).await;
    assert_eq!(
        post("check-slug", json!({"slug":"http-org"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let cleared = post("set-active", json!({"organizationId":null}))
        .send()
        .await
        .unwrap();
    assert_eq!(cleared.status(), StatusCode::OK);
    assert!(cleared.headers().get("set-cookie").is_some());
    let selected = post("set-active", json!({"organizationSlug":"http-org"}))
        .send()
        .await
        .unwrap();
    assert_eq!(selected.status(), StatusCode::OK);
    assert!(selected.headers().get("set-cookie").is_some());
    assert_eq!(
        post("set-active", json!({"organizationId":"unknown"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let view: Value = client
        .get(format!("{base}/api/auth/get-session"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(view["session"]["activeOrganizationId"].is_null());
    assert_eq!(view["session"]["activeTeamId"], graph["teams"][0]["id"]);
    members::exercise(client, base, cookie, store, id).await;
    teams::exercise(client, base, cookie).await;
    team_mutations::exercise(client, base, cookie, store, id).await;
    team_members::exercise(client, base, cookie, store, id).await;
    invitations::exercise(client, base, cookie, store, id).await;
    mutations(client, base, cookie, store, id).await;
}

async fn exercise_http_organization_jwt(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    id: &str,
    graph: &Value,
) {
    let selected = client
        .get(format!("{base}/api/auth/get-session"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap();
    let jwt = selected.headers()["set-auth-jwt"]
        .to_str()
        .unwrap()
        .to_owned();
    let view: Value = selected.json().await.unwrap();
    assert_eq!(view["session"]["activeOrganizationId"], id);
    assert_eq!(view["session"]["activeTeamId"], graph["teams"][0]["id"]);
    let jwks: Value = client
        .get(format!("{base}/api/auth/jwks"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(verify_jwt(&jwt, &jwks)["activeOrganizationId"], id);
}

async fn mutations<S: AuthStore>(
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
    let body = json!({"organizationId":id,"data":{"name":"HTTP Changed","metadata":{"source":"http-grant"}}});
    assert_eq!(
        post("update", body.clone()).send().await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"http-org-grant","organizationId":id,"role":"owner","permission":"{\"organization\":[\"update\",\"delete\"]}"})).await.unwrap();
    tx.commit().await.unwrap();
    let permission = post(
        "has-permission",
        json!({"organizationId":id,"permissions":{"organization":["update","delete"]}}),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(permission.status(), StatusCode::OK);
    assert_eq!(permission.json::<Value>().await.unwrap()["success"], true);
    let updated = post("update", body).send().await.unwrap();
    assert_eq!(updated.status(), StatusCode::OK);
    let updated: Value = updated.json().await.unwrap();
    assert_eq!(updated["name"], "HTTP Changed");
    assert_eq!(updated["metadata"]["source"], "http-grant");
    let metadata: Value = client
        .get(format!("{base}/api/auth/organization/get-organization"))
        .query(&[("organizationId", id)])
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(metadata["name"], "HTTP Changed");
    assert_eq!(metadata["metadata"], "{\"source\":\"http-grant\"}");
    let removed = post("delete", json!({"organizationId":id}))
        .send()
        .await
        .unwrap();
    assert_eq!(removed.status(), StatusCode::OK);
    assert_eq!(removed.json::<Value>().await.unwrap()["id"], id);
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
        tx.list(Model::Team)
            .await
            .unwrap()
            .iter()
            .any(|row| row["organizationId"] == id)
    );
    tx.commit().await.unwrap();
}
