//! Backend organization regressions shared by the full and focused PG suites.
use super::*;

pub(crate) async fn exercise<S: AuthStore + 'static>(store: Arc<S>, foreign: &S) {
    exercise_organizations(store.clone()).await;
    #[cfg(feature = "http")]
    crate::http::clock_tests::organization::lifecycle(store.clone()).await;
    // An exact actor ID in another tenant must not consume organization quota.
    let mut tx = foreign.begin(true).await.unwrap();
    tx.insert(Model::Member,json!({"id":"foreign-policy-membership","userId":"org-policy-create-2-owner","organizationId":"foreign-policy-org","role":"owner"})).await.unwrap();
    tx.commit().await.unwrap();
    policy::exercise(store.clone()).await;
    invitation_policy::exercise(store.clone()).await;
    invitation_cancel::exercise(store.clone()).await;
    invitation_delivery::exercise(store.clone()).await;
    team_policy::exercise(store.clone()).await;
    controls::exercise(store.clone()).await;
    creator_role::exercise(store.clone()).await;
    access_control::exercise(store.clone()).await;
    authorizers::exercise(store.clone()).await;
    creation_policy::exercise(store).await;
}

#[tokio::test]
#[ignore = "requires FORKLAUNCH_TEST_POSTGRES_URL"]
async fn restricted_postgres_organization_lifecycle_policies_and_tenant_boundaries() {
    crate::restricted_postgres_test::exercise(|context| async move {
        exercise(context.store, &context.foreign).await;
    })
    .await;
}
