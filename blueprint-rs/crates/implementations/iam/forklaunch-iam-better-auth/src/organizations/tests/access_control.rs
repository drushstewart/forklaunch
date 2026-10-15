use super::invitation_policy::{Fixture, seed};
use super::*;
use crate::organizations::{AddMemberInput, MemberRoles, OrganizationAccessControl};
fn policy(catalog: Value, roles: Value) -> OrganizationAccessControl {
    OrganizationAccessControl::new(
        serde_json::from_value(catalog).unwrap(),
        serde_json::from_value(roles).unwrap(),
    )
    .unwrap()
}
fn full() -> Value {
    json!({"organization":["update","delete"],"member":["update","delete"],"invitation":["create","cancel"],"team":["create","update","delete"],"ac":["create","read","update","delete"],"project":["read","write","outside-catalog"]})
}
fn configured() -> OrganizationAccessControl {
    let mut catalog = full();
    catalog["project"] = json!(["read", "write"]);
    policy(
        catalog,
        json!({"boss":full(),"reader":{"project":["read"]}}),
    )
}
async fn fixture<S: AuthStore>(store: &S, label: &str, role: &str) -> Fixture {
    let f = seed(store, &format!("access-{label}")).await;
    let mut tx = store.begin(true).await.unwrap();
    tx.delete(Model::OrganizationRole, &format!("{}-grant", f.org))
        .await
        .unwrap();
    let mut member = tx
        .get(Model::Member, &format!("{}-member", f.org))
        .await
        .unwrap()
        .unwrap();
    member["role"] = json!(role);
    tx.replace(Model::Member, member).await.unwrap();
    tx.commit().await.unwrap();
    f
}
async fn grant<S: AuthStore>(store: &S, f: &Fixture, id: &str, role: &str, permission: &str) {
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":format!("{}-{id}",f.org),"organizationId":f.org,"role":role,"permission":permission})).await.unwrap();
    tx.commit().await.unwrap();
}
#[allow(clippy::too_many_lines)]
pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    for (i, roles, actor, request, expected, dynamic) in [
        (
            0,
            json!({"custom":{"project":["read"]}}),
            "owner",
            json!({"platform":["platform:read"]}),
            false,
            None,
        ),
        (
            1,
            json!({"custom":{"project":["read"]}}),
            "custom",
            json!({"project":["read"]}),
            true,
            None,
        ),
        (
            2,
            json!({}),
            "owner",
            json!({"platform":["platform:read"]}),
            false,
            None,
        ),
        (
            3,
            json!({"custom":{"project":["read"]},"other":{"project":["write"]}}),
            "custom,other",
            json!({"project":["write"]}),
            true,
            None,
        ),
        (
            4,
            json!({"custom":{"project":["read"]},"other":{"project":["write"]}}),
            "custom,other",
            json!({"project":["read","write"]}),
            false,
            None,
        ),
        (
            5,
            json!({"custom":{"project":["read"]}}),
            "custom",
            json!({"project":["read","write"]}),
            true,
            Some("{\"project\":[\"write\"]}"),
        ),
    ] {
        let f = fixture(store.as_ref(), &format!("permission-{i}"), actor).await;
        if let Some(encoded) = dynamic {
            grant(store.as_ref(), &f, "dynamic", "custom", encoded).await;
        }
        let svc = OrganizationService::new(store.clone())
            .with_access_control(policy(json!({"project":["read","write"]}), roles));
        let result = svc
            .has_permission(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"permissions":request}),
                f.now,
            )
            .await
            .unwrap();
        assert_eq!(result["success"], expected);
    }
    for (i, name, permissions, code) in [
        (0, "boss", json!({}), Some("ROLE_NAME_IS_ALREADY_TAKEN")),
        (
            1,
            "fresh",
            json!({"unknown":["read"]}),
            Some("INVALID_RESOURCE"),
        ),
        (
            2,
            "fresh",
            json!({"project":["missing"]}),
            Some("YOU_ARE_NOT_ALLOWED_TO_CREATE_A_ROLE"),
        ),
        (3, "fresh", json!({"project":["read"]}), None),
        (4, "fresh", json!({"project":["outside-catalog"]}), None),
        (5, "owner", json!({}), None),
    ] {
        let f = fixture(store.as_ref(), &format!("role-{i}"), "boss").await;
        let svc = OrganizationService::new(store.clone()).with_access_control(configured());
        let out = svc
            .create_organization_role(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"role":name,"permission":permissions}),
                f.now,
            )
            .await;
        if let Some(code) = code {
            assert_eq!(out.unwrap_err().code, code);
        } else {
            assert!(out.is_ok());
        }
    }
    for role in ["reader", "owner", "admin", "member", "viewer"] {
        let f = fixture(store.as_ref(), &format!("invite-{role}"), "boss").await;
        let svc = OrganizationService::new(store.clone())
            .with_access_control(configured())
            .with_creator_role(crate::organizations::CreatorRole::new("boss").unwrap());
        let out = svc
            .create_invitation(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"email":"new-access@example.test","role":role}),
                f.now,
            )
            .await;
        if role == "viewer" {
            assert_eq!(out.unwrap_err().message, "ROLE_NOT_FOUND: viewer");
        } else {
            assert!(out.is_ok());
        }
    }
    let f = fixture(store.as_ref(), "empty-keys", "").await;
    let svc = OrganizationService::new(store.clone())
        .with_access_control(policy(json!({"":["read"]}), json!({"":{"": ["read"]}})));
    assert_eq!(
        svc.has_permission(
            &f.owner_token,
            false,
            &json!({"organizationId":f.org,"permissions":{"": ["read"]}}),
            f.now
        )
        .await
        .unwrap()["success"],
        true
    );
    prototypes(store.clone()).await;
    dynamic_prototypes(store.clone()).await;
    lifecycle(store.clone()).await;
    // Dynamic grants remain scoped and limited to the actual first-100 window.
    let f = fixture(store.as_ref(), "window", "boss").await;
    let svc = OrganizationService::new(store.clone())
        .with_access_control(policy(json!({"project":["read"]}), json!({})));
    for i in 0..100 {
        grant(store.as_ref(), &f, &format!("fill-{i}"), "filler", "{}").await;
    }
    grant(
        store.as_ref(),
        &f,
        "late",
        "boss",
        "{\"project\":[\"read\"]}",
    )
    .await;
    assert_eq!(
        svc.has_permission(
            &f.owner_token,
            false,
            &json!({"organizationId":f.org,"permissions":{"project":["read"]}}),
            f.now
        )
        .await
        .unwrap()["success"],
        false
    );
    // Malformed dynamic data is still validated before creator-role bypass.
    let f = fixture(store.as_ref(), "malformed", "boss").await;
    grant(store.as_ref(), &f, "bad", "boss", "not-json").await;
    let svc = OrganizationService::new(store.clone())
        .with_access_control(configured())
        .with_creator_role(crate::organizations::CreatorRole::new("boss").unwrap());
    assert_eq!(
        svc.update_member_role(
            &f.owner_token,
            false,
            &json!({"organizationId":f.org,"memberId":format!("{}-member",f.org),"role":"boss"}),
            f.now
        )
        .await
        .unwrap_err()
        .code,
        "HTTP_INTERNAL_EMPTY"
    );
    #[cfg(feature = "http")]
    http(store).await;
}
#[allow(clippy::too_many_lines)]
async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let f = fixture(store.as_ref(), "lifecycle", "boss").await;
    let svc = OrganizationService::new(store.clone()).with_access_control(configured());
    let base = json!({"organizationId":f.org,"role":"fresh","permission":{"project":["read"]}});
    let created = svc
        .create_organization_role(&f.owner_token, false, &base, f.now)
        .await
        .unwrap();
    let role_id = created["roleData"]["id"].as_str().unwrap();
    assert_eq!(
        svc.get_organization_role(
            &f.owner_token,
            false,
            &json!({"organizationId":f.org,"roleId":role_id}),
            f.now
        )
        .await
        .unwrap()["role"],
        "fresh"
    );
    assert_eq!(
        svc.list_organization_roles(&f.owner_token, false, Some(&f.org), f.now)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let out = svc
        .update_organization_role(
            &f.owner_token,
            false,
            &json!({"organizationId":f.org,"roleId":role_id,"data":{"roleName":"boss"}}),
            f.now,
        )
        .await
        .unwrap_err();
    assert_eq!(out.code, "ROLE_NAME_IS_ALREADY_TAKEN");
    svc.update_organization_role(&f.owner_token,false,&json!({"organizationId":f.org,"roleId":role_id,"data":{"permission":{"project":["write"]}}}),f.now).await.unwrap();
    assert_eq!(
        svc.delete_organization_role(
            &f.owner_token,
            false,
            &json!({"organizationId":f.org,"roleName":"boss"}),
            f.now
        )
        .await
        .unwrap_err()
        .code,
        "CANNOT_DELETE_A_PRE_DEFINED_ROLE"
    );
    svc.delete_organization_role(
        &f.owner_token,
        false,
        &json!({"organizationId":f.org,"roleId":role_id}),
        f.now,
    )
    .await
    .unwrap();
    svc.update(
        &f.owner_token,
        false,
        &json!({"organizationId":f.org,"data":{"name":"Changed"}}),
        f.now,
    )
    .await
    .unwrap();
    let member = svc
        .add_member(
            None,
            false,
            AddMemberInput {
                user_id: f.recipient.clone(),
                role: MemberRoles::Role("member".into()),
                organization_id: Some(f.org.clone()),
                team_id: None,
            },
            f.now,
        )
        .await
        .unwrap();
    svc.update_member_role(
        &f.owner_token,
        false,
        &json!({"organizationId":f.org,"memberId":member["id"],"role":"reader"}),
        f.now,
    )
    .await
    .unwrap();
    let team = svc
        .create_team(
            &f.owner_token,
            false,
            &json!({"organizationId":f.org,"name":"Custom"}),
            f.now,
        )
        .await
        .unwrap();
    svc.update_team(
        &f.owner_token,
        false,
        &json!({"teamId":team["id"],"data":{"organizationId":f.org,"name":"Changed"}}),
        f.now,
    )
    .await
    .unwrap();
    let body = json!({"organizationId":f.org,"teamId":team["id"],"userId":f.recipient});
    svc.add_team_member(&f.owner_token, false, &body, f.now)
        .await
        .unwrap();
    svc.remove_team_member(&f.owner_token, false, &body, f.now)
        .await
        .unwrap();
    svc.remove_member(
        &f.owner_token,
        false,
        &json!({"organizationId":f.org,"memberIdOrEmail":member["id"]}),
        f.now,
    )
    .await
    .unwrap();
    svc.delete(&f.owner_token, false, &f.org, f.now)
        .await
        .unwrap();
}
#[cfg(feature = "http")]
async fn http<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use tower::ServiceExt;
    let f = fixture(store.as_ref(), "http", "boss").await;
    let now = f.now;
    let app =
        crate::http::AuthHttp::new(store, "http://localhost:3000".into(), vec![7; 32], vec![])
            .unwrap()
            .with_access_control(configured())
            .with_organization_controls(crate::organizations::OrganizationControls::default())
            .with_clock(move || now)
            .with_rate_limiting(false);
    let cookie =
        (crate::cookies::SessionCookie { secure: false }).serialize(&f.owner_token, &[7; 32], true);
    let router = app.router();
    let ordered = Request::builder()
        .method("POST")
        .uri("/organization/has-permission")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, cookie.split(';').next().unwrap())
        .body(Body::from(format!(
            r#"{{"organizationId":"{}","permissions":{{"toString":["read"],"0":["read"]}}}}"#,
            f.org
        )))
        .unwrap();
    let response = router.clone().oneshot(ordered).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(body["success"], false);

    for (path, payload, status, expected) in [
        (
            "/organization/has-permission",
            json!({"organizationId":f.org,"permissions":{"project":["write"]},"roles":{}}),
            200,
            Some(json!({"error":null,"success":true})),
        ),
        (
            "/organization/has-permission",
            json!({"organizationId":f.org,"permissions":{"__proto__":42}}),
            200,
            Some(json!({"error":null,"success":false})),
        ),
        (
            "/organization/has-permission",
            json!({"organizationId":f.org,"permissions":{"constructor":["read"]}}),
            500,
            None,
        ),
        (
            "/organization/create-role",
            json!({"organizationId":f.org,"role":"http-proto","permission":{"__proto__":42,"project":["read"]}}),
            200,
            None,
        ),
        (
            "/organization/update-role",
            json!({"organizationId":f.org,"roleName":"http-proto","data":{"permission":{"__proto__":42,"project":["write"]}}}),
            200,
            None,
        ),
    ] {
        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://localhost:3000")
            .header(header::COOKIE, cookie.split(';').next().unwrap())
            .body(Body::from(payload.to_string()))
            .unwrap();
        let response = router.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status().as_u16(), status);
        let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
        if status == 500 {
            assert!(bytes.is_empty());
            continue;
        }
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        if let Some(expected) = expected {
            assert_eq!(body, expected);
        } else {
            assert!(body["roleData"]["permission"].get("__proto__").is_none());
        }
    }
}
#[tokio::test]
async fn declarative_access_control_replacement_admission_and_scoped_merges() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}
#[test]
fn access_control_configuration_is_bounded_before_use() {
    assert!(
        OrganizationAccessControl::new(
            std::collections::BTreeMap::default(),
            std::collections::BTreeMap::default()
        )
        .is_ok()
    );
    let roles = (0..129)
        .map(|i| (i.to_string(), std::collections::BTreeMap::default()))
        .collect();
    assert!(OrganizationAccessControl::new(std::collections::BTreeMap::default(), roles).is_err());
    let statements = std::collections::BTreeMap::from([("x".repeat(4097), vec![])]);
    assert!(
        OrganizationAccessControl::new(statements, std::collections::BTreeMap::default()).is_err()
    );
    let statements = std::collections::BTreeMap::from([("x".into(), vec!["a".into(); 4096])]);
    assert!(
        OrganizationAccessControl::new(statements, std::collections::BTreeMap::default()).is_err()
    );
}

#[allow(clippy::too_many_lines)]
async fn prototypes<S: AuthStore + 'static>(store: Arc<S>) {
    for (i, name) in ["", "__proto__", "constructor", "toString"]
        .into_iter()
        .enumerate()
    {
        for own in [false, true] {
            let f = fixture(store.as_ref(), &format!("proto-role-{i}-{own}"), name).await;
            let roles = if own {
                json!({name:{"project":["read"]}})
            } else {
                json!({})
            };
            let svc = OrganizationService::new(store.clone())
                .with_access_control(policy(json!({"project":["read"]}), roles));
            let out = svc
                .has_permission(
                    &f.owner_token,
                    false,
                    &json!({"organizationId":f.org,"permissions":{"project":["read"]}}),
                    f.now,
                )
                .await;
            if own || name.is_empty() {
                assert_eq!(out.unwrap()["success"], own);
            } else {
                assert_eq!(out.unwrap_err().code, "HTTP_INTERNAL_EMPTY");
            }
        }
        let f = fixture(store.as_ref(), &format!("proto-resource-{i}"), "boss").await;
        let statements = json!({name:["read"]});
        let svc = OrganizationService::new(store.clone())
            .with_access_control(policy(statements.clone(), json!({"boss":statements})));
        assert_eq!(
            svc.has_permission(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"permissions":{name:["read"]}}),
                f.now
            )
            .await
            .unwrap()["success"],
            name != "__proto__"
        );
    }
    for (i, permissions, error) in [
        (0, json!({"toString":["read"],"0":["read"]}), false),
        (1, json!({"toString":["read"],"absent":["read"]}), true),
        (2, json!({"absent":["read"],"toString":["read"]}), false),
        (3, json!({"toString":["read"],"01":["read"]}), true),
        (4, json!({"toString":["read"],"4294967294":["read"]}), false),
        (5, json!({"toString":["read"],"4294967295":["read"]}), true),
    ] {
        let f = fixture(store.as_ref(), &format!("resource-order-{i}"), "boss").await;
        let svc = OrganizationService::new(store.clone()).with_access_control(configured());
        let out = svc
            .has_permission(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"permissions":permissions}),
                f.now,
            )
            .await;
        if error {
            assert_eq!(out.unwrap_err().code, "HTTP_INTERNAL_EMPTY");
        } else {
            assert_eq!(out.unwrap()["success"], false);
        }
    }
    for (i, actor, success) in [
        (0, "boss,constructor", true),
        (1, "constructor,boss", false),
    ] {
        let f = fixture(store.as_ref(), &format!("shortcircuit-{i}"), actor).await;
        let svc = OrganizationService::new(store.clone()).with_access_control(configured());
        let out = svc
            .has_permission(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"permissions":{"project":["read"]}}),
                f.now,
            )
            .await;
        if success {
            assert_eq!(out.unwrap()["success"], true);
        } else {
            assert_eq!(out.unwrap_err().code, "HTTP_INTERNAL_EMPTY");
        }
    }
    for own in [false, true] {
        let f = fixture(store.as_ref(), &format!("dynamic-proto-{own}"), "boss").await;
        let statements = if own {
            json!({"constructor":["read"]})
        } else {
            json!({})
        };
        let svc = OrganizationService::new(store.clone()).with_access_control(policy(
            json!({"constructor":["read","write"]}),
            json!({"boss":statements}),
        ));
        grant(
            store.as_ref(),
            &f,
            "dynamic",
            "boss",
            "{\"constructor\":[\"write\"]}",
        )
        .await;
        let out = svc
            .has_permission(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"permissions":{"constructor":["write"]}}),
                f.now,
            )
            .await;
        if own {
            assert_eq!(out.unwrap()["success"], true);
        } else {
            assert_eq!(out.unwrap_err().code, "HTTP_INTERNAL_EMPTY");
        }
    }
    let f = fixture(store.as_ref(), "proto-admission", "boss").await;
    let svc = OrganizationService::new(store.clone()).with_access_control(configured());
    let created=svc.create_organization_role(&f.owner_token,false,&json!({"organizationId":f.org,"role":"proto-record","permission":{"__proto__":42,"project":["read"]}}),f.now).await.unwrap();
    assert_eq!(created["statements"], json!({"project":["read"]}));
    let changed=svc.update_organization_role(&f.owner_token,false,&json!({"organizationId":f.org,"roleName":"proto-record","data":{"permission":{"__proto__":42,"project":["write"]}}}),f.now).await.unwrap();
    assert_eq!(
        changed["roleData"]["permission"],
        json!({"project":["write"]})
    );
    grant(
        store.as_ref(),
        &f,
        "invalid-proto-value",
        "boss",
        "{\"__proto__\":42}",
    )
    .await;
    assert_eq!(
        svc.has_permission(
            &f.owner_token,
            false,
            &json!({"organizationId":f.org,"permissions":{"project":["read"]}}),
            f.now
        )
        .await
        .unwrap()["success"],
        true
    );
}

#[allow(clippy::too_many_lines)]
async fn dynamic_prototypes<S: AuthStore + 'static>(store: Arc<S>) {
    for (i, actor, rows, request, own, expected) in [
        (
            0,
            "__proto__",
            vec![
                ("__proto__", json!({"project":["read"]})),
                ("__proto__", json!({"project":["write"]})),
            ],
            json!({"project":["read","write"]}),
            false,
            Some(true),
        ),
        (
            1,
            "authorize",
            vec![
                ("__proto__", json!({"project":["read"]})),
                ("authorize", json!({"project":["write"]})),
            ],
            json!({"project":["write"]}),
            false,
            Some(true),
        ),
        (
            2,
            "statements",
            vec![
                ("__proto__", json!({"statements":["readwrite"]})),
                ("statements", json!({})),
            ],
            json!({"0":["read"]}),
            false,
            Some(true),
        ),
        (
            3,
            "statements",
            vec![
                ("__proto__", json!({"statements":["readwrite"]})),
                ("statements", json!({"0":["other"]})),
            ],
            json!({"0":["r","other"]}),
            false,
            Some(true),
        ),
        (
            4,
            "authorize",
            vec![("__proto__", json!({"project":["read"]}))],
            json!({"project":["read"]}),
            true,
            Some(false),
        ),
        (
            5,
            "__proto__",
            vec![("__proto__", json!({"project":["read"]}))],
            json!({"project":["read"]}),
            false,
            Some(true),
        ),
        (
            6,
            "authorize",
            vec![("__proto__", json!({"project":["read"]}))],
            json!({"project":["read"]}),
            false,
            None,
        ),
        (
            7,
            "statements",
            vec![("__proto__", json!({"project":["read"]}))],
            json!({"project":["read"]}),
            false,
            None,
        ),
        (
            8,
            "constructor",
            vec![("__proto__", json!({"project":["read"]}))],
            json!({"project":["read"]}),
            false,
            None,
        ),
    ] {
        let f = fixture(store.as_ref(), &format!("dynamic-prototype-{i}"), actor).await;
        for (index, (role, permission)) in rows.into_iter().enumerate() {
            grant(
                store.as_ref(),
                &f,
                &format!("row-{index}"),
                role,
                &permission.to_string(),
            )
            .await;
        }
        let roles = if own {
            json!({"__proto__":{}})
        } else {
            json!({})
        };
        let svc =
            OrganizationService::new(store.clone()).with_access_control(policy(json!({}), roles));
        let result = svc
            .has_permission(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"permissions":request}),
                f.now,
            )
            .await;
        if let Some(expected) = expected {
            assert_eq!(result.unwrap()["success"], expected, "case {i}");
        } else {
            assert_eq!(result.unwrap_err().code, "HTTP_INTERNAL_EMPTY", "case {i}");
        }
    }
}
