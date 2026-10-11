use forklaunch_billing_base::services::{
    BaseBillingService, BillingKind, BillingStore, Scope, memory::MemoryBillingStore,
};
use forklaunch_billing_interface::services::{
    BillingPortalService, CheckoutSessionService, PaymentLinkService, PlanService,
    SubscriptionService, UpdatePlanDto,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
fn dto<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}
fn plan(id: &str, external: &str) -> Value {
    json!({"id":id,"name":"Pro","price":1999,"cadence":"month","currency":"USD","externalId":external,"billingProvider":"stripe","active":true,"providerFields":{"email":"private-billing@example.test"}})
}
fn checkout(id: &str) -> Value {
    json!({"id":id,"customerId":"customer","paymentMethods":["card"],"currency":"USD","status":"PENDING","expiresAt":"2020-01-01T00:00:00Z","uri":"https://checkout.example/private-session-token","providerFields":{"email":"private-billing@example.test"}})
}
fn value<T: Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap()
}
#[allow(clippy::too_many_lines, clippy::float_cmp)] // Exact integer-valued money round trips.
async fn exercise<S: BillingStore + 'static>(store: Arc<S>) {
    let service = BaseBillingService::new(Arc::clone(&store));
    service
        .create_plan(dto(plan("plan", "price")))
        .await
        .unwrap();
    let first = BaseBillingService::new(Arc::clone(&store));
    let second = BaseBillingService::new(Arc::clone(&store));
    let (a, b) = tokio::join!(
        first.update_plan(dto(
            json!({"id":"plan","description":"concurrent-description"})
        )),
        second.update_plan(dto(json!({"id":"plan","features":["concurrent-feature"]})))
    );
    a.unwrap();
    b.unwrap();
    let current = service.get_plan("plan").await.unwrap();
    assert_eq!(
        current.description.as_deref(),
        Some("concurrent-description")
    );
    assert_eq!(current.features.unwrap(), vec!["concurrent-feature"]);
    let mut invalid: UpdatePlanDto = dto(json!({"id":"plan"}));
    invalid.price = Some(f64::NAN);
    assert!(service.update_plan(invalid).await.is_err());
    assert_eq!(service.get_plan("plan").await.unwrap().price, 1999.0);
    let mut tasks = Vec::new();
    for index in 0..8 {
        let store = Arc::clone(&store);
        tasks.push(tokio::spawn(async move {
            BaseBillingService::new(store)
                .create_plan(dto(plan(&format!("race-{index}"), "unique-provider-price")))
                .await
                .is_ok()
        }));
    }
    let mut winners = 0;
    for task in tasks {
        winners += usize::from(task.await.unwrap());
    }
    assert_eq!(winners, 1);
    let subscription = json!({"id":"subscription","partyId":"party","partyType":"USER","productId":"plan","active":true,"externalId":"provider-subscription","startDate":"2026-01-01T00:00:00Z","status":"TRIALING","billingProvider":"stripe","providerFields":{"email":"private-billing@example.test"}});
    service
        .create_subscription(dto(subscription))
        .await
        .unwrap();
    assert_eq!(
        service.get_user_subscription("party").await.unwrap().id,
        "subscription"
    );
    assert!(
        service
            .get_organization_subscription("party")
            .await
            .is_err()
    );
    service.cancel_subscription("subscription").await.unwrap();
    assert!(service.get_user_subscription("party").await.is_err());
    service.resume_subscription("subscription").await.unwrap();
    assert_eq!(
        service
            .find_subscription_id_by_external_id("provider-subscription")
            .await
            .unwrap()
            .as_deref(),
        Some("subscription")
    );
    service
        .create_checkout_session(dto(checkout("cached")))
        .await
        .unwrap();
    assert!(
        service.get_checkout_session("cached").await.is_ok(),
        "Provider expiry must not replace configured cache TTL"
    );
    assert!(
        store
            .get(Scope::Durable, BillingKind::CheckoutSession, "cached")
            .await
            .unwrap()
            .is_none()
    );
    let mut replacement = checkout("cached");
    replacement["customerId"] = json!("replacement-customer");
    service
        .create_checkout_session(dto(replacement))
        .await
        .unwrap();
    assert_eq!(
        service
            .get_checkout_session("cached")
            .await
            .unwrap()
            .customer_id,
        "replacement-customer"
    );
    service.expire_checkout_session("cached").await.unwrap();
    assert!(service.get_checkout_session("cached").await.is_err());
    assert!(service.expire_checkout_session("cached").await.is_err());
    let backed = BaseBillingService::new(Arc::clone(&store)).with_database_backup(true);
    backed
        .create_checkout_session(dto(checkout("backed")))
        .await
        .unwrap();
    backed.handle_checkout_success("backed").await.unwrap();
    assert!(backed.get_checkout_session("backed").await.is_err());
    assert_eq!(
        store
            .get(Scope::Durable, BillingKind::CheckoutSession, "backed")
            .await
            .unwrap()
            .unwrap()["status"],
        "SUCCESS"
    );
    let link = json!({"id":"link","amount":100,"paymentMethods":["card"],"currency":"USD","status":"PENDING"});
    backed.create_payment_link(dto(link)).await.unwrap();
    backed
        .update_payment_link(dto(json!({"id":"link","amount":0})))
        .await
        .unwrap();
    assert_eq!(backed.get_payment_link("link").await.unwrap().amount, 0.0);
    assert_eq!(
        backed
            .list_payment_links(Some(&["link".to_owned(), "link".to_owned()]))
            .await
            .unwrap()
            .len(),
        2
    );
    backed.handle_payment_success("link").await.unwrap();
    assert!(
        store
            .get(Scope::Durable, BillingKind::PaymentLink, "link")
            .await
            .unwrap()
            .is_none()
    );
    assert!(backed.handle_payment_success("link").await.is_err());
    backed.create_billing_portal_session(dto(json!({"id":"portal","customerId":"customer","expiresAt":"2020-01-01T00:00:00Z","uri":"https://portal.example/private-session-token","providerFields":null}))).await.unwrap();
    let updated = backed
        .update_billing_portal_session(dto(
            json!({"id":"portal","uri":"https://portal.example/updated-session-token"}),
        ))
        .await
        .unwrap();
    assert_eq!(updated.customer_id, "customer");
    assert_eq!(value(updated)["providerFields"], Value::Null);
    backed
        .expire_billing_portal_session("portal")
        .await
        .unwrap();
    assert!(
        store
            .get(Scope::Durable, BillingKind::BillingPortal, "portal")
            .await
            .unwrap()
            .is_some()
    );
    let short = BaseBillingService::new(Arc::clone(&store))
        .with_database_backup(true)
        .with_cache_ttl(Duration::from_millis(200))
        .unwrap();
    short
        .create_checkout_session(dto(checkout("ttl")))
        .await
        .unwrap();
    assert!(short.get_checkout_session("ttl").await.is_ok());
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(short.get_checkout_session("ttl").await.is_err());
    assert!(
        store
            .get(Scope::Durable, BillingKind::CheckoutSession, "ttl")
            .await
            .unwrap()
            .is_some()
    );
}
#[tokio::test]
async fn memory_billing_lifecycle_cache_validation_and_concurrency() {
    exercise(Arc::new(MemoryBillingStore::new())).await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "requires an isolated FORKLAUNCH_TEST_POSTGRES_URL owner database"]
#[allow(clippy::too_many_lines)]
async fn postgres_billing_rls_encryption_audit_concurrency_and_reopening() {
    use forklaunch_billing_base::services::postgres::PostgresBillingStore;
    use forklaunch_blueprint_core::AuditContext;
    use forklaunch_core::persistence::{TenantContext, encryption::FieldEncryptor};
    use sqlx::postgres::PgPoolOptions;
    let owner = PgPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("FORKLAUNCH_TEST_POSTGRES_URL").unwrap())
        .await
        .unwrap();
    PostgresBillingStore::migrate(&owner).await.unwrap();
    let role = format!("fl_bill_{}", uuid::Uuid::new_v4().simple());
    let password = "billing-fixture-only";
    sqlx::raw_sql(&format!("CREATE ROLE {role} LOGIN NOSUPERUSER NOBYPASSRLS PASSWORD '{password}'; GRANT USAGE ON SCHEMA fl_billing TO {role}; GRANT SELECT,INSERT,UPDATE,DELETE ON fl_billing.records TO {role}; GRANT INSERT ON fl_billing.sensitive_read_audit TO {role}")).execute(&owner).await.unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect_with(
            (*owner.connect_options())
                .clone()
                .username(&role)
                .password(password),
        )
        .await
        .unwrap();
    let tenant = TenantContext::new(format!("{role}-a")).unwrap();
    let other_tenant = TenantContext::new(format!("{role}-b")).unwrap();
    let keys = Arc::new(FieldEncryptor::new("billing-old-fixture-secret").unwrap());
    let audit = AuditContext::actor("service:billing-fixture", "billing-fixture-correlation");
    assert!(
        PostgresBillingStore::new(
            owner.clone(),
            tenant.clone(),
            Arc::clone(&keys),
            audit.clone()
        )
        .await
        .is_err()
    );
    let store = Arc::new(
        PostgresBillingStore::new(
            pool.clone(),
            tenant.clone(),
            Arc::clone(&keys),
            audit.clone(),
        )
        .await
        .unwrap(),
    );
    exercise(Arc::clone(&store)).await;
    let other = Arc::new(
        PostgresBillingStore::new(
            pool.clone(),
            other_tenant.clone(),
            Arc::clone(&keys),
            audit.clone(),
        )
        .await
        .unwrap(),
    );
    let other_service = BaseBillingService::new(Arc::clone(&other));
    assert!(other_service.get_plan("plan").await.is_err());
    assert!(other_service.get_user_subscription("party").await.is_err());
    assert!(
        other_service
            .find_subscription_id_by_external_id("provider-subscription")
            .await
            .unwrap()
            .is_none()
    );
    other_service
        .create_plan(dto(plan("plan", "price")))
        .await
        .unwrap();
    let (unscoped,): (i64,) = sqlx::query_as("SELECT count(*) FROM fl_billing.records")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(unscoped, 0);
    let data: Vec<(Value,)> =
        sqlx::query_as("SELECT data FROM fl_billing.records WHERE tenant_id=$1")
            .bind(tenant.id())
            .fetch_all(&owner)
            .await
            .unwrap();
    for (row,) in data {
        let raw = row.to_string();
        assert!(!raw.contains("private-billing@example.test"));
        assert!(!raw.contains("private-session-token"));
        assert!(!raw.contains("updated-session-token"));
    }
    let (count_before,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM fl_billing.sensitive_read_audit WHERE tenant_id=$1")
            .bind(tenant.id())
            .fetch_one(&owner)
            .await
            .unwrap();
    assert!(count_before > 0);
    let service = BaseBillingService::new(Arc::clone(&store));
    assert_eq!(
        service
            .find_subscription_id_by_external_id("provider-subscription")
            .await
            .unwrap()
            .as_deref(),
        Some("subscription")
    );
    let (count_after,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM fl_billing.sensitive_read_audit WHERE tenant_id=$1")
            .bind(tenant.id())
            .fetch_one(&owner)
            .await
            .unwrap();
    assert_eq!(
        count_before, count_after,
        "Provider-ID lookup must not decrypt provider data"
    );
    sqlx::raw_sql(&format!(
        "REVOKE INSERT ON fl_billing.sensitive_read_audit FROM {role}"
    ))
    .execute(&owner)
    .await
    .unwrap();
    assert!(service.get_plan("plan").await.is_err());
    assert!(
        service
            .update_plan(dto(json!({"id":"plan","description":"must-rollback"})))
            .await
            .is_err()
    );
    sqlx::raw_sql(&format!(
        "GRANT INSERT ON fl_billing.sensitive_read_audit TO {role}"
    ))
    .execute(&owner)
    .await
    .unwrap();
    assert_eq!(
        service
            .get_plan("plan")
            .await
            .unwrap()
            .description
            .as_deref(),
        Some("concurrent-description")
    );
    let rotated = Arc::new(
        FieldEncryptor::new("billing-new-fixture-secret")
            .unwrap()
            .with_previous_keys(&["billing-old-fixture-secret".to_owned()])
            .unwrap(),
    );
    let reopened = BaseBillingService::new(Arc::new(
        PostgresBillingStore::new(pool.clone(), tenant.clone(), rotated, audit.clone())
            .await
            .unwrap(),
    ));
    assert_eq!(
        reopened
            .get_plan("plan")
            .await
            .unwrap()
            .provider_fields
            .unwrap()["email"],
        "private-billing@example.test"
    );
    reopened
        .update_plan(dto(json!({"id":"plan","description":"rotated"})))
        .await
        .unwrap();
    let new_only = BaseBillingService::new(Arc::new(
        PostgresBillingStore::new(
            pool.clone(),
            tenant.clone(),
            Arc::new(FieldEncryptor::new("billing-new-fixture-secret").unwrap()),
            audit.clone(),
        )
        .await
        .unwrap(),
    ));
    assert_eq!(
        new_only
            .get_plan("plan")
            .await
            .unwrap()
            .description
            .as_deref(),
        Some("rotated")
    );
    // Authenticated envelopes bind scope, kind, record and field, preventing
    // an owner/storage bug from substituting another record's encrypted payload.
    let (original,): (Value,) = sqlx::query_as("SELECT data->'providerFields' FROM fl_billing.records WHERE tenant_id=$1 AND scope='durable' AND kind='plan' AND id='plan'")
        .bind(tenant.id()).fetch_one(&owner).await.unwrap();
    let (other_cipher,): (Value,) = sqlx::query_as("SELECT data->'providerFields' FROM fl_billing.records WHERE tenant_id=$1 AND scope='durable' AND kind='subscription' AND id='subscription'")
        .bind(tenant.id()).fetch_one(&owner).await.unwrap();
    sqlx::query("UPDATE fl_billing.records SET data=jsonb_set(data,'{providerFields}',$2) WHERE tenant_id=$1 AND scope='durable' AND kind='plan' AND id='plan'")
        .bind(tenant.id()).bind(other_cipher).execute(&owner).await.unwrap();
    assert!(reopened.get_plan("plan").await.is_err());
    sqlx::query("UPDATE fl_billing.records SET data=jsonb_set(data,'{providerFields}',$2) WHERE tenant_id=$1 AND scope='durable' AND kind='plan' AND id='plan'")
        .bind(tenant.id()).bind(original).execute(&owner).await.unwrap();
    assert!(new_only.get_plan("plan").await.is_ok());
    assert!(store.purge_expired_cache().await.unwrap() >= 1);
    let actors: Vec<(Option<String>, String)> = sqlx::query_as(
        "SELECT actor_id,correlation_id FROM fl_billing.sensitive_read_audit WHERE tenant_id=$1",
    )
    .bind(tenant.id())
    .fetch_all(&owner)
    .await
    .unwrap();
    assert!(actors.iter().all(|(actor, correlation)| actor.as_deref()
        == Some("service:billing-fixture")
        && correlation == "billing-fixture-correlation"));
    drop(new_only);
    drop(reopened);
    drop(service);
    drop(store);
    drop(other_service);
    drop(other);
    pool.close().await;
    for table in ["records", "sensitive_read_audit"] {
        sqlx::query(&format!(
            "DELETE FROM fl_billing.{table} WHERE tenant_id=ANY($1)"
        ))
        .bind(vec![tenant.id(), other_tenant.id()])
        .execute(&owner)
        .await
        .unwrap();
    }
    sqlx::raw_sql(&format!("DROP OWNED BY {role}; DROP ROLE {role}"))
        .execute(&owner)
        .await
        .unwrap();
    owner.close().await;
}
