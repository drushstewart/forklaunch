use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
struct Hook {
    calls: AtomicUsize,
    fail: bool,
}
impl<S: AuthStore> RelayHooks<S> for Hook {
    async fn establish(
        &self,
        tokens: RelayTokens,
        _store: Arc<S>,
    ) -> BlueprintResult<RelayEstablishment> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(tokens.access_token.as_deref(), Some("relay-private-token"));
        if self.fail {
            return Err(invalid());
        }
        Ok(RelayEstablishment {
            owner_user_id: Some("relay-user".into()),
            active_organization_id: Some("relay-org".into()),
            redirect_path: "/dashboard?q=1".into(),
        })
    }
}
fn tokens() -> RelayTokens {
    RelayTokens {
        access_token: Some("relay-private-token".into()),
        ..RelayTokens::default()
    }
}
fn ticket(result: &RelayIngestResult) -> &str {
    result
        .redirect_path
        .strip_prefix("/relay/handoff?ticket=")
        .unwrap()
}
#[allow(clippy::too_many_lines)]
pub(crate) async fn exercise<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User,json!({"id":"relay-user","email":"relay@example.test","createdAt":stamp(now),"updatedAt":stamp(now),"emailVerified":false})).await.unwrap();
    tx.commit().await.unwrap();
    let hooks = Arc::new(Hook {
        calls: AtomicUsize::new(0),
        fail: false,
    });
    let service = RelayService::new(
        store.clone(),
        hooks.clone(),
        SessionCookie { secure: false },
        b"relay-fixture-secret".to_vec(),
    );
    let (first, second) = tokio::join!(
        service.ingest("relay-nonce", tokens(), now),
        service.ingest("relay-nonce", tokens(), now)
    );
    let successful = match (first, second) {
        (Ok(value), Err(error)) | (Err(error), Ok(value)) => {
            assert_eq!(error.code, "relay_replay");
            value
        }
        _ => panic!("exactly one nonce claimant must succeed"),
    };
    assert_eq!(hooks.calls.load(Ordering::SeqCst), 1);
    let id = ticket(&successful);
    let (first, second) = tokio::join!(
        service.redeem_ticket(id, now),
        service.redeem_ticket(id, now)
    );
    let ((Some(successful), None) | (None, Some(successful))) = (first.unwrap(), second.unwrap())
    else {
        panic!("exactly one browser must redeem a ticket");
    };
    assert_eq!(successful.redirect_to, "/dashboard?q=1");
    let cookie = successful.cookie.unwrap();
    let signed = crate::cookies::cookie_value(&cookie, "better-auth.session_token").unwrap();
    let token = crate::cookies::verify_cookie_value(&signed, b"relay-fixture-secret").unwrap();
    assert_eq!(token.len(), 43);
    let session = crate::sessions::SessionService::new(store.clone())
        .get(token, false, true, now)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.user["id"], "relay-user");
    assert_eq!(session.session["activeOrganizationId"], "relay-org");
    assert_eq!(session.session["expiresAt"], stamp(now + Duration::days(1)));
    assert!(service.redeem_ticket(id, now).await.unwrap().is_none());
    let mut tx = store.begin(false).await.unwrap();
    let row = tx
        .get(Model::RelaySessionHandoff, id)
        .await
        .unwrap()
        .unwrap();
    assert!(!row.to_string().contains("relay-private-token"));
    assert!(
        tx.find_unique(Model::RelaySessionHandoff, "nonce", "relay-nonce")
            .await
            .unwrap()
            .is_some()
    );
    tx.commit().await.unwrap();
    let expired = service
        .ingest("expired-relay", tokens(), now)
        .await
        .unwrap();
    assert!(
        service
            .redeem_ticket(ticket(&expired), now + Duration::minutes(5))
            .await
            .unwrap()
            .is_none()
    );
    let default = RelayService::new(
        store.clone(),
        Arc::new(DefaultRelayHooks),
        SessionCookie { secure: true },
        b"relay-fixture-secret".to_vec(),
    );
    let default_ticket = default
        .ingest("default-relay", RelayTokens::default(), now)
        .await
        .unwrap();
    let redeemed = default
        .redeem_ticket(ticket(&default_ticket), now)
        .await
        .unwrap()
        .unwrap();
    assert!(redeemed.cookie.is_none());
    assert_eq!(redeemed.redirect_to, "/");
    let failed_hook = Arc::new(Hook {
        calls: AtomicUsize::new(0),
        fail: true,
    });
    let failed = RelayService::new(
        store,
        failed_hook.clone(),
        SessionCookie { secure: false },
        b"relay-fixture-secret".to_vec(),
    );
    assert!(failed.ingest("failed-relay", tokens(), now).await.is_err());
    assert_eq!(
        failed
            .ingest("failed-relay", tokens(), now)
            .await
            .err()
            .unwrap()
            .code,
        "relay_replay"
    );
    assert_eq!(failed_hook.calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn nonce_hook_failure_atomic_redemption_session_cookie_and_expiry() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}
#[test]
fn root_relative_redirects_match_the_template() {
    for value in [
        None,
        Some(""),
        Some("https://foreign.test"),
        Some("//foreign.test"),
        Some("/\t/foreign.test"),
        Some("/\n/foreign.test"),
        Some("/\r/foreign.test"),
        Some("/\\foreign.test"),
    ] {
        assert_eq!(sanitize_redirect_path(value), "/");
    }
    for value in ["/", "/dashboard", "/path?query=1#fragment", "/%2F%2Fhost"] {
        assert_eq!(sanitize_redirect_path(Some(value)), value);
    }
}
