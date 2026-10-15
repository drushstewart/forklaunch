#![cfg(feature = "postgres-identity")]
#[path = "support/bound_creation_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use fixture::{Fixture, payload, request};
use forklaunch_billing_base::services::{
    BaseBillingService, BillingKind, BillingStore, Scope,
    postgres::{PaymentLinkCreationClaim, StripeBindingScope},
};
use forklaunch_billing_stripe::{
    bound_creation::{BoundPaymentLinkCreationOutcome, BoundPaymentLinkCreationProfile},
    client::{STRIPE_API_VERSION, StripeBillingClient},
    services::StripeBillingService,
};
use forklaunch_blueprint_core::AuditContext;
use serde_json::{Value, json};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use uuid::Uuid;
#[tokio::test]
#[ignore = "owned PostgreSQL and authenticated loopback provider"]
async fn created_payment_link_read_matrix() {
    let f = Arc::new(Fixture::new().await);
    let run = f.clone();
    let result = tokio::task::LocalSet::new()
        .run_until(async move { tokio::task::spawn_local(async move { matrix(&run).await }).await })
        .await;
    Arc::try_unwrap(f).ok().unwrap().cleanup().await;
    result.unwrap();
}
async fn matrix(f: &Fixture) {
    let BoundPaymentLinkCreationOutcome::Completed { record, .. } = f
        .service
        .create_bound_payment_link(&f.profile, Uuid::new_v4(), request())
        .await
        .unwrap()
    else {
        panic!("created");
    };
    let id = Uuid::parse_str(&record.id).unwrap();
    let provider_id = record.provider_fields.as_ref().unwrap()["id"]
        .as_str()
        .unwrap();
    let got = f
        .service
        .get_bound_created_payment_link(&f.profile, id)
        .await
        .unwrap();
    let mut expected = serde_json::to_value(&record).unwrap();
    expected["providerFields"] = json!({"id":provider_id,"object":"payment_link","livemode":false,"active":false,"metadata":{"presentation":"fresh"}});
    assert_eq!(serde_json::to_value(got).unwrap(), expected);
    assert_eq!(f.posts(), 1);
    denied_profiles(f, id).await;
    unsupported_provenance(f, &serde_json::to_value(&record).unwrap()).await;
    for bad in [
        Value::Null,
        json!([]),
        json!({"id":"plink_other","object":"payment_link","livemode":false}),
        json!({"id":provider_id,"object":"customer","livemode":false}),
        json!({"id":provider_id,"object":"payment_link","livemode":true}),
        json!({"id":provider_id,"object":"payment_link","livemode":null}),
    ] {
        *f.provider.readback.lock().unwrap() = Some(bad);
        assert!(
            f.service
                .get_bound_created_payment_link(&f.profile, id)
                .await
                .is_err()
        );
        assert_eq!(f.posts(), 1);
    }
    *f.provider.readback.lock().unwrap() = None;
    sqlx::raw_sql(&format!("REVOKE INSERT,UPDATE,DELETE ON fl_billing.records,fl_billing.provider_bindings,fl_billing.provider_binding_audit,fl_billing.payment_link_creation_intents,fl_billing.payment_link_creation_audit FROM {}",f.role)).execute(&f.owner).await.unwrap();
    assert!(
        f.service
            .get_bound_created_payment_link(&f.profile, id)
            .await
            .is_ok()
    );
    denied_audit(f, id).await;
    audit_expiry(f, id).await;
    network_race(f, id).await;
    changed_local_proof(f, id).await;
}
async fn changed_local_proof(f: &Fixture, id: Uuid) {
    let binding = f
        .store
        .stripe_binding_by_public_id(&f.scope, BillingKind::PaymentLink, id)
        .await
        .unwrap()
        .unwrap();
    let snapshot = f
        .store
        .payment_link_creation_read_snapshot(&binding, STRIPE_API_VERSION)
        .await
        .unwrap();
    update(
        f,
        "UPDATE fl_billing.provider_bindings SET provider_id=provider_id WHERE tenant_id=$1",
    )
    .await;
    assert!(
        f.store
            .read_payment_link_creation_snapshot(&snapshot)
            .await
            .is_err()
    );
    let snapshot = f
        .store
        .payment_link_creation_read_snapshot(&binding, STRIPE_API_VERSION)
        .await
        .unwrap();
    assert!(
        sqlx::query(
            "UPDATE fl_billing.payment_link_creation_intents SET state=state WHERE tenant_id=$1"
        )
        .bind(f.tenant.id())
        .execute(&f.owner)
        .await
        .is_err()
    );
    assert!(
        f.store
            .read_payment_link_creation_snapshot(&snapshot)
            .await
            .is_ok()
    );
    update(f,"UPDATE fl_billing.records SET data=jsonb_set(data,'{status}','\"CHANGED\"'::jsonb) WHERE tenant_id=$1 AND kind='payment_link'").await;
    let before = f.provider.requests.load(Ordering::SeqCst);
    assert!(
        f.service
            .get_bound_created_payment_link(&f.profile, id)
            .await
            .is_err()
    );
    assert_eq!(before, f.provider.requests.load(Ordering::SeqCst));
    assert_eq!(f.posts(), 1);
}
async fn unsupported_provenance(f: &Fixture, record: &Value) {
    let admin = Uuid::new_v4();
    let mut value = record.clone();
    value["id"] = json!(admin.to_string());
    value["providerFields"]["id"] = json!("plink_admin");
    f.store
        .insert(
            Scope::Durable,
            BillingKind::PaymentLink,
            value,
            Duration::from_secs(600),
        )
        .await
        .unwrap();
    f.store
        .provision_stripe_binding(
            &f.scope,
            BillingKind::PaymentLink,
            admin,
            &admin.to_string(),
            "plink_admin",
        )
        .await
        .unwrap();
    let before = f.provider.requests.load(Ordering::SeqCst);
    assert!(
        f.service
            .get_bound_created_payment_link(&f.profile, admin)
            .await
            .is_err()
    );
    assert_eq!(before, f.provider.requests.load(Ordering::SeqCst));
    assert!(
        f.service
            .get_bound_payment_link(&f.scope, admin)
            .await
            .is_ok()
    );
    // These are trusted-store adapter fixtures, not provider creation evidence.
    // Neither a sealed managed envelope nor excessive legacy evidence qualifies.
    let mut nested = json!(true);
    for _ in 0..34 {
        nested = json!([nested]);
    }
    for (index, extra) in [
        json!({"managed":{"kind":"payment_link_creation_v1"}}),
        json!({"extra":nested}),
        json!({"extra":"x".repeat(55_000)}),
    ]
    .into_iter()
    .enumerate()
    {
        let mut payload = payload();
        for (k, v) in extra.as_object().unwrap() {
            payload[k] = v.clone();
        }
        let PaymentLinkCreationClaim::Reserved(ticket) = f
            .store
            .reserve_payment_link_creation(&f.scope, Uuid::new_v4(), STRIPE_API_VERSION, &payload)
            .await
            .unwrap()
        else {
            panic!("reserved");
        };
        let id = ticket.public_id();
        let mut record = record.clone();
        record["id"] = json!(id.to_string());
        record["providerFields"]["id"] = json!(format!("plink_unsupported_{index}"));
        f.store
            .complete_payment_link_creation(ticket, record, || Ok(()))
            .await
            .unwrap();
        let before = f.provider.requests.load(Ordering::SeqCst);
        assert!(
            f.service
                .get_bound_created_payment_link(&f.profile, id)
                .await
                .is_err()
        );
        assert_eq!(before, f.provider.requests.load(Ordering::SeqCst));
    }
}
async fn denied_profiles(f: &Fixture, id: Uuid) {
    let before = f.provider.requests.load(Ordering::SeqCst);
    for profile in [
        BoundPaymentLinkCreationProfile::trusted(f.scope.clone(), vec!["price_other".into()])
            .unwrap(),
        BoundPaymentLinkCreationProfile::trusted(
            StripeBindingScope::administrative("other".into(), "acct_owned".into(), false).unwrap(),
            vec!["price_owned".into()],
        )
        .unwrap(),
    ] {
        assert!(
            f.service
                .get_bound_created_payment_link(&profile, id)
                .await
                .is_err()
        );
    }
    let other = StripeBillingService::new(
        BaseBillingService::new(Arc::new(
            f.store
                .with_audit_context(AuditContext::actor("other", "read"))
                .unwrap(),
        )),
        f.client.clone(),
    );
    assert!(
        other
            .get_bound_created_payment_link(&f.profile, id)
            .await
            .is_err()
    );
    assert!(
        f.service
            .get_bound_created_payment_link(&f.profile, Uuid::new_v4())
            .await
            .is_err()
    );
    let foreign_store = forklaunch_billing_base::services::postgres::PostgresBillingStore::new(
        f.pool.clone(),
        forklaunch_core::persistence::TenantContext::new(format!("{}-foreign", f.role)).unwrap(),
        Arc::new(
            forklaunch_core::persistence::encryption::FieldEncryptor::new(
                "owned-bound-create-encryption-key",
            )
            .unwrap(),
        ),
        AuditContext::actor("service:bound-create", "read"),
    )
    .await
    .unwrap();
    let foreign = StripeBillingService::new(
        BaseBillingService::new(Arc::new(foreign_store)),
        f.client.clone(),
    );
    assert!(
        foreign
            .get_bound_created_payment_link(&f.profile, id)
            .await
            .is_err()
    );
    let managed = StripeBillingService::new(
        BaseBillingService::new(f.store.clone()),
        StripeBillingClient::managed(
            "http://127.0.0.1:9",
            Uuid::new_v4().to_string(),
            "key".into(),
        )
        .unwrap(),
    );
    assert!(
        managed
            .get_bound_created_payment_link(&f.profile, id)
            .await
            .is_err()
    );
    assert_eq!(before, f.provider.requests.load(Ordering::SeqCst));
}
async fn update(f: &Fixture, sql: &str) {
    sqlx::query(sql)
        .bind(f.tenant.id())
        .execute(&f.owner)
        .await
        .unwrap();
}
async fn denied_audit(f: &Fixture, id: Uuid) {
    sqlx::raw_sql(&format!(
        "REVOKE INSERT ON fl_billing.sensitive_read_audit FROM {}",
        f.role
    ))
    .execute(&f.owner)
    .await
    .unwrap();
    let before = f.provider.requests.load(Ordering::SeqCst);
    assert!(
        f.service
            .get_bound_created_payment_link(&f.profile, id)
            .await
            .is_err()
    );
    assert_eq!(before, f.provider.requests.load(Ordering::SeqCst));
    sqlx::raw_sql(&format!(
        "GRANT INSERT ON fl_billing.sensitive_read_audit TO {}",
        f.role
    ))
    .execute(&f.owner)
    .await
    .unwrap();
}
async fn network_race(f: &Fixture, id: Uuid) {
    f.provider.pause_read.store(true, Ordering::SeqCst);
    let service = f.service.clone();
    let scope = f.scope.clone();
    let read = tokio::spawn(async move {
        let profile =
            BoundPaymentLinkCreationProfile::trusted(scope, vec!["price_owned".into()]).unwrap();
        service.get_bound_created_payment_link(&profile, id).await
    });
    f.provider.entered.notified().await;
    update(
        f,
        "UPDATE fl_billing.provider_bindings SET provider_id=provider_id WHERE tenant_id=$1",
    )
    .await;
    f.provider.resume.notify_one();
    assert!(read.await.unwrap().is_err());
    f.provider.pause_read.store(false, Ordering::SeqCst);
}
async fn audit_expiry(f: &Fixture, id: Uuid) {
    f.provider.pause_read.store(true, Ordering::SeqCst);
    let service = f.service.clone();
    let scope = f.scope.clone();
    let read = tokio::spawn(async move {
        let profile =
            BoundPaymentLinkCreationProfile::trusted(scope, vec!["price_owned".into()]).unwrap();
        service.get_bound_created_payment_link(&profile, id).await
    });
    f.provider.entered.notified().await;
    let mut tx = f.owner.begin().await.unwrap();
    sqlx::query("LOCK TABLE fl_billing.sensitive_read_audit IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await
        .unwrap();
    f.provider.resume.notify_one();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename=$1 AND wait_event_type='Lock' AND query LIKE '%sensitive_read_audit%')").bind(&f.role).fetch_one(&f.owner).await.unwrap();
        if waiting {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "read must reach audit wait"
        );
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(Duration::from_millis(5100)).await;
    tx.commit().await.unwrap();
    assert!(read.await.unwrap().is_err());
    f.provider.pause_read.store(false, Ordering::SeqCst);
}
