use super::access_control::{fixture, grant};
use super::*;
use crate::organizations::{
    OrganizationAccessControl, OrganizationAuthorizationRequest, OrganizationRoleAuthorizer,
    OrganizationRoleAuthorizers,
};
use std::{collections::BTreeMap, sync::Mutex};
#[derive(Default)]
struct Recorder {
    allow: bool,
    fail: bool,
    calls: Mutex<Vec<(String, Vec<String>)>>,
}
impl OrganizationRoleAuthorizer for Recorder {
    fn authorize(&self, request: OrganizationAuthorizationRequest<'_>) -> BlueprintResult<bool> {
        assert_eq!(request.statements().get("project").unwrap(), &["read"]);
        assert_eq!(request.permissions().len(), request.entries().count());
        self.calls.lock().unwrap().push((
            request.role().into(),
            request.entries().map(|(key, _)| key.to_owned()).collect(),
        ));
        if self.fail {
            Err(BlueprintError::new("OWNED_SECRET_ERROR", "must-not-leak"))
        } else {
            Ok(self.allow)
        }
    }
}
fn policy(roles: &[&str]) -> OrganizationAccessControl {
    OrganizationAccessControl::new(
        serde_json::from_value(json!({"organization":["update","delete"],"member":["update","delete"],"team":["create","update","delete"],"invitation":["create","cancel"],"ac":["create","read","update","delete"],"project":["read","write"]})).unwrap(),
        roles.iter().map(|role|((*role).into(),BTreeMap::from([("project".into(),vec!["read".into()])]))).collect(),
    ).unwrap()
}
fn registry(
    roles: &[&str],
    selected: &str,
    callback: Arc<Recorder>,
) -> OrganizationRoleAuthorizers {
    let callback: Arc<dyn OrganizationRoleAuthorizer> = callback;
    OrganizationRoleAuthorizers::new(policy(roles), BTreeMap::from([(selected.into(), callback)]))
        .unwrap()
}
fn count(callback: &Recorder) -> usize {
    callback.calls.lock().unwrap().len()
}

#[test]
fn authorizer_registry_requires_exact_own_static_keys() {
    for name in ["constructor", "__proto__", "toString", "absent", ""] {
        let callback: Arc<dyn OrganizationRoleAuthorizer> = Arc::new(Recorder::default());
        assert!(
            OrganizationRoleAuthorizers::new(
                policy(&["custom"]),
                BTreeMap::from([(name.into(), callback.clone())])
            )
            .is_err()
        );
        assert!(
            OrganizationRoleAuthorizers::new(
                policy(&[name]),
                BTreeMap::from([(name.into(), callback)])
            )
            .is_ok()
        );
    }
}

#[allow(clippy::too_many_lines)]
pub(super) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    for (i, actor, allow, fail, replaced, expected, calls) in [
        (0, "custom", true, false, false, Some(true), 1),
        (1, "custom", false, false, false, Some(false), 1),
        (2, "custom", true, true, false, None, 1),
        (3, "custom,custom", false, false, false, Some(false), 2),
        (4, "custom,constructor", true, false, false, Some(true), 1),
        (5, "constructor,custom", true, false, false, None, 0),
        (6, "custom", true, false, true, Some(false), 0),
    ] {
        let f = fixture(store.as_ref(), &format!("authorizer-{i}"), actor).await;
        let callback = Arc::new(Recorder {
            allow,
            fail,
            ..Recorder::default()
        });
        let svc = OrganizationService::new(store.clone()).with_role_authorizers(registry(
            &["custom"],
            "custom",
            callback.clone(),
        ));
        if replaced {
            grant(
                store.as_ref(),
                &f,
                "dynamic",
                "custom",
                "{\"project\":[\"read\"]}",
            )
            .await;
        }
        let action = if i == 1 { "read" } else { "write" };
        let result = svc
            .has_permission(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"permissions":{"project":[action]}}),
                f.now,
            )
            .await;
        if let Some(expected) = expected {
            assert_eq!(result.unwrap()["success"], expected);
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.code, "HTTP_INTERNAL_EMPTY");
            assert!(!error.message.contains("must-not-leak"));
        }
        assert_eq!(count(&callback), calls, "case {i}");
        if replaced {
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
            assert_eq!(count(&callback), 0);
        }
    }
    for (i, name) in ["__proto__", "constructor", "toString", ""]
        .into_iter()
        .enumerate()
    {
        let f = fixture(store.as_ref(), &format!("authorizer-own-{i}"), name).await;
        let callback = Arc::new(Recorder {
            allow: true,
            ..Recorder::default()
        });
        let svc = OrganizationService::new(store.clone()).with_role_authorizers(registry(
            &[name],
            name,
            callback.clone(),
        ));
        assert_eq!(
            svc.has_permission(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"permissions":{"project":["write"]}}),
                f.now
            )
            .await
            .unwrap()["success"],
            true
        );
        assert_eq!(count(&callback), 1);
    }
    for reverse in [false, true] {
        let f = fixture(
            store.as_ref(),
            &format!("authorizer-order-{reverse}"),
            "custom",
        )
        .await;
        let callback = Arc::new(Recorder {
            allow: true,
            ..Recorder::default()
        });
        let callbacks = registry(&["custom"], "custom", callback.clone());
        let svc = OrganizationService::new(store.clone());
        let svc = if reverse {
            svc.with_role_authorizers(callbacks)
                .with_access_control(policy(&["custom"]))
        } else {
            svc.with_access_control(policy(&["other"]))
                .with_role_authorizers(callbacks)
        };
        assert_eq!(
            svc.has_permission(
                &f.owner_token,
                false,
                &json!({"organizationId":f.org,"permissions":{"project":["write"]}}),
                f.now
            )
            .await
            .unwrap()["success"],
            !reverse
        );
        assert_eq!(count(&callback), usize::from(!reverse));
    }
    for malformed in [false, true] {
        let f = fixture(
            store.as_ref(),
            &format!("authorizer-creator-{malformed}"),
            "custom",
        )
        .await;
        let callback = Arc::new(Recorder::default());
        let svc = OrganizationService::new(store.clone())
            .with_role_authorizers(registry(&["custom"], "custom", callback.clone()))
            .with_creator_role(crate::organizations::CreatorRole::new("custom").unwrap());
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(Model::Member,json!({"id":format!("{}-target",f.org),"organizationId":f.org,"userId":f.recipient,"role":"member"})).await.unwrap();
        tx.commit().await.unwrap();
        if malformed {
            grant(store.as_ref(), &f, "bad", "other", "invalid").await;
        }
        let result=svc.update_member_role(&f.owner_token,false,&json!({"organizationId":f.org,"memberId":format!("{}-target",f.org),"role":"admin"}),f.now).await;
        if malformed {
            assert_eq!(result.unwrap_err().code, "HTTP_INTERNAL_EMPTY");
        } else {
            assert_eq!(result.unwrap()["role"], "admin");
        }
        assert_eq!(count(&callback), 0);
    }
    let f = fixture(store.as_ref(), "authorizer-request", "boss").await;
    let callback = Arc::new(Recorder {
        allow: true,
        ..Recorder::default()
    });
    let svc = OrganizationService::new(store.clone()).with_role_authorizers(registry(
        &["boss", "reader"],
        "boss",
        callback.clone(),
    ));
    assert!(
        svc.has_permission(
            "invalid-token",
            false,
            &json!({"organizationId":f.org,"permissions":{}}),
            f.now
        )
        .await
        .is_err()
    );
    assert_eq!(count(&callback), 0);
    assert_eq!(
        svc.has_permission(
            &f.owner_token,
            false,
            &json!({"organizationId":f.org,"permissions":{"tail":["write"],"0":["read"]}}),
            f.now
        )
        .await
        .unwrap()["success"],
        true
    );
    assert_eq!(callback.calls.lock().unwrap()[0].1, ["0", "tail"]);
    assert_eq!(
        svc.has_permission(
            &f.owner_token,
            false,
            &json!({"organizationId":f.org,"permissions":{"__proto__":42}}),
            f.now
        )
        .await
        .unwrap()["success"],
        true
    );
    assert!(callback.calls.lock().unwrap()[1].1.is_empty());
    svc.create_invitation(
        &f.owner_token,
        false,
        &super::invitation_policy::request(&f),
        f.now,
    )
    .await
    .unwrap();
    super::access_control::lifecycle(
        store.clone(),
        Some(registry(&["boss", "reader"], "boss", callback.clone())),
    )
    .await;
    {
        let calls = callback.calls.lock().unwrap();
        for resource in ["invitation", "organization", "team", "member", "ac"] {
            assert!(
                calls
                    .iter()
                    .any(|(_, keys)| keys.iter().any(|key| key == resource)),
                "{resource}"
            );
        }
    }
    #[cfg(feature = "http")]
    http(store).await;
}

#[tokio::test]
async fn synchronous_authorizers_order_replacement_authority_and_lifecycle() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}

#[cfg(feature = "http")]
async fn http<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use tower::ServiceExt;
    for (reverse, fail) in [(false, false), (true, false), (false, true)] {
        let f = fixture(
            store.as_ref(),
            &format!("authorizer-http-{reverse}-{fail}"),
            "custom",
        )
        .await;
        let callback = Arc::new(Recorder {
            allow: true,
            fail,
            ..Recorder::default()
        });
        let callbacks = registry(&["custom"], "custom", callback.clone());
        let now = f.now;
        let app = crate::http::AuthHttp::new(
            store.clone(),
            "http://localhost:3000".into(),
            vec![7; 32],
            vec![],
        )
        .unwrap()
        .with_clock(move || now)
        .with_rate_limiting(false);
        let app = if reverse {
            app.with_role_authorizers(callbacks)
                .with_access_control(policy(&["custom"]))
        } else {
            app.with_access_control(policy(&["other"]))
                .with_role_authorizers(callbacks)
        };
        let cookie = (crate::cookies::SessionCookie { secure: false }).serialize(
            &f.owner_token,
            &[7; 32],
            true,
        );
        let (path, body) = if fail {
            (
                "/organization/update",
                json!({"organizationId":f.org,"data":{"name":"Unauthorized change"}}),
            )
        } else {
            (
                "/organization/has-permission",
                json!({"organizationId":f.org,"permissions":{"__proto__":42,"project":["write"]},"roles":{}}),
            )
        };
        let response = app
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ORIGIN, "http://localhost:3000")
                    .header(header::COOKIE, cookie.split(';').next().unwrap())
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), if fail { 500 } else { 200 });
        let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
        if fail {
            assert!(bytes.is_empty());
            let mut tx = store.begin(false).await.unwrap();
            assert_eq!(
                tx.get(Model::Organization, &f.org).await.unwrap().unwrap()["name"],
                "Invitation Org"
            );
            tx.commit().await.unwrap();
        } else {
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap()["success"],
                !reverse
            );
        }
        assert_eq!(count(&callback), usize::from(!reverse));
        if !reverse {
            assert_eq!(
                callback.calls.lock().unwrap()[0].1,
                if fail {
                    vec!["organization"]
                } else {
                    vec!["project"]
                }
            );
        }
    }
}
