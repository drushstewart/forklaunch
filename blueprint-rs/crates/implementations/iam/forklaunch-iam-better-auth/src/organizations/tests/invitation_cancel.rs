use super::invitation_policy::{request, rows, seed};
use super::*;
use crate::organizations::InvitationPolicy;
use std::sync::Mutex;
mod store;
use store::{AfterCancel, Failure, Store};

#[allow(clippy::too_many_lines)]
pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>) {
    for (label, limit, extra, code, status, count) in [
        (
            "quota",
            0,
            json!({}),
            Some("INVITATION_LIMIT_REACHED"),
            "canceled",
            1,
        ),
        (
            "team",
            100,
            json!({"teamId":"missing-team"}),
            Some("TEAM_NOT_FOUND"),
            "canceled",
            1,
        ),
        (
            "role",
            100,
            json!({"role":"missing-role"}),
            Some("ROLE_NOT_FOUND: missing-role"),
            "pending",
            1,
        ),
        (
            "resend",
            0,
            json!({"resend":true,"teamId":"missing-team"}),
            None,
            "pending",
            1,
        ),
        ("success", 100, json!({}), None, "canceled", 2),
    ] {
        let f = seed(store.as_ref(), &format!("cancel-{label}")).await;
        let old = OrganizationService::new(store.clone())
            .create_invitation(&f.owner_token, false, &request(&f), f.now)
            .await
            .unwrap();
        let mut body = request(&f);
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let svc = OrganizationService::new(store.clone())
            .with_cancel_pending_invitations_on_reinvite(true)
            .with_invitation_policy(InvitationPolicy::new(10, limit, true));
        let out = svc
            .create_invitation(&f.owner_token, false, &body, f.now)
            .await;
        if let Some(code) = code {
            let e = out.err().unwrap();
            if label == "role" {
                assert!(e.message.contains("missing-role"));
            } else {
                assert_eq!(e.code, code);
            }
        } else {
            assert!(out.is_ok());
        }
        let current = rows(store.as_ref(), &f.org).await;
        assert_eq!(current.len(), count);
        assert_eq!(
            current.iter().find(|r| r["id"] == old["id"]).unwrap()["status"],
            status
        );
    }
    duplicate_cases(store.clone()).await;
    saturated_window(store.clone()).await;
    failure_cases(store.clone()).await;
    #[cfg(feature = "http")]
    http_cancel(store.clone()).await;
    for action in ["role", "session", "delete", "change"] {
        let f = seed(store.as_ref(), &format!("cancel-between-{action}")).await;
        let old = OrganizationService::new(store.clone())
            .create_invitation(&f.owner_token, false, &request(&f), f.now)
            .await
            .unwrap();
        let id = text(&old, "id").unwrap().to_owned();
        let mutation = match action {
            "role" => AfterCancel::Delete(Model::OrganizationRole, format!("{}-grant", f.org)),
            "session" => {
                let mut tx = store.begin(false).await.unwrap();
                let session = tx
                    .find_unique(Model::Session, "token", &f.owner_token)
                    .await
                    .unwrap()
                    .unwrap();
                tx.commit().await.unwrap();
                AfterCancel::Delete(Model::Session, text(&session, "id").unwrap().to_owned())
            }
            "delete" => AfterCancel::Delete(Model::Invitation, id.clone()),
            _ => {
                let mut changed = old.clone();
                changed["status"] = json!("canceled");
                changed["role"] = json!("viewer");
                AfterCancel::Replace(Model::Invitation, changed)
            }
        };
        let wrapped = Arc::new(Store {
            inner: store.clone(),
            after: Mutex::new(Some(mutation)),
            failure: Failure::None,
        });
        let svc =
            OrganizationService::new(wrapped).with_cancel_pending_invitations_on_reinvite(true);
        let e = svc
            .create_invitation(&f.owner_token, false, &request(&f), f.now)
            .await
            .err()
            .unwrap();
        assert_eq!(
            e.code,
            match action {
                "role" => "YOU_ARE_NOT_ALLOWED_TO_INVITE_USERS_TO_THIS_ORGANIZATION",
                "session" => "UNAUTHORIZED",
                _ => "INVITATION_STATE_CHANGED",
            }
        );
        let current = rows(store.as_ref(), &f.org).await;
        assert_eq!(current.len(), usize::from(action != "delete"));
        assert!(current.iter().all(|r| r["status"] == "canceled"));
    }
    for resend in [false, true] {
        let f = seed(store.as_ref(), &format!("cancel-race-{resend}")).await;
        let old = OrganizationService::new(store.clone())
            .create_invitation(&f.owner_token, false, &request(&f), f.now)
            .await
            .unwrap();
        let ready = Arc::new(tokio::sync::Notify::new());
        let resume = Arc::new(tokio::sync::Notify::new());
        let wrapped = Arc::new(Store {
            inner: store.clone(),
            after: Mutex::new(Some(AfterCancel::Pause(ready.clone(), resume.clone()))),
            failure: Failure::None,
        });
        let svc =
            OrganizationService::new(wrapped).with_cancel_pending_invitations_on_reinvite(true);
        let body = request(&f);
        let task = async {
            ready.notified().await;
            let mut other = body.clone();
            other["resend"] = json!(resend);
            let r = OrganizationService::new(store.clone())
                .with_cancel_pending_invitations_on_reinvite(true)
                .create_invitation(&f.owner_token, false, &other, f.now)
                .await
                .unwrap();
            resume.notify_one();
            r
        };
        let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(
                svc.create_invitation(&f.owner_token, false, &body, f.now),
                task
            )
        })
        .await
        .unwrap();
        assert_eq!(
            first.err().unwrap().code,
            "USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION"
        );
        assert_ne!(old["id"], second["id"]);
        let current = rows(store.as_ref(), &f.org).await;
        assert_eq!(current.len(), 2);
        assert_eq!(
            current.iter().filter(|r| r["status"] == "pending").count(),
            1
        );
    }
}
async fn failure_cases<S: AuthStore + 'static>(store: Arc<S>) {
    for (index, failure) in [
        Failure::CancelCommit,
        Failure::Insert,
        Failure::ReplacementCommit,
        Failure::CancelAcknowledgement,
        Failure::ReplacementAcknowledgement,
    ]
    .into_iter()
    .enumerate()
    {
        let f = seed(store.as_ref(), &format!("cancel-failure-{index}")).await;
        OrganizationService::new(store.clone())
            .create_invitation(&f.owner_token, false, &request(&f), f.now)
            .await
            .unwrap();
        let wrapped = Arc::new(Store {
            inner: store.clone(),
            after: Mutex::new(None),
            failure,
        });
        let e = OrganizationService::new(wrapped)
            .with_cancel_pending_invitations_on_reinvite(true)
            .create_invitation(&f.owner_token, false, &request(&f), f.now)
            .await
            .err()
            .unwrap();
        assert_eq!(e.code, "better_auth_storage_error");
        let current = rows(store.as_ref(), &f.org).await;
        assert_eq!(
            current.len(),
            if failure == Failure::ReplacementAcknowledgement {
                2
            } else {
                1
            }
        );
        assert_eq!(
            current
                .iter()
                .filter(|row| row["status"] == "pending")
                .count(),
            usize::from(matches!(
                failure,
                Failure::CancelCommit | Failure::ReplacementAcknowledgement
            ))
        );
        assert_eq!(
            current[0]["status"],
            if failure == Failure::CancelCommit {
                "pending"
            } else {
                "canceled"
            }
        );
    }
}
#[tokio::test]
async fn invitation_reinvite_cancellation_commits_and_rechecks() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}

#[cfg(feature = "http")]
async fn http_cancel<S: AuthStore + 'static>(store: Arc<S>) {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use tower::ServiceExt;
    for enabled in [false, true] {
        let f = seed(store.as_ref(), &format!("cancel-http-{enabled}")).await;
        let now = f.now;
        OrganizationService::new(store.clone())
            .create_invitation(&f.owner_token, false, &request(&f), now)
            .await
            .unwrap();
        let app = crate::http::AuthHttp::new(
            store.clone(),
            "http://localhost:3000".into(),
            vec![7; 32],
            vec![],
        )
        .unwrap()
        .with_cancel_pending_invitations_on_reinvite(enabled)
        .with_invitation_policy(InvitationPolicy::new(10, 0, true))
        .with_rate_limiting(false)
        .with_clock(move || now);
        let cookie = (crate::cookies::SessionCookie { secure: false }).serialize(
            &f.owner_token,
            &[7; 32],
            true,
        );
        let mut body = request(&f);
        body["cancelPendingInvitationsOnReInvite"] = json!(!enabled);
        let req = Request::builder()
            .method("POST")
            .uri("/organization/invite-member")
            .header("origin", "http://localhost:3000")
            .header("content-type", "application/json")
            .header("cookie", cookie)
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.router().oneshot(req).await.unwrap();
        assert_eq!(
            response.status(),
            if enabled {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::BAD_REQUEST
            }
        );
        assert!(!response.headers().contains_key(header::SET_COOKIE));
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(
            body["code"],
            if enabled {
                "INVITATION_LIMIT_REACHED"
            } else {
                "USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION"
            }
        );
        let current = rows(store.as_ref(), &f.org).await;
        assert_eq!(current.len(), 1);
        assert_eq!(
            current[0]["status"],
            if enabled { "canceled" } else { "pending" }
        );
    }
}

async fn duplicate_cases<S: AuthStore + 'static>(store: Arc<S>) {
    for changed in [false, true] {
        let f = seed(store.as_ref(), &format!("cancel-duplicates-{changed}")).await;
        let old = OrganizationService::new(store.clone())
            .create_invitation(&f.owner_token, false, &request(&f), f.now)
            .await
            .unwrap();
        let mut second = old.clone();
        second["id"] = json!(format!("{}-second", f.org));
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(Model::Invitation, second.clone()).await.unwrap();
        tx.commit().await.unwrap();
        let after = if changed {
            second["role"] = json!("viewer");
            Some(AfterCancel::Replace(Model::Invitation, second.clone()))
        } else {
            None
        };
        let wrapped = Arc::new(Store {
            inner: store.clone(),
            after: Mutex::new(after),
            failure: Failure::None,
        });
        let result = OrganizationService::new(wrapped)
            .with_cancel_pending_invitations_on_reinvite(true)
            .create_invitation(&f.owner_token, false, &request(&f), f.now)
            .await;
        if changed {
            assert_eq!(
                result.err().unwrap().code,
                "USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION"
            );
        } else {
            assert!(result.is_ok());
        }
        let current = rows(store.as_ref(), &f.org).await;
        assert_eq!(current.len(), if changed { 2 } else { 3 });
        assert_eq!(
            current.iter().find(|row| row["id"] == old["id"]).unwrap()["status"],
            "canceled"
        );
        assert_eq!(
            current
                .iter()
                .find(|row| row["id"] == second["id"])
                .unwrap(),
            &second
        );
    }
}

async fn saturated_window<S: AuthStore + 'static>(store: Arc<S>) {
    for count in [100u32, 101] {
        let f = seed(store.as_ref(), &format!("cancel-window-{count}")).await;
        let old = OrganizationService::new(store.clone())
            .create_invitation(&f.owner_token, false, &request(&f), f.now)
            .await
            .unwrap();
        let mut tx = store.begin(true).await.unwrap();
        for i in 1..count {
            let mut next = old.clone();
            next["id"] = json!(format!("{}-{i}", f.org));
            tx.insert(Model::Invitation, next).await.unwrap();
        }
        tx.commit().await.unwrap();
        let result = OrganizationService::new(store.clone())
            .with_cancel_pending_invitations_on_reinvite(true)
            .with_invitation_policy(InvitationPolicy::new(10, count, true))
            .create_invitation(&f.owner_token, false, &request(&f), f.now)
            .await
            .unwrap();
        assert_ne!(result["id"], old["id"]);
        let current = rows(store.as_ref(), &f.org).await;
        assert_eq!(current.len(), usize::try_from(count).unwrap() + 1);
        assert_eq!(
            current
                .iter()
                .filter(|row| row["status"] == "canceled")
                .count(),
            1
        );
        assert_eq!(
            current.iter().find(|row| row["id"] == old["id"]).unwrap()["status"],
            "canceled"
        );
    }
}
