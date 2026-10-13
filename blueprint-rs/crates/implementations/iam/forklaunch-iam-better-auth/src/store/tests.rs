use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::json;

#[allow(clippy::too_many_lines)]
async fn exercise_transactions<S: AuthStore>(store: &S) {
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"user-1","email":"private@fixture.test","subscription":null}),
    )
    .await
    .unwrap();
    tx.insert(Model::Account, json!({"id":"account-1","userId":"user-1","password":"private-hash","accessToken":"private-provider-token"})).await.unwrap();
    tx.insert(
        Model::Session,
        json!({"id":"session-1","userId":"user-1","token":"private-session-token"}),
    )
    .await
    .unwrap();
    let signing_key = crate::jwt::SigningKey::generate("persisted-key-id").unwrap();
    tx.insert(Model::Jwks, json!({"id":"persisted-key-id","privateKey":STANDARD.encode(signing_key.private_pkcs8()),"publicKey":signing_key.public_jwk()})).await.unwrap();
    tx.commit().await.unwrap();
    {
        let mut tx = store.begin(false).await.unwrap();
        assert!(
            tx.list_page(Model::Invitation, None, 10)
                .await
                .unwrap()
                .records
                .is_empty()
        );
        assert_eq!(
            tx.find_unique(Model::User, "email", "private@fixture.test")
                .await
                .unwrap()
                .unwrap()["id"],
            "user-1"
        );
        assert!(
            tx.find_unique(Model::User, "email", "PRIVATE@fixture.test")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            tx.find_unique(Model::User, "email", "missing@fixture.test")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            tx.find_unique(Model::User, "subscription", "missing")
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            tx.find_unique(Model::Session, "token", "private-session-token")
                .await
                .unwrap()
                .unwrap()["id"],
            "session-1"
        );
        assert!(
            tx.find_unique(Model::User, "email OR 1=1", "ignored")
                .await
                .is_err()
        );
        tx.commit().await.unwrap();
    }
    {
        let mut tx = store.begin(false).await.unwrap();
        let row = tx
            .get(Model::Jwks, "persisted-key-id")
            .await
            .unwrap()
            .unwrap();
        let restored = crate::jwt::SigningKey::from_pkcs8(
            "persisted-key-id",
            &STANDARD
                .decode(row["privateKey"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(restored.public_jwk(), signing_key.public_jwk());
        tx.commit().await.unwrap();
    }
    {
        let mut tx = store.begin(true).await.unwrap();
        assert!(
            tx.insert(Model::User, json!({"id":"invalid-type","email":42}))
                .await
                .is_err()
        );
        assert!(
            tx.replace(Model::User, json!({"id":"user-1","email":42}))
                .await
                .is_err()
        );
        tx.commit().await.unwrap();
    }
    {
        let mut dropped = store.begin(true).await.unwrap();
        dropped
            .insert(
                Model::User,
                json!({"id":"dropped-user","email":"dropped@fixture.test"}),
            )
            .await
            .unwrap();
        dropped.delete(Model::Session, "session-1").await.unwrap();
    }
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.get(Model::User, "dropped-user").await.unwrap().is_none());
    assert!(tx.get(Model::User, "invalid-type").await.unwrap().is_none());
    assert!(tx.get(Model::Session, "session-1").await.unwrap().is_some());
    assert!(tx.delete(Model::User, "user-1").await.is_err());
    assert!(
        tx.replace(
            Model::User,
            json!({"id":"user-1","email":"changed@fixture.test"})
        )
        .await
        .is_err()
    );
    assert!(
        tx.insert(Model::User, json!({"id":"read-only-user"}))
            .await
            .is_err()
    );
    tx.commit().await.unwrap();
    {
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(
            Model::User,
            json!({"id":"rolled-back","email":"other@fixture.test"}),
        )
        .await
        .unwrap();
        assert!(
            tx.insert(
                Model::User,
                json!({"id":"conflicting","email":"private@fixture.test"})
            )
            .await
            .is_err()
        );
    }
    let mut tx = store.begin(false).await.unwrap();
    let rows = tx.list(Model::User).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["email"], "private@fixture.test");
    assert!(tx.get(Model::User, "rolled-back").await.unwrap().is_none());
    tx.commit().await.unwrap();
}

async fn exercise_relationship_queries<S: AuthStore>(store: &S) {
    let mut tx = store.begin(true).await.unwrap();
    for index in 0..13 {
        tx.insert(
            Model::Account,
            json!({"id":format!("relation-{index}"),"userId":"Quoted ' ☃","providerId":"external"}),
        )
        .await
        .unwrap();
    }
    let first = tx
        .find_page(Model::Account, "userId", "Quoted ' ☃", None, 5)
        .await
        .unwrap();
    assert_eq!(first.records.len(), 5);
    assert_eq!(first.records[4]["id"], "relation-4");
    tx.delete(Model::Account, "relation-4").await.unwrap();
    let second = tx
        .find_page(Model::Account, "userId", "Quoted ' ☃", first.next, 5)
        .await
        .unwrap();
    assert_eq!(second.records[0]["id"], "relation-5");
    let last = tx
        .find_page(Model::Account, "userId", "Quoted ' ☃", second.next, 5)
        .await
        .unwrap();
    assert_eq!(last.records.len(), 3);
    assert!(last.next.is_none());
    assert!(
        tx.find_page(Model::Member, "userId", "Quoted ' ☃", first.next, 5)
            .await
            .is_err()
    );
    assert!(
        tx.find_page(Model::Account, "userId", "Quoted ' ☃", None, 0)
            .await
            .is_err()
    );
    assert!(
        tx.find_page(Model::Account, "userId", "Quoted ' ☃", None, 4097)
            .await
            .is_err()
    );
    assert!(
        tx.find_page(Model::Account, "password", "ignored", None, 5)
            .await
            .is_err()
    );
    assert!(
        tx.find_page(Model::Account, "userId", "quoted ' ☃", None, 5)
            .await
            .unwrap()
            .records
            .is_empty()
    );
    tx.replace(
        Model::Account,
        json!({"id":"relation-0","userId":"replacement"}),
    )
    .await
    .unwrap();
    assert_eq!(
        tx.find_page(Model::Account, "userId", "Quoted ' ☃", None, 5)
            .await
            .unwrap()
            .records[0]["id"],
        "relation-1"
    );
    assert_eq!(
        tx.find_page(Model::Account, "userId", "replacement", None, 5)
            .await
            .unwrap()
            .records[0]["id"],
        "relation-0"
    );
    tx.replace(Model::Account, json!({"id":"relation-0","userId":null}))
        .await
        .unwrap();
    assert!(
        tx.find_page(Model::Account, "userId", "replacement", None, 5)
            .await
            .unwrap()
            .records
            .is_empty()
    );
    // All inserted/updated/deleted rows, lookup claims and audits roll back.
    drop(tx);
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.find_page(Model::Account, "userId", "Quoted ' ☃", None, 5)
            .await
            .unwrap()
            .records
            .is_empty()
    );
    tx.commit().await.unwrap();
}

async fn exercise_many<T: AuthTransaction>(tx: &mut T) {
    let ids = vec![
        "imported-4099".into(),
        "missing".into(),
        "imported-2".into(),
        "imported-4099".into(),
    ];
    let rows = tx.get_many(Model::User, &ids, 100).await.unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["imported-2", "imported-4099"]
    );
    assert_eq!(
        tx.get_many(Model::User, &ids, 1).await.unwrap()[0]["id"],
        "imported-2"
    );
    assert!(tx.get_many(Model::User, &ids, 0).await.unwrap().is_empty());
    assert!(tx.get_many(Model::User, &[], 100).await.unwrap().is_empty());
    assert!(tx.get_many(Model::User, &ids, 4097).await.is_err());
    assert!(
        tx.get_many(Model::User, &vec!["id".into(); 4097], 1)
            .await
            .is_err()
    );
}

async fn exercise_large_session_tokens<S: AuthStore + 'static>(store: std::sync::Arc<S>) {
    let now = chrono::DateTime::<chrono::Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"indexed-session-user","email":"indexed-session-user@fixture.test"}),
    )
    .await
    .unwrap();
    for index in 0..4100 {
        let mut session =
            crate::sessions::new_session("indexed-session-user", "", "", now).unwrap();
        session["id"] = json!(format!("indexed-session-{index}"));
        session["token"] = json!(format!("indexed-token-{index}"));
        tx.insert(Model::Session, session).await.unwrap();
    }
    tx.commit().await.unwrap();
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.list(Model::Session).await.is_err());
    assert_eq!(
        tx.find_unique(Model::Session, "token", "indexed-token-4099")
            .await
            .unwrap()
            .unwrap()["id"],
        "indexed-session-4099"
    );
    tx.commit().await.unwrap();
    {
        let mut tx = store.begin(true).await.unwrap();
        let mut duplicate = tx
            .get(Model::Session, "indexed-session-4099")
            .await
            .unwrap()
            .unwrap();
        duplicate["id"] = json!("duplicate-token");
        assert!(tx.insert(Model::Session, duplicate).await.is_err());
    }
    let service = crate::sessions::SessionService::new(store);
    assert_eq!(
        service
            .get("indexed-token-4099", false, false, now)
            .await
            .unwrap()
            .unwrap()
            .user["id"],
        "indexed-session-user"
    );
    assert!(
        service
            .get("indexed-token-missing", false, false, now)
            .await
            .unwrap()
            .is_none()
    );
    service.sign_out("indexed-token-4099").await.unwrap();
    assert!(
        service
            .get("indexed-token-4099", false, false, now)
            .await
            .unwrap()
            .is_none()
    );
}

// Reuses the >4096-row token fixture after its legacy-index backfill checks.
async fn exercise_large_session_collections<S: AuthStore + 'static>(store: std::sync::Arc<S>) {
    let now = chrono::DateTime::<chrono::Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let service = crate::sessions::SessionService::new(store.clone());
    let current = "indexed-token-0";
    let mut tx = store.begin(false).await.unwrap();
    let mut preserved = Vec::new();
    let mut after = None;
    loop {
        let page = tx.list_page(Model::Session, after, 256).await.unwrap();
        preserved.extend(
            page.records
                .into_iter()
                .filter(|row| row["userId"] != "indexed-session-user"),
        );
        after = page.next;
        if after.is_none() {
            break;
        }
    }
    tx.commit().await.unwrap();
    let rows = service.list(current, false, now).await.unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 4099);
    assert_eq!(rows[4098]["token"], "indexed-token-4098");
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"foreign-session-user","email":"foreign@fixture.test"}),
    )
    .await
    .unwrap();
    let foreign = crate::sessions::new_session("foreign-session-user", "", "", now).unwrap();
    tx.insert(Model::Session, foreign.clone()).await.unwrap();
    let mut expired = tx
        .get(Model::Session, "indexed-session-4098")
        .await
        .unwrap()
        .unwrap();
    expired["expiresAt"] = json!(now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    tx.replace(Model::Session, expired).await.unwrap();
    tx.commit().await.unwrap();
    let foreign_token = foreign["token"].as_str().unwrap();
    service
        .revoke_one(current, foreign_token, false, now)
        .await
        .unwrap();
    service
        .revoke_one(current, "missing", false, now)
        .await
        .unwrap();
    service
        .revoke_one(current, "indexed-token-4097", false, now)
        .await
        .unwrap();
    assert!(
        service
            .get("indexed-token-4097", false, false, now)
            .await
            .unwrap()
            .is_none()
    );
    service.revoke_others(current, false, now).await.unwrap();
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.list(Model::Session).await.unwrap().len(),
        preserved.len() + 3
    );
    assert!(
        tx.get(Model::Session, "indexed-session-4098")
            .await
            .unwrap()
            .is_some()
    );
    tx.commit().await.unwrap();
    assert_eq!(
        service
            .list(current, false, now)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    service.revoke_all(current, false, now).await.unwrap();
    let mut tx = store.begin(false).await.unwrap();
    preserved.push(foreign);
    assert_eq!(tx.list(Model::Session).await.unwrap(), preserved);
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn memory_session_authentication_exceeds_model_scan_bound() {
    let store = std::sync::Arc::new(memory::MemoryAuthStore::default());
    exercise_large_session_tokens(store.clone()).await;
    exercise_large_session_collections(store).await;
}

async fn exercise_pagination<S: AuthStore>(store: &S) {
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.list_page(Model::User, None, 0).await.is_err());
    assert!(tx.list_page(Model::User, None, 4097).await.is_err());
    let mut after = None;
    let mut ids = Vec::new();
    loop {
        let page = tx.list_page(Model::User, after, 137).await.unwrap();
        assert!(!page.records.is_empty() && page.records.len() <= 137);
        ids.extend(
            page.records
                .into_iter()
                .map(|row| row["id"].as_str().unwrap().to_owned()),
        );
        after = page.next;
        if after.is_none() {
            break;
        }
        assert!(tx.list_page(Model::Organization, after, 1).await.is_err());
    }
    assert_eq!(
        ids,
        (0..4100)
            .map(|index| format!("imported-{index}"))
            .collect::<Vec<_>>()
    );
    let first = tx.list_page(Model::User, None, 4096).await.unwrap();
    assert_eq!(first.records.len(), 4096);
    let last = tx.list_page(Model::User, first.next, 4096).await.unwrap();
    assert_eq!(last.records.len(), 4);
    assert!(last.next.is_none());
    tx.commit().await.unwrap();

    // A continuation remains valid after its row is deleted, and replacement
    // does not move a record to the end. Rollback restores both data and indexes.
    let mut tx = store.begin(true).await.unwrap();
    let first = tx.list_page(Model::User, None, 1).await.unwrap();
    tx.delete(Model::User, "imported-0").await.unwrap();
    let mut row = tx.get(Model::User, "imported-1").await.unwrap().unwrap();
    row["name"] = json!("updated");
    tx.replace(Model::User, row).await.unwrap();
    let second = tx.list_page(Model::User, first.next, 1).await.unwrap();
    assert_eq!(second.records[0]["id"], "imported-1");
    assert_eq!(second.records[0]["name"], "updated");
    drop(tx);
    let mut tx = store.begin(false).await.unwrap();
    let first = tx.list_page(Model::User, None, 2).await.unwrap();
    assert_eq!(first.records[0]["id"], "imported-0");
    assert!(first.records[1].get("name").is_none());
    tx.commit().await.unwrap();
}

async fn seed_large_directory<S: AuthStore>(store: &S) {
    let mut tx = store.begin(true).await.unwrap();
    for index in 0..4100 {
        tx.insert(Model::User, json!({"id":format!("imported-{index}"),"email":format!("imported-{index}@fixture.test")})).await.unwrap();
        tx.insert(Model::Account, json!({"id":format!("imported-account-{index}"),"userId":format!("imported-{index}"),"providerId":"external","accountId":format!("external-{index}")})).await.unwrap();
        tx.insert(Model::Member, json!({"id":format!("imported-member-{index}"),"userId":format!("imported-{index}"),"organizationId":"foreign-large-org","role":"member"})).await.unwrap();
        tx.insert(Model::OrganizationRole, json!({"id":format!("imported-role-{index}"),"organizationId":"foreign-large-org","role":"owner","permission":"{"})).await.unwrap();
        tx.insert(Model::Team, json!({"id":format!("imported-team-{index}"),"organizationId":"foreign-large-org","name":"Foreign team"})).await.unwrap();
        tx.insert(Model::TeamMember, json!({"id":format!("imported-team-member-{index}"),"teamId":format!("imported-team-{index}"),"userId":format!("imported-{index}"),"membershipKey":format!("foreign-key-{index}")})).await.unwrap();
        tx.insert(Model::Invitation, json!({"id":format!("imported-invitation-{index}"),"organizationId":"foreign-large-org","email":format!("imported-{index}@fixture.test"),"status":"pending"})).await.unwrap();
    }
    tx.insert(
        Model::Organization,
        json!({"id":"org-index","slug":"Quoted ' Unicode ☃"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

async fn exercise_large_unique_lookups<S: AuthStore + 'static>(store: std::sync::Arc<S>) {
    seed_large_directory(store.as_ref()).await;
    exercise_pagination(store.as_ref()).await;
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.list(Model::User).await.is_err(),
        "bounded list must still fail explicitly"
    );
    exercise_many(&mut tx).await;
    assert!(tx.list(Model::Account).await.is_err());
    assert_eq!(
        tx.find_page(Model::Account, "userId", "imported-4099", None, 1)
            .await
            .unwrap()
            .records[0]["id"],
        "imported-account-4099"
    );
    assert_eq!(
        tx.find_unique(Model::User, "email", "imported-4099@fixture.test")
            .await
            .unwrap()
            .unwrap()["id"],
        "imported-4099"
    );
    assert_eq!(
        tx.find_unique(Model::Organization, "slug", "Quoted ' Unicode ☃")
            .await
            .unwrap()
            .unwrap()["id"],
        "org-index"
    );
    tx.commit().await.unwrap();
    let now = chrono::DateTime::<chrono::Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let service = crate::accounts::CredentialService::new(store.clone());
    let signup = serde_json::from_value(json!({"name":"Large tenant","firstName":"Large","lastName":"Tenant","email":"Beyond@Fixture.test","password":"test-only-password"})).unwrap();
    let grant = service.sign_up(signup, "", "", now).await.unwrap();
    let signin = serde_json::from_value(
        json!({"email":"BEYOND@FIXTURE.TEST","password":"test-only-password"}),
    )
    .unwrap();
    let login = service.sign_in(signin, "", "", now).await.unwrap();
    assert_eq!(grant.user, login.user);
    exercise_large_organization_queries(
        store.clone(),
        login.session["token"].as_str().unwrap(),
        login.user["id"].as_str().unwrap(),
        now,
    )
    .await;
    let organizations = crate::organizations::OrganizationService::new(store.clone());
    assert_eq!(
        organizations
            .check_slug(
                login.session["token"].as_str().unwrap(),
                false,
                "Quoted ' Unicode ☃",
                now
            )
            .await
            .unwrap_err()
            .code,
        "ORGANIZATION_SLUG_ALREADY_TAKEN"
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.replace(
        Model::Organization,
        json!({"id":"org-index","slug":"replacement"}),
    )
    .await
    .unwrap();
    assert!(
        tx.find_unique(Model::Organization, "slug", "Quoted ' Unicode ☃")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tx.find_unique(Model::Organization, "slug", "replacement")
            .await
            .unwrap()
            .is_some()
    );
    tx.delete(Model::Organization, "org-index").await.unwrap();
    assert!(
        tx.find_unique(Model::Organization, "slug", "replacement")
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
    #[cfg(feature = "http")]
    crate::http::tests::exercise_http(store).await;
}

async fn exercise_large_organization_queries<S: AuthStore>(
    store: std::sync::Arc<S>,
    token: &str,
    user: &str,
    now: chrono::DateTime<chrono::Utc>,
) {
    let service = crate::organizations::OrganizationService::new(store.clone());
    let org = service
        .create(
            token,
            false,
            &json!({"name":"Scoped organization","slug":"scoped-large-org"}),
            now,
        )
        .await
        .unwrap();
    let id = org["id"].as_str().unwrap();
    exercise_large_team_queries(&service, token, id, now).await;
    let listed = service
        .list_members(token, false, &json!({"organizationId":id}), now)
        .await
        .unwrap();
    assert_eq!(listed["total"], 1);
    assert_eq!(listed["members"][0]["userId"], user);
    let graph = service
        .full(token, false, Some(id), None, None, now)
        .await
        .unwrap();
    assert_eq!(graph["members"].as_array().unwrap().len(), 1);
    assert_eq!(
        service
            .has_permission(
                token,
                false,
                &json!({"permissions":{"platform":["platform:read"]}}),
                now
            )
            .await
            .unwrap()["success"],
        true
    );
    let mut tx = store.begin(true).await.unwrap();
    let permission = json!({"ac":["read"],"organization":["delete"]}).to_string();
    tx.insert(
        Model::OrganizationRole,
        json!({"id":"large-own-grant","organizationId":id,"role":"owner","permission":permission}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        service
            .list_organization_roles(token, false, Some(id), now)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        service
            .get_organization_role(
                token,
                false,
                &json!({"organizationId":id,"roleName":"owner"}),
                now
            )
            .await
            .unwrap()["id"],
        "large-own-grant"
    );
    let surfacing = crate::surfacing::SurfacingService::new(store.clone());
    assert_eq!(surfacing.surface_role(user, now).await.unwrap(), "owner");
    assert_eq!(
        surfacing.surface_permissions(user, now).await.unwrap(),
        vec![json!(permission)]
    );
    service.delete(token, false, id, now).await.unwrap();
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.find_page(Model::Member, "organizationId", id, None, 1)
            .await
            .unwrap()
            .records
            .is_empty()
    );
    assert_eq!(
        tx.find_page(
            Model::Member,
            "organizationId",
            "foreign-large-org",
            None,
            4096
        )
        .await
        .unwrap()
        .records
        .len(),
        4096
    );
    tx.commit().await.unwrap();
}

async fn exercise_large_team_queries<S: AuthStore>(
    service: &crate::organizations::OrganizationService<S>,
    token: &str,
    org: &str,
    now: chrono::DateTime<chrono::Utc>,
) {
    let teams = service
        .list_teams(token, false, &json!({"organizationId":org}), now)
        .await
        .unwrap();
    assert_eq!(teams.as_array().unwrap().len(), 1);
    let team = teams[0]["id"].as_str().unwrap();
    assert_eq!(
        service
            .list_team_members(token, false, &json!({"teamId":team}), now)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        service
            .list_user_teams(token, false, &json!({}), now)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        service
            .list_invitations(token, false, Some(org), now)
            .await
            .unwrap(),
        json!([])
    );
    // An unauthenticated explicit-email query keeps the configured public API.
    assert_eq!(
        service
            .list_user_invitations("", false, Some("missing@fixture.test"), now)
            .await
            .unwrap(),
        json!([])
    );
}

#[tokio::test]
async fn memory_exact_lookups_and_credentials_exceed_the_model_scan_bound() {
    exercise_large_unique_lookups(std::sync::Arc::new(memory::MemoryAuthStore::default())).await;
}

#[tokio::test]
async fn memory_auth_transactions_are_atomic_unique_and_read_only() {
    let store = memory::MemoryAuthStore::default();
    exercise_transactions(&store).await;
    exercise_relationship_queries(&store).await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "requires FORKLAUNCH_TEST_POSTGRES_URL"]
#[allow(clippy::too_many_lines)]
async fn postgres_auth_encryption_rls_rotation_audits_and_atomicity() {
    use forklaunch_blueprint_core::AuditContext;
    use forklaunch_core::persistence::{TenantContext, encryption::FieldEncryptor};
    use postgres::PostgresAuthStore;
    use sqlx::postgres::PgPoolOptions;
    use std::sync::Arc;
    let url = std::env::var("FORKLAUNCH_TEST_POSTGRES_URL").unwrap();
    let owner = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    PostgresAuthStore::migrate(&owner).await.unwrap();
    PostgresAuthStore::migrate(&owner).await.unwrap();
    let role = format!("fl_auth_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!(
        "CREATE ROLE {role} NOLOGIN NOSUPERUSER NOBYPASSRLS"
    ))
    .execute(&owner)
    .await
    .unwrap();
    sqlx::query(&format!("GRANT USAGE ON SCHEMA fl_better_auth TO {role}"))
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query(&format!("GRANT SELECT,INSERT,UPDATE,DELETE ON fl_better_auth.records,fl_better_auth.unique_claims,fl_better_auth.lookup_claims,fl_better_auth.audit TO {role}")).execute(&owner).await.unwrap();
    sqlx::query(&format!(
        "GRANT USAGE ON ALL SEQUENCES IN SCHEMA fl_better_auth TO {role}"
    ))
    .execute(&owner)
    .await
    .unwrap();
    let set_role = format!("SET ROLE {role}");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |connection, _| {
            let set_role = set_role.clone();
            Box::pin(async move {
                sqlx::query(&set_role).execute(connection).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    let tenant = TenantContext::new(format!("{role}-a")).unwrap();
    let other = TenantContext::new(format!("{role}-b")).unwrap();
    let key = Arc::new(FieldEncryptor::new("auth-fixture-old-key").unwrap());
    assert!(
        PostgresAuthStore::new(
            owner.clone(),
            tenant.clone(),
            key.clone(),
            AuditContext::system("reject-owner")
        )
        .await
        .is_err()
    );
    let store = PostgresAuthStore::new(
        pool.clone(),
        tenant.clone(),
        key.clone(),
        AuditContext::actor("operator", "auth-fixture"),
    )
    .await
    .unwrap();
    assert!(store.with_audit_context(AuditContext::system("")).is_err());
    exercise_transactions(&store).await;
    exercise_relationship_queries(&store).await;
    let other_store = PostgresAuthStore::new(
        pool.clone(),
        other.clone(),
        key.clone(),
        AuditContext::system("other-fixture"),
    )
    .await
    .unwrap();
    let mut tx = other_store.begin(true).await.unwrap();
    assert!(tx.get(Model::User, "user-1").await.unwrap().is_none());
    assert!(
        tx.find_unique(Model::User, "email", "private@fixture.test")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tx.find_page(Model::Account, "userId", "user-1", None, 1)
            .await
            .unwrap()
            .records
            .is_empty()
    );
    tx.insert(
        Model::User,
        json!({"id":"user-1","email":"private@fixture.test"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    crate::retention::tests::exercise(Arc::new(other_store.clone())).await;
    crate::compliance::tests::exercise(Arc::new(other_store.clone())).await;
    crate::compliance::tests::exercise_failures(Arc::new(other_store.clone())).await;
    assert!(
        crate::compliance::ComplianceService::new(Arc::new(store.clone()))
            .export("compliance-other")
            .await
            .unwrap()
            .entities
            .is_empty()
    );
    crate::surfacing::tests::exercise(Arc::new(other_store.clone())).await;
    let isolated_surfacing = crate::surfacing::SurfacingService::new(Arc::new(store.clone()));
    assert_eq!(
        isolated_surfacing
            .surface_role("surface-owner", chrono::Utc::now())
            .await
            .unwrap(),
        Value::Null
    );
    assert!(
        isolated_surfacing
            .surface_permissions("surface-owner", chrono::Utc::now())
            .await
            .unwrap()
            .is_empty()
    );
    crate::keyring::tests::exercise_key_lifecycle(Arc::new(other_store.clone())).await;
    crate::oauth::state::tests::exercise(Arc::new(other_store.clone())).await;
    crate::oauth::google_accounts::tests::exercise(Arc::new(other_store.clone())).await;
    #[cfg(feature = "http")]
    crate::http::tests::tokens::exercise(Arc::new(other_store.clone())).await;
    let google_ciphertexts: Vec<String> =
        sqlx::query_scalar("SELECT ciphertext FROM fl_better_auth.records WHERE tenant_id=$1")
            .bind(other.id())
            .fetch_all(&owner)
            .await
            .unwrap();
    assert!(!google_ciphertexts.is_empty());
    for ciphertext in google_ciphertexts {
        for secret in [
            "native-google@fixture.test",
            "private-google-id-token",
            "private-google-refresh-token",
            "updated-private-access",
            "native-updated-access",
            "opaque-not-expanded",
            "native-compliance-foreign-private",
            "native-retention-exact-private",
            "native-retention-marked-private",
            "native-rotated-refresh",
        ] {
            assert!(!ciphertext.contains(secret));
        }
    }

    let encrypted: Vec<String> =
        sqlx::query_scalar("SELECT ciphertext FROM fl_better_auth.records WHERE tenant_id=$1")
            .bind(tenant.id())
            .fetch_all(&owner)
            .await
            .unwrap();
    assert_eq!(encrypted.len(), 4);
    for value in encrypted {
        for secret in [
            "private@fixture.test",
            "private-hash",
            "private-provider-token",
            "private-session-token",
            "privateKey",
        ] {
            assert!(!value.contains(secret));
        }
    }
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM fl_better_auth.audit WHERE tenant_id=$1 AND actor_id='operator'",
    )
    .bind(tenant.id())
    .fetch_one(&owner)
    .await
    .unwrap();
    assert_eq!(
        audit, 4,
        "rolled-back transactions must not leak audit records"
    );
    let unscoped: i64 = sqlx::query_scalar("SELECT count(*) FROM fl_better_auth.records")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        unscoped, 0,
        "tenant context must not leak between pooled transactions"
    );
    let unscoped_lookups: i64 =
        sqlx::query_scalar("SELECT count(*) FROM fl_better_auth.lookup_claims")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(unscoped_lookups, 0);
    let stored_indexes: Vec<String> = sqlx::query_scalar(
        "SELECT blind_index FROM fl_better_auth.lookup_claims WHERE tenant_id=$1",
    )
    .bind(tenant.id())
    .fetch_all(&owner)
    .await
    .unwrap();
    assert!(!stored_indexes.is_empty());
    assert!(stored_indexes.iter().all(|index| !index.contains("user-1")));
    let rotated = PostgresAuthStore::new(
        pool.clone(),
        tenant.clone(),
        Arc::new(
            FieldEncryptor::new("auth-fixture-new-key")
                .unwrap()
                .with_previous_keys(&["auth-fixture-old-key".into()])
                .unwrap(),
        ),
        AuditContext::system("rotation-fixture"),
    )
    .await
    .unwrap();
    let mut indexed = rotated.begin(false).await.unwrap();
    let page = indexed
        .find_page(Model::Account, "userId", "user-1", None, 1)
        .await
        .unwrap();
    assert_eq!(page.records.len(), 1);
    assert!(
        page.next.is_none(),
        "retained-key claims must not duplicate rows"
    );
    assert_eq!(page.records[0]["id"], "account-1");
    assert_eq!(
        indexed
            .find_unique(Model::User, "email", "private@fixture.test")
            .await
            .unwrap()
            .unwrap()["id"],
        "user-1"
    );
    indexed.commit().await.unwrap();
    crate::sessions::tests::exercise_sessions(Arc::new(rotated.clone())).await;
    crate::accounts::tests::exercise_credentials(Arc::new(rotated.clone())).await;
    crate::organizations::tests::exercise_organizations(Arc::new(rotated.clone())).await;
    let bulk_tenant = TenantContext::new(format!("{role}-bulk")).unwrap();
    let bulk = PostgresAuthStore::new(
        pool.clone(),
        bulk_tenant.clone(),
        key.clone(),
        AuditContext::system("indexed-bulk"),
    )
    .await
    .unwrap();
    exercise_large_unique_lookups(Arc::new(bulk.clone())).await;
    exercise_large_session_tokens(Arc::new(bulk.clone())).await;
    sqlx::query("ANALYZE fl_better_auth.unique_claims")
        .execute(&owner)
        .await
        .unwrap();
    let plan: Value = sqlx::query_scalar("EXPLAIN (FORMAT JSON) DELETE FROM fl_better_auth.unique_claims WHERE tenant_id=$1 AND model='session' AND id='indexed-session-4098'")
        .bind(bulk_tenant.id()).fetch_one(&owner).await.unwrap();
    assert!(
        plan.to_string().contains("unique_claims_record"),
        "record claim deletion should use the selective owner index: {plan}"
    );
    // Emulate pre-index encrypted sessions, then rebuild all 4,099 live tokens
    // in bounded pages with a newly opened runtime store.
    sqlx::query("DELETE FROM fl_better_auth.unique_claims WHERE tenant_id=$1 AND model='session'")
        .bind(bulk_tenant.id())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM fl_better_auth.lookup_claims WHERE tenant_id=$1")
        .bind(bulk_tenant.id())
        .execute(&owner)
        .await
        .unwrap();
    let reopened = PostgresAuthStore::new(
        pool.clone(),
        bulk_tenant.clone(),
        key.clone(),
        AuditContext::system("session-backfill"),
    )
    .await
    .unwrap();
    let mut indexed = reopened.begin(false).await.unwrap();
    assert_eq!(
        indexed
            .find_unique(Model::Session, "token", "indexed-token-4098")
            .await
            .unwrap()
            .unwrap()["id"],
        "indexed-session-4098"
    );
    indexed.commit().await.unwrap();

    let mut indexed = reopened.begin(false).await.unwrap();
    assert_eq!(
        indexed
            .find_page(Model::Account, "userId", "imported-4099", None, 1)
            .await
            .unwrap()
            .records[0]["id"],
        "imported-account-4099"
    );
    indexed.commit().await.unwrap();
    exercise_large_session_collections(Arc::new(reopened)).await;
    sqlx::query("UPDATE fl_better_auth.lookup_claims SET id='imported-account-0',sequence=(SELECT sequence FROM fl_better_auth.records WHERE tenant_id=$1 AND model='account' AND id='imported-account-0') WHERE tenant_id=$1 AND model='account' AND field='userId' AND id='imported-account-4099'")
        .bind(bulk_tenant.id()).execute(&owner).await.unwrap();
    let mut indexed = bulk.begin(false).await.unwrap();
    assert!(
        indexed
            .find_page(Model::Account, "userId", "imported-4099", None, 1)
            .await
            .is_err()
    );
    indexed.commit().await.unwrap();

    // A transplanted index must not authenticate a different valid encrypted row.
    let moved_indexes: Vec<String> = sqlx::query_scalar("UPDATE fl_better_auth.unique_claims SET id='imported-0' WHERE tenant_id=$1 AND model='user' AND field='email' AND id='imported-4099' RETURNING blind_index")
        .bind(bulk_tenant.id()).fetch_all(&owner).await.unwrap();
    assert!(!moved_indexes.is_empty());
    let mut indexed = bulk.begin(false).await.unwrap();
    assert!(
        indexed
            .find_unique(Model::User, "email", "imported-4099@fixture.test")
            .await
            .is_err()
    );
    indexed.commit().await.unwrap();
    sqlx::query("UPDATE fl_better_auth.unique_claims SET id='imported-4099' WHERE tenant_id=$1 AND model='user' AND field='email' AND blind_index=ANY($2)")
        .bind(bulk_tenant.id()).bind(moved_indexes).execute(&owner).await.unwrap();
    let cleanup_tenants = vec![
        tenant.id().to_owned(),
        other.id().to_owned(),
        bulk_tenant.id().to_owned(),
    ];
    #[cfg(feature = "http")]
    let cleanup_tenants = {
        // Raw storage fixtures intentionally contain incomplete session/JWKS
        // records. Give the live auth service its own complete tenant graph.
        let http_tenant = TenantContext::new(format!("{role}-http")).unwrap();
        let http_store = PostgresAuthStore::new(
            pool.clone(),
            http_tenant.clone(),
            key.clone(),
            AuditContext::system("http-fixture"),
        )
        .await
        .unwrap();
        crate::http::tests::exercise_http(Arc::new(http_store)).await;
        let mut tenants = cleanup_tenants;
        tenants.push(http_tenant.id().to_owned());
        tenants
    };
    let mut tx = rotated.begin(true).await.unwrap();
    assert_eq!(
        tx.get(Model::Account, "account-1").await.unwrap().unwrap()["password"],
        "private-hash"
    );
    assert!(
        tx.insert(
            Model::User,
            json!({"id":"duplicate-after-rotation","email":"private@fixture.test"})
        )
        .await
        .is_err()
    );
    drop(tx);
    // Valid ciphertext cannot be transplanted between records or model families.
    sqlx::query("UPDATE fl_better_auth.records SET ciphertext=(SELECT ciphertext FROM fl_better_auth.records WHERE tenant_id=$1 AND model='account' AND id='account-1') WHERE tenant_id=$1 AND model='session' AND id='session-1'").bind(tenant.id()).execute(&owner).await.unwrap();
    let mut tx = rotated.begin(false).await.unwrap();
    assert!(tx.get(Model::Session, "session-1").await.is_err());
    tx.commit().await.unwrap();
    for table in ["fl_better_auth.records", "fl_better_auth.audit"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE tenant_id=ANY($1)"))
            .bind(&cleanup_tenants)
            .execute(&owner)
            .await
            .unwrap();
    }
    pool.close().await;
    sqlx::query(&format!("DROP OWNED BY {role}"))
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query(&format!("DROP ROLE {role}"))
        .execute(&owner)
        .await
        .unwrap();
    owner.close().await;
}
