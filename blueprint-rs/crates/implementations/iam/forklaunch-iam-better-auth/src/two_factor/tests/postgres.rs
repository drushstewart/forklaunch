use super::*;

#[tokio::test]
#[ignore = "requires FORKLAUNCH_TEST_POSTGRES_URL"]
#[allow(clippy::too_many_lines)]
async fn restricted_postgres_totp_lifecycle_encryption_tenant_and_atomicity() {
    crate::restricted_postgres_test::exercise(|context| async move {
        let crate::restricted_postgres_test::Context {store,foreign,owner:reader,tenant}=context;
        lifecycle(store.clone()).await;
        http::email_delivery_lifecycle(store.clone()).await;
        crate::accounts::verification_policy::tests::lifecycle(store.clone()).await;
        crate::accounts::verification_policy::tests_auto::lifecycle(store.clone()).await;
        crate::accounts::verification_policy::tests_async::lifecycle(store.clone()).await;
        otp::lifecycle(store.clone()).await;
        lifecycle_options::lifecycle(store.clone()).await;
        lifetime_options::lifecycle(store.clone()).await;
        lockout_options::lifecycle(store.clone()).await;
        backup_options::lifecycle(store.clone()).await;
        backup_storage::lifecycle(store.clone()).await;
        backup_async::lifecycle(store.clone()).await;
        backup_async::rollback(store.clone()).await;
        backup_view::readonly(store.clone()).await;
        crate::accounts::reset_request::tests::lifecycle(store.clone()).await;
        crate::accounts::revocation_tests::lifecycle(store.clone()).await;
        crate::accounts::reset_observer::tests::lifecycle(store.clone()).await;
        crate::http::email_observer_tests::lifecycle(store.clone()).await;
        crate::http::email_auto_tests::lifecycle(store.clone()).await;
        crate::sessions::policy_tests::exercise_policy(store.clone()).await;
        crate::http::session_policy_tests::lifecycle(store.clone()).await;
        crate::http::credential_policy_tests::lifecycle(store.clone()).await;
        crate::http::clock_tests::lifecycle(store.clone()).await;
        crate::organizations::tests::postgres::exercise(store.clone(), &foreign).await;
        let before_view_audit:i64=sqlx::query_scalar("SELECT count(*) FROM fl_better_auth.audit WHERE tenant_id=$1").bind(&tenant).fetch_one(&reader).await.unwrap();
        let viewer=TwoFactorService::new(store.clone(),vec![42;32],"Read".into()).unwrap();
        assert_eq!(viewer.view_backup_codes_for_user("view-legacy").await.unwrap().len(),4);
        let after_view_audit:i64=sqlx::query_scalar("SELECT count(*) FROM fl_better_auth.audit WHERE tenant_id=$1").bind(&tenant).fetch_one(&reader).await.unwrap();
        assert_eq!(before_view_audit,after_view_audit);
        let foreign_viewer=TwoFactorService::new(Arc::new(foreign.clone()),vec![42;32],"Read".into()).unwrap();
        assert_eq!(foreign_viewer.view_backup_codes_for_user("view-legacy").await.err().unwrap().code,"BACKUP_CODES_NOT_ENABLED");
        otp_storage::lifecycle(store.clone()).await;
        otp_encryption::lifecycle(store.clone()).await;
        otp_async::lifecycle(store.clone()).await;
        otp_async::rollback(store.clone()).await;
        otp_async::late_commit(store.clone()).await;
        let secret=STANDARD.encode([7u8;32]);
        let backup="ABCDE-FGHIJ";
        let mut tx=store.begin(true).await.unwrap();
        tx.insert(Model::TwoFactor,json!({"id":"private-factor","userId":"private-factor-owner","secret":secret,"backupCodes":[backup],"verified":true})).await.unwrap();
        tx.commit().await.unwrap();
        let ciphertext:String=sqlx::query_scalar("SELECT ciphertext FROM fl_better_auth.records WHERE tenant_id=$1 AND model='twoFactor' AND id='private-factor'").bind(&tenant).fetch_one(&reader).await.unwrap();
        assert!(!ciphertext.contains(&secret));assert!(!ciphertext.contains(backup));
        let mut tx=foreign.begin(false).await.unwrap();assert!(tx.find_unique(Model::TwoFactor,"userId","private-factor-owner").await.unwrap().is_none());tx.commit().await.unwrap();
        let mut tx=store.begin(true).await.unwrap();assert!(tx.insert(Model::TwoFactor,json!({"id":"duplicate-factor","userId":"private-factor-owner"})).await.is_err());drop(tx);
        let audits:i64=sqlx::query_scalar("SELECT count(*) FROM fl_better_auth.audit WHERE tenant_id=$1 AND model='twoFactor'").bind(&tenant).fetch_one(&reader).await.unwrap();assert!(audits>0);
    }).await;
}
