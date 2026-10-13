use forklaunch_blueprint_core::AuditContext;
use forklaunch_ecommerce_base::{
    BaseEcommerceService,
    reconciliation::{PaymentReconciliationStore, VerifiedCapture},
    store::InMemoryEcommerceStore,
};
use forklaunch_ecommerce_interface::{
    CreatePaymentDto, FailPaymentDto, PaymentService, PaymentStatus,
};
use std::sync::Arc;
fn evidence() -> VerifiedCapture {
    VerifiedCapture {
        provider: "paypal".into(),
        provider_ref: "order-owned".into(),
        capture_id: "capture-owned".into(),
        amount_cents: 105,
        currency: "USD".into(),
    }
}
fn actor() -> AuditContext {
    AuditContext::actor("operator", "reconcile-owned")
}
async fn exercise<S: PaymentReconciliationStore>(store: Arc<S>) {
    let service = BaseEcommerceService::new(store);
    let original = service
        .create_payment_with_reference(
            CreatePaymentDto {
                id: Some("payment".into()),
                order_id: "order".into(),
                amount_cents: 105,
                currency: "usd".into(),
            },
            Some("order-owned".into()),
        )
        .await
        .unwrap();
    assert!(
        service
            .record_verified_capture(
                original.clone(),
                evidence(),
                AuditContext::system("anonymous")
            )
            .await
            .is_err()
    );
    let mut wrong = evidence();
    wrong.amount_cents = 106;
    assert!(
        service
            .record_verified_capture(original.clone(), wrong, actor())
            .await
            .is_err()
    );
    service
        .fail_payment(FailPaymentDto {
            provider_ref: "order-owned".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        service
            .record_verified_capture(original, evidence(), actor())
            .await
            .unwrap_err()
            .code,
        "ecommerce_reconciliation_conflict"
    );
    let failed = service.get_payment("payment").await.unwrap();
    let repaired = service
        .record_verified_capture(failed, evidence(), actor())
        .await
        .unwrap();
    assert_eq!(repaired.status, PaymentStatus::Succeeded);
    // A delayed failure from the old capture attempt cannot reset verified completion.
    service
        .release_confirmation_claim("order-owned")
        .await
        .unwrap();
    assert_eq!(
        service.get_payment("payment").await.unwrap().status,
        PaymentStatus::Succeeded
    );
    assert!(!service.claim_for_confirmation("order-owned").await.unwrap());
}
#[tokio::test]
async fn memory_repairs_are_audited_and_reject_stale_snapshots() {
    let store = Arc::new(InMemoryEcommerceStore::default());
    exercise(Arc::clone(&store)).await;
    let audits = store.payment_reconciliation_audits().unwrap();
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].previous_status, PaymentStatus::Failed);
    assert_eq!(audits[0].actor, actor());
    assert_eq!(audits[0].evidence, evidence());
}
#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "requires isolated FORKLAUNCH_TEST_POSTGRES_URL owner database"]
#[allow(clippy::too_many_lines)]
async fn postgres_repair_audit_rollback_tenants_and_restart() {
    use forklaunch_core::persistence::{TenantContext, encryption::FieldEncryptor};
    use forklaunch_ecommerce_base::postgres::PostgresEcommerceStore;
    use sqlx::postgres::PgPoolOptions;
    let owner = PgPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("FORKLAUNCH_TEST_POSTGRES_URL").unwrap())
        .await
        .unwrap();
    PostgresEcommerceStore::migrate(&owner).await.unwrap();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let role = format!("ecom_reconcile_{suffix}");
    sqlx::raw_sql(&format!("CREATE ROLE {role} LOGIN NOSUPERUSER NOBYPASSRLS PASSWORD 'fixture-only'; GRANT USAGE ON SCHEMA fl_ecommerce TO {role}; GRANT SELECT,INSERT,UPDATE,DELETE ON fl_ecommerce.records TO {role}; GRANT SELECT,INSERT ON fl_ecommerce.payment_reconciliation_audit TO {role};"))
        .execute(&owner).await.unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(
            (*owner.connect_options())
                .clone()
                .username(&role)
                .password("fixture-only"),
        )
        .await
        .unwrap();
    let tenant = TenantContext::new(format!("reconcile-{suffix}")).unwrap();
    let other_tenant = TenantContext::new(format!("other-{suffix}")).unwrap();
    let keys = Arc::new(FieldEncryptor::new("reconciliation-fixture-only").unwrap());
    let store = Arc::new(
        PostgresEcommerceStore::new(pool.clone(), tenant.clone(), Arc::clone(&keys), actor())
            .await
            .unwrap(),
    );
    exercise(Arc::clone(&store)).await;
    let service = BaseEcommerceService::new(Arc::clone(&store));
    let expected = service.get_payment("payment").await.unwrap();
    let other = PostgresEcommerceStore::new(
        pool.clone(),
        other_tenant.clone(),
        Arc::clone(&keys),
        actor(),
    )
    .await
    .unwrap();
    assert!(
        other
            .record_verified_capture(expected, evidence(), actor())
            .await
            .is_err()
    );
    // Exercise audit failure on a transition that would actually change state.
    let pending = service
        .create_payment_with_reference(
            CreatePaymentDto {
                id: Some("rollback".into()),
                order_id: "order".into(),
                amount_cents: 105,
                currency: "usd".into(),
            },
            Some("rollback-ref".into()),
        )
        .await
        .unwrap();
    let mut proof = evidence();
    proof.provider_ref = "rollback-ref".into();
    sqlx::raw_sql(&format!(
        "REVOKE INSERT ON fl_ecommerce.payment_reconciliation_audit FROM {role}"
    ))
    .execute(&owner)
    .await
    .unwrap();
    assert!(
        service
            .record_verified_capture(pending.clone(), proof, actor())
            .await
            .is_err()
    );
    assert_eq!(service.get_payment("rollback").await.unwrap(), pending);
    sqlx::raw_sql(&format!(
        "GRANT INSERT ON fl_ecommerce.payment_reconciliation_audit TO {role}"
    ))
    .execute(&owner)
    .await
    .unwrap();
    let hidden: i64 =
        sqlx::query_scalar("SELECT count(*) FROM fl_ecommerce.payment_reconciliation_audit")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(hidden, 0);
    let rows:Vec<(String,String,String,String)>=sqlx::query_as("SELECT payment_id,capture_id,actor_id,correlation_id FROM fl_ecommerce.payment_reconciliation_audit WHERE tenant_id=$1")
        .bind(tenant.id()).fetch_all(&owner).await.unwrap();
    assert_eq!(
        rows,
        vec![(
            "payment".into(),
            "capture-owned".into(),
            "operator".into(),
            "reconcile-owned".into()
        )]
    );
    let reopened = BaseEcommerceService::new(Arc::new(
        PostgresEcommerceStore::new(pool.clone(), tenant.clone(), keys, actor())
            .await
            .unwrap(),
    ));
    reopened
        .release_confirmation_claim("order-owned")
        .await
        .unwrap();
    assert_eq!(
        reopened.get_payment("payment").await.unwrap().status,
        PaymentStatus::Succeeded
    );
    drop(reopened);
    drop(other);
    drop(service);
    drop(store);
    pool.close().await;
    for table in ["records", "payment_reconciliation_audit"] {
        sqlx::query(&format!(
            "DELETE FROM fl_ecommerce.{table} WHERE tenant_id=$1"
        ))
        .bind(tenant.id())
        .execute(&owner)
        .await
        .unwrap();
    }
    sqlx::raw_sql(&format!("DROP OWNED BY {role};DROP ROLE {role}"))
        .execute(&owner)
        .await
        .unwrap();
    owner.close().await;
}
