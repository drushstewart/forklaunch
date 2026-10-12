use super::*;
pub(super) async fn exercise<S: AuthStore>(
    store: Arc<S>,
    service: &OrganizationService<S>,
    token: &str,
    org: &str,
    now: DateTime<Utc>,
) {
    let rejected = service
        .reject_invitation(
            token,
            false,
            "native-invitation-read",
            now + chrono::Duration::milliseconds(1),
        )
        .await
        .unwrap();
    assert_eq!(rejected["invitation"]["status"], "rejected");
    assert!(rejected["member"].is_null());
    assert_eq!(
        service
            .reject_invitation(token, false, "native-invitation-read", now)
            .await
            .unwrap_err()
            .message,
        "Invitation not found!"
    );
    assert_eq!(
        service
            .cancel_invitation(token, false, "native-invitation-read", now)
            .await
            .unwrap_err()
            .code,
        "YOU_ARE_NOT_ALLOWED_TO_CANCEL_THIS_INVITATION"
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::OrganizationRole,json!({"id":"native-invitation-cancel-grant","organizationId":org,"role":"owner","permission":"{\"invitation\":[\"cancel\"]}"})).await.unwrap();
    tx.commit().await.unwrap();
    let (a, b) = tokio::join!(
        service.cancel_invitation(token, false, "native-invitation-read", now),
        service.cancel_invitation(token, false, "native-invitation-read", now)
    );
    assert_eq!(a.unwrap()["status"], "canceled");
    assert_eq!(b.unwrap()["status"], "canceled");
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::Invitation, "native-invitation-read")
            .await
            .unwrap()
            .unwrap()["status"],
        "canceled"
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
