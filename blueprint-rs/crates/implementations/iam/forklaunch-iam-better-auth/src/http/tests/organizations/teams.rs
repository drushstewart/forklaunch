use super::*;
pub(super) async fn exercise(client: &reqwest::Client, base: &str, cookie: &str) {
    let get = |path: &str| {
        client
            .get(format!("{base}/api/auth/organization/{path}"))
            .header("cookie", cookie)
    };
    let post = |body: Value| {
        client
            .post(format!("{base}/api/auth/organization/set-active-team"))
            .header("cookie", cookie)
            .header("origin", "http://localhost:3000")
            .json(&body)
    };
    let teams: Value = get("list-teams")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(teams.as_array().unwrap().len(), 1);
    assert!(teams[0].get("memberCount").is_none());
    let id = teams[0]["id"].as_str().unwrap();
    let mine: Value = get("list-user-teams")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(mine, teams);
    let members: Value = get("list-team-members")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(members[0]["userId"], "http-owner");
    assert!(members[0].get("membershipKey").is_none());
    let clear = post(json!({"teamId":null})).send().await.unwrap();
    assert_eq!(clear.status(), StatusCode::OK);
    assert!(clear.headers().get("set-cookie").is_some());
    let clear = post(json!({"teamId":null})).send().await.unwrap();
    assert_eq!(clear.status(), StatusCode::OK);
    assert!(clear.headers().get("set-cookie").is_none());
    let selected = post(json!({"teamId":id})).send().await.unwrap();
    assert_eq!(selected.status(), StatusCode::OK);
    assert!(selected.headers().get("set-cookie").is_some());
    assert_eq!(selected.json::<Value>().await.unwrap()["id"], id);
    let view: Value = client
        .get(format!("{base}/api/auth/get-session"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(view["session"]["activeTeamId"], id);
    assert_eq!(
        view["session"]["activeOrganizationId"],
        teams[0]["organizationId"]
    );
}
