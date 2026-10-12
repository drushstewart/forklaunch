use super::*;
pub(super) async fn exercise<S: AuthStore>(
    store: Arc<S>,
    service: &OrganizationService<S>,
    token: &str,
    org: &str,
    now: DateTime<Utc>,
) {
    let body = json!({"email":"INVITED@fixture.test","role":["member","viewer"]});
    assert_eq!(
        service
            .create_invitation(token, false, &body, now)
            .await
            .unwrap_err()
            .code,
        "YOU_ARE_NOT_ALLOWED_TO_INVITE_USERS_TO_THIS_ORGANIZATION"
    );
    let mut tx = store.begin(true).await.unwrap();
    let mut grant = tx
        .get(Model::OrganizationRole, "native-invitation-cancel-grant")
        .await
        .unwrap()
        .unwrap();
    grant["permission"] = json!("{\"invitation\":[\"create\",\"cancel\"]}");
    tx.replace(Model::OrganizationRole, grant).await.unwrap();
    let team = tx
        .list(Model::Team)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row["organizationId"] == org)
        .unwrap();
    tx.commit().await.unwrap();
    let body = json!({"email":"INVITED@fixture.test","role":["member","viewer"],"teamId":[team["id"],team["id"]]});
    let invitation = service
        .create_invitation(token, false, &body, now)
        .await
        .unwrap();
    assert_eq!(invitation["email"], "invited@fixture.test");
    assert_eq!(invitation["role"], "member,viewer");
    assert_eq!(
        invitation["teamId"],
        format!(
            "{},{}",
            team["id"].as_str().unwrap(),
            team["id"].as_str().unwrap()
        )
    );
    let duplicate = service
        .create_invitation(token, false, &body, now)
        .await
        .unwrap_err();
    assert_eq!(
        duplicate.code,
        "USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION"
    );
    let resent = service.create_invitation(token,false,&json!({"email":"invited@fixture.test","role":"viewer","resend":true,"teamId":"absent"}),now + chrono::Duration::seconds(1)).await.unwrap();
    assert_eq!(resent["id"], invitation["id"]);
    assert_eq!(resent["role"], invitation["role"]);
    assert_eq!(resent["teamId"], invitation["teamId"]);
    assert_eq!(
        resent["expiresAt"],
        timestamp(now + chrono::Duration::hours(48) + chrono::Duration::seconds(1))
    );
    let mut tx = store.begin(true).await.unwrap();
    for i in 0..98 {
        tx.insert(Model::Invitation,json!({"id":format!("native-invite-limit-{i}"),"email":format!("limit-{i}@fixture.test"),"role":"member","organizationId":org,"inviterId":"org-native-owner","status":"pending","createdAt":timestamp(now),"expiresAt":timestamp(now + chrono::Duration::hours(48)),"teamId":null})).await.unwrap();
    }
    tx.commit().await.unwrap();
    let a = json!({"email":"last-slot-a@fixture.test","role":"member"});
    let b = json!({"email":"last-slot-b@fixture.test","role":"member"});
    let (a, b) = tokio::join!(
        service.create_invitation(token, false, &a, now),
        service.create_invitation(token, false, &b, now)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(
        a.err().or_else(|| b.err()).unwrap().code,
        "INVITATION_LIMIT_REACHED"
    );
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::Team, team["id"].as_str().unwrap())
            .await
            .unwrap()
            .unwrap()["memberCount"],
        team["memberCount"]
    );
    assert_eq!(
        tx.list(Model::Member)
            .await
            .unwrap()
            .iter()
            .filter(|row| row["organizationId"] == org)
            .count(),
        1
    );
    tx.commit().await.unwrap();
}
