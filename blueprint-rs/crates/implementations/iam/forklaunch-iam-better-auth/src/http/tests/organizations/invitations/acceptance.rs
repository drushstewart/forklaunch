use super::*;
use crate::sessions::SessionService;
pub(super) async fn exercise<S: AuthStore>(
    client: &reqwest::Client,
    base: &str,
    store: &Arc<S>,
    org: &str,
) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User,json!({"id":"http-invitee","email":"http-invitee@fixture.test","name":"Invitee","emailVerified":true})).await.unwrap();
    let invitation = crate::store::find_all(
        &mut tx,
        Model::Invitation,
        "email",
        "http-invitee@fixture.test",
    )
    .await
    .unwrap()
    .into_iter()
    .find(|row| row["email"] == "http-invitee@fixture.test")
    .unwrap();
    let team = crate::store::find_all(&mut tx, Model::Team, "organizationId", org)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row["organizationId"] == org)
        .unwrap();
    tx.insert(Model::Invitation,json!({"id":"http-accept-team","organizationId":org,"email":"http-invitee@fixture.test","inviterId":"http-owner","role":"viewer","status":"pending","createdAt":now.to_rfc3339(),"expiresAt":"2027-01-16T08:00:00.000Z","teamId":team["id"]})).await.unwrap();
    tx.commit().await.unwrap();
    let session = SessionService::new(store.clone())
        .create("http-invitee", "", "", now)
        .await
        .unwrap();
    let secret = b"auth-http-fixture-only-secret-32-characters";
    let cookie = SessionCookie { secure: false };
    let signed = cookie
        .serialize(session["token"].as_str().unwrap(), secret, false)
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let transient = cookie
        .transient_flag(secret)
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let header = format!("{signed}; {transient}");
    let post = |id: &str| {
        client
            .post(format!("{base}/api/auth/organization/accept-invitation"))
            .header("cookie", &header)
            .header("origin", "http://localhost:3000")
            .json(&json!({"invitationId":id}))
    };
    let none = post(invitation["id"].as_str().unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(none.status(), StatusCode::OK);
    assert!(none.headers().get("set-cookie").is_none());
    assert_eq!(
        none.json::<Value>().await.unwrap()["member"]["role"],
        "member,viewer"
    );
    let single = post("http-accept-team").send().await.unwrap();
    assert_eq!(single.status(), StatusCode::OK);
    let cookies = single
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(cookies.len(), 2);
    assert!(
        cookies
            .iter()
            .all(|cookie| !cookie.to_lowercase().contains("max-age"))
    );
    assert_eq!(
        single.json::<Value>().await.unwrap()["invitation"]["status"],
        "accepted"
    );
    assert_eq!(
        post("http-accept-team").send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    persisted(store, org, &session, &team).await;
}

async fn persisted<S: AuthStore>(store: &Arc<S>, org: &str, session: &Value, team: &Value) {
    let mut tx = store.begin(false).await.unwrap();
    let current = tx
        .get(Model::Session, session["id"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current["activeOrganizationId"], org);
    assert_eq!(current["activeTeamId"], team["id"]);
    assert_eq!(
        crate::store::find_all(&mut tx, Model::Member, "organizationId", org)
            .await
            .unwrap()
            .iter()
            .filter(|row| row["organizationId"] == org && row["userId"] == "http-invitee")
            .count(),
        2
    );
    assert_eq!(
        tx.get(Model::Team, team["id"].as_str().unwrap())
            .await
            .unwrap()
            .unwrap()["memberCount"],
        team["memberCount"].as_i64().unwrap() + 1
    );
    tx.commit().await.unwrap();
}
