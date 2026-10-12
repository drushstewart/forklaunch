use super::*;
pub(super) async fn exercise<S: AuthStore>(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    store: &Arc<S>,
    org: &str,
) {
    let post = |path: &str| {
        client
            .post(format!("{base}/api/auth/organization/{path}"))
            .header("cookie", cookie)
            .header("origin", "http://localhost:3000")
            .json(&json!({"invitationId":"http-invitation-read"}))
    };
    let rejected = post("reject-invitation").send().await.unwrap();
    assert_eq!(rejected.status(), StatusCode::OK);
    let rejected: Value = rejected.json().await.unwrap();
    assert_eq!(rejected["invitation"]["status"], "rejected");
    assert!(rejected["member"].is_null());
    assert_eq!(
        post("reject-invitation").send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        post("cancel-invitation").send().await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"http-invitation-cancel-grant","organizationId":org,"role":"owner","permission":"{\"invitation\":[\"cancel\"]}"})).await.unwrap();
    tx.commit().await.unwrap();
    for _ in 0..2 {
        let canceled = post("cancel-invitation").send().await.unwrap();
        assert_eq!(canceled.status(), StatusCode::OK);
        assert_eq!(
            canceled.json::<Value>().await.unwrap()["status"],
            "canceled"
        );
    }
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::Invitation, "http-invitation-read")
            .await
            .unwrap()
            .unwrap()["status"],
        "canceled"
    );
    tx.commit().await.unwrap();
}
