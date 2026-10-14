#![cfg(feature = "postgres")]
#[path = "../../forklaunch-billing-stripe/tests/support/commit_ack_proxy.rs"]
mod commit_ack_proxy;
use forklaunch_billing_base::services::{
    BaseBillingService,
    postgres::{
        PlanCreationClaim, PostgresBillingStore, StripeBindingScope, SubscriptionCreationClaim,
        SubscriptionDependencySnapshot, TrustedCustomerParty,
    },
};
use forklaunch_billing_interface::customers::{CreateCustomerDto, CustomerService};
use forklaunch_blueprint_core::{AuditContext, BlueprintError};
use forklaunch_core::persistence::{TenantContext, encryption::FieldEncryptor};
use serde_json::json;
use sqlx::{
    PgPool,
    postgres::{PgPoolOptions, PgSslMode},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use uuid::Uuid;

struct Fixture {
    admin: PgPool,
    owner: PgPool,
    pool: PgPool,
    store: Arc<PostgresBillingStore>,
    database: String,
    owner_role: String,
    runtime_role: String,
    scope: StripeBindingScope,
    customer: Uuid,
    plan: Uuid,
    plan_operation: Uuid,
}
impl Fixture {
    async fn store(pool: PgPool, actor: &str) -> PostgresBillingStore {
        PostgresBillingStore::new(
            pool,
            TenantContext::new("subscription-a").unwrap(),
            Arc::new(FieldEncryptor::new("owned-subscription-foundation-key").unwrap()),
            AuditContext::actor(actor, "subscription-foundation"),
        )
        .await
        .unwrap()
    }
    #[allow(clippy::too_many_lines)]
    async fn new() -> Self {
        let admin = PgPoolOptions::new()
            .max_connections(3)
            .connect(&std::env::var("FORKLAUNCH_TEST_POSTGRES_URL").unwrap())
            .await
            .unwrap();
        let suffix = Uuid::new_v4().simple().to_string();
        let database = format!("fl_sub_claim_{suffix}");
        let owner_role = format!("fl_sub_owner_{suffix}");
        let runtime_role = format!("fl_sub_runtime_{suffix}");
        sqlx::raw_sql(&format!("CREATE ROLE {owner_role} LOGIN NOINHERIT NOSUPERUSER NOBYPASSRLS PASSWORD 'owned-fixture'; CREATE ROLE {runtime_role} LOGIN NOINHERIT NOSUPERUSER NOBYPASSRLS PASSWORD 'owned-fixture'")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE DATABASE {database} OWNER {owner_role}"))
            .execute(&admin)
            .await
            .unwrap();
        let options = (*admin.connect_options())
            .clone()
            .database(&database)
            .password("owned-fixture");
        let owner = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options.clone().username(&owner_role))
            .await
            .unwrap();
        // First install the exact existing migrations, then populate encrypted
        // records/bindings before applying migration 10 under a non-bypass owner.
        sqlx::query("CREATE SCHEMA fl_billing")
            .execute(&owner)
            .await
            .unwrap();
        let mut migrations = sqlx::migrate!("./migrations");
        migrations.migrations = std::borrow::Cow::Owned(
            migrations
                .migrations
                .iter()
                .filter(|m| m.version < 10)
                .cloned()
                .collect(),
        );
        migrations.set_locking(false);
        let mut initial = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL search_path=fl_billing,pg_catalog")
            .execute(&mut *initial)
            .await
            .unwrap();
        migrations.run_direct(&mut *initial).await.unwrap();
        initial.commit().await.unwrap();
        sqlx::raw_sql(&format!("GRANT USAGE ON SCHEMA fl_billing TO {runtime_role}; GRANT ALL ON ALL TABLES IN SCHEMA fl_billing TO {runtime_role}")).execute(&owner).await.unwrap();
        let pool = PgPoolOptions::new()
            .max_connections(6)
            .connect_with(options.username(&runtime_role))
            .await
            .unwrap();
        let store = Arc::new(Self::store(pool.clone(), "service:subscription").await);
        let scope =
            StripeBindingScope::administrative("owned".into(), "acct_owned".into(), false).unwrap();
        let customer = Uuid::parse_str(
            &BaseBillingService::new(store.clone())
                .create_customer(CreateCustomerDto {
                    id: None,
                    email: "private@example.test".into(),
                    name: None,
                })
                .await
                .unwrap()
                .id,
        )
        .unwrap();
        let association = TrustedCustomerParty::administrative(
            customer,
            "user".into(),
            "external-party-assertion".into(),
        )
        .unwrap();
        let snapshot = store
            .snapshot_customer_registration(&scope, &association, "cus_owned", "fixture-version")
            .await
            .unwrap();
        store
            .complete_customer_registration(snapshot, || Ok(()))
            .await
            .unwrap();
        let plan_operation = Uuid::new_v4();
        let request = json!({"name":"Pro","active":true,"currency":"USD","cadence":"month","unitAmount":1200,"productId":"custom-product"});
        let PlanCreationClaim::Reserved(ticket) = store
            .reserve_plan_creation(
                &scope,
                plan_operation,
                "fixture-version",
                &json!({"request":request}),
            )
            .await
            .unwrap()
        else {
            panic!("new fixture intent")
        };
        let plan = ticket.public_id();
        let product = json!({"id":"custom-product","object":"product","livemode":false});
        let record = json!({"id":plan.to_string(),"name":"Pro","price":1200,"active":true,"currency":"USD","cadence":"month","externalId":"custom-plan","billingProvider":"stripe","providerFields":product,"createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z"});
        store
            .complete_plan_creation(
                ticket,
                record,
                json!({"id":"custom-plan","object":"plan","livemode":false,"product":product}),
                || Ok(()),
            )
            .await
            .unwrap();
        let before = record_bodies(&pool).await;
        PostgresBillingStore::migrate(&owner).await.unwrap();
        assert_eq!(
            before,
            record_bodies(&pool).await,
            "migration changed legacy ciphertext"
        );
        let guards: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM fl_billing.subscription_dependency_guards WHERE claims=0",
        )
        .fetch_one(&owner)
        .await
        .unwrap();
        assert_eq!(
            guards, 4,
            "FORCE RLS migration owner failed to backfill other tenant"
        );
        sqlx::raw_sql(&format!(
            "GRANT ALL ON ALL TABLES IN SCHEMA fl_billing TO {runtime_role}"
        ))
        .execute(&owner)
        .await
        .unwrap();
        Self {
            admin,
            owner,
            pool,
            store,
            database,
            owner_role,
            runtime_role,
            scope,
            customer,
            plan,
            plan_operation,
        }
    }
    async fn snapshot(&self) -> SubscriptionDependencySnapshot {
        self.store
            .snapshot_subscription_dependencies(
                &self.scope,
                self.customer,
                self.plan,
                self.plan_operation,
                "fixture-version",
            )
            .await
            .unwrap()
    }
    async fn count(&self, table: &str) -> i64 {
        let mut tx = self.pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('forklaunch.tenant_id','subscription-a',true)")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query_scalar(&format!("SELECT count(*) FROM fl_billing.{table}"))
            .fetch_one(&mut *tx)
            .await
            .unwrap()
    }
    async fn denied(&self, sql: &str, tenant: &str) {
        let mut tx = self.pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('forklaunch.tenant_id',$1,true)")
            .bind(tenant)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            sqlx::raw_sql(sql).execute(&mut *tx).await.is_err(),
            "unexpected success: {sql}"
        );
        tx.rollback().await.unwrap();
    }
    async fn cleanup(self) {
        self.pool.close().await;
        self.owner.close().await;
        sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", self.database))
            .execute(&self.admin)
            .await
            .unwrap();
        sqlx::raw_sql(&format!(
            "DROP ROLE {}; DROP ROLE {}",
            self.runtime_role, self.owner_role
        ))
        .execute(&self.admin)
        .await
        .unwrap();
        self.admin.close().await;
    }
}

async fn record_bodies(pool: &PgPool) -> Vec<(String, sqlx::types::Json<serde_json::Value>)> {
    let mut tx = tenant_tx(pool).await;
    sqlx::query_as("SELECT kind,data FROM fl_billing.records ORDER BY kind")
        .fetch_all(&mut *tx)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "owned isolated PostgreSQL database, no provider calls"]
async fn subscription_reservation_barrier_matrix() {
    let f = Arc::new(Fixture::new().await);
    let test = f.clone();
    let result = tokio::task::LocalSet::new()
        .run_until(async move { tokio::task::spawn_local(async move { run(&test).await }).await })
        .await;
    Arc::try_unwrap(f).ok().unwrap().cleanup().await;
    assert!(
        result.is_ok(),
        "fixture assertion failed; isolated database/roles cleaned"
    );
}
#[allow(clippy::too_many_lines)]
async fn run(f: &Fixture) {
    stale_authority(f).await;
    let mut nested = json!(null);
    for _ in 0..40 {
        nested = json!([nested]);
    }
    for payload in [nested, json!(vec![0; 9000]), json!("x".repeat(262_145))] {
        let snapshot = f.snapshot().await;
        let before = f.count("subscription_creation_audit").await;
        assert!(
            f.store
                .reserve_subscription_creation(snapshot, Uuid::new_v4(), &payload, || panic!(
                    "hostile input reached freshness/DB work"
                ))
                .await
                .is_err()
        );
        assert_eq!(f.count("subscription_creation_intents").await, 0);
        assert_eq!(f.count("subscription_creation_audit").await, before);
    }
    let blank = Fixture::store(f.pool.clone(), " ").await;
    assert!(
        blank
            .inspect_subscription_creation(Uuid::new_v4())
            .await
            .is_err()
    );
    // Store rejects membership in the migration owner even without SUPERUSER/BYPASSRLS.
    let owner_store = Fixture::store(f.owner.clone(), "service:subscription").await;
    assert!(
        owner_store
            .snapshot_subscription_dependencies(
                &f.scope,
                f.customer,
                f.plan,
                f.plan_operation,
                "fixture-version"
            )
            .await
            .is_err()
    );
    let snapshot = f.snapshot().await;
    assert_eq!(snapshot.provider_customer_id(), "cus_owned");
    assert_eq!(snapshot.provider_plan_id(), "custom-plan");
    assert_eq!(snapshot.plan_receipt()["id"], f.plan.to_string());
    // Audit failure rolls back both journal and all four claim counters.
    sqlx::query(&format!(
        "REVOKE INSERT ON fl_billing.subscription_creation_audit FROM {}",
        f.runtime_role
    ))
    .execute(&f.owner)
    .await
    .unwrap();
    assert!(
        f.store
            .reserve_subscription_creation(snapshot, Uuid::new_v4(), &json!({"quantity":1}), || Ok(
                ()
            ))
            .await
            .is_err()
    );
    assert_eq!(f.count("subscription_creation_intents").await, 0);
    assert_eq!(f.count("subscription_dependency_claims").await, 0);
    sqlx::query(&format!(
        "GRANT INSERT ON fl_billing.subscription_creation_audit TO {}",
        f.runtime_role
    ))
    .execute(&f.owner)
    .await
    .unwrap();
    // Freshness failure after claims/audit also rolls everything back immediately.
    let calls = AtomicUsize::new(0);
    assert!(
        f.store
            .reserve_subscription_creation(
                f.snapshot().await,
                Uuid::new_v4(),
                &json!({"quantity":1}),
                || {
                    if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                        Ok(())
                    } else {
                        Err(BlueprintError::new("expired", "Expired"))
                    }
                }
            )
            .await
            .is_err()
    );
    assert_eq!(f.count("subscription_creation_intents").await, 0);
    let operation = Uuid::new_v4();
    let a = f.snapshot().await;
    let b = f.snapshot().await;
    let payload = json!({"quantity":1});
    let (a, b) = tokio::join!(
        f.store
            .reserve_subscription_creation(a, operation, &payload, || Ok(())),
        f.store
            .reserve_subscription_creation(b, operation, &payload, || Ok(()))
    );
    let results = [a.unwrap(), b.unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|v| matches!(v, SubscriptionCreationClaim::Reserved(_)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|v| matches!(v, SubscriptionCreationClaim::Existing(_)))
            .count(),
        1
    );
    assert_eq!(f.count("subscription_dependency_claims").await, 4);
    assert!(
        f.store
            .inspect_subscription_creation(operation)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        f.store
            .reserve_subscription_creation(
                f.snapshot().await,
                operation,
                &json!({"quantity":2}),
                || Ok(())
            )
            .await
            .is_err()
    );
    for table in [
        "subscription_creation_intents",
        "subscription_dependency_claims",
    ] {
        f.denied(&format!("DELETE FROM fl_billing.{table}"), "subscription-a")
            .await;
        f.denied(&format!("TRUNCATE fl_billing.{table} CASCADE"), "")
            .await;
    }
    f.denied("TRUNCATE fl_billing.subscription_dependency_guards", "")
        .await;
    for sql in [
        "UPDATE fl_billing.records SET data=data WHERE kind IN ('customer','plan')",
        "DELETE FROM fl_billing.stripe_customer_bindings",
        "DELETE FROM fl_billing.provider_bindings WHERE kind='plan'",
        "UPDATE fl_billing.provider_bindings SET binding_id=gen_random_uuid() WHERE kind='plan'",
    ] {
        f.denied(sql, "subscription-a").await;
    }
    for table in ["records", "stripe_customer_bindings", "provider_bindings"] {
        f.denied(
            &format!("TRUNCATE fl_billing.{table} CASCADE"),
            "wrong-tenant",
        )
        .await;
    }
    // Broad runtime table grants do not grant private counter authority.
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('forklaunch.tenant_id','subscription-a',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    let visible: i64 =
        sqlx::query_scalar("SELECT count(*) FROM fl_billing.subscription_dependency_guards")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(visible, 0);
    assert_eq!(
        sqlx::query("UPDATE fl_billing.subscription_dependency_guards SET claims=0")
            .execute(&mut *tx)
            .await
            .unwrap()
            .rows_affected(),
        0
    );
    tx.rollback().await.unwrap();
    f.denied(
        "SELECT fl_billing.claim_subscription_dependency()",
        "subscription-a",
    )
    .await;
    // Even if an unrelated permissive record policy allows foreign-tenant writes,
    // the trigger uses OLD.tenant_id, not the caller's GUC, and still sees claims.
    sqlx::query(&format!("CREATE POLICY fixture_cross_tenant ON fl_billing.records TO {} USING(true) WITH CHECK(true)",f.runtime_role)).execute(&f.owner).await.unwrap();
    f.denied(
        "UPDATE fl_billing.records SET data=data WHERE kind='customer'",
        "",
    )
    .await;
    f.denied(
        "DELETE FROM fl_billing.records WHERE kind='plan'",
        "wrong-tenant",
    )
    .await;
    sqlx::query("DROP POLICY fixture_cross_tenant ON fl_billing.records")
        .execute(&f.owner)
        .await
        .unwrap();
    let wrong_actor = Fixture::store(f.pool.clone(), "service:other").await;
    assert!(
        wrong_actor
            .inspect_subscription_creation(operation)
            .await
            .is_err()
    );
    let foreign = PostgresBillingStore::new(
        f.pool.clone(),
        TenantContext::new("subscription-b").unwrap(),
        Arc::new(FieldEncryptor::new("owned-subscription-foundation-key").unwrap()),
        AuditContext::actor("service:subscription", "foreign"),
    )
    .await
    .unwrap();
    assert!(
        foreign
            .inspect_subscription_creation(operation)
            .await
            .unwrap()
            .is_none()
    );
    lost_ack(f).await;
}
async fn stale_authority(f: &Fixture) {
    let old = f.snapshot().await;
    let binding = f
        .store
        .stripe_customer_binding(&f.scope, f.customer)
        .await
        .unwrap()
        .unwrap();
    f.store
        .revoke_stripe_customer_binding(&binding)
        .await
        .unwrap();
    let party = TrustedCustomerParty::administrative(
        f.customer,
        "user".into(),
        "external-party-assertion".into(),
    )
    .unwrap();
    let local = f
        .store
        .snapshot_customer_registration(&f.scope, &party, "cus_owned", "fixture-version")
        .await
        .unwrap();
    let replacement = f
        .store
        .complete_customer_registration(local, || Ok(()))
        .await
        .unwrap();
    assert_ne!(binding.generation(), replacement.generation());
    assert!(
        f.store
            .reserve_subscription_creation(old, Uuid::new_v4(), &json!({"quantity":1}), || Ok(()))
            .await
            .is_err()
    );
    assert!(
        f.store
            .revoke_stripe_customer_binding(&binding)
            .await
            .is_err()
    );
    let mut tx = tenant_tx(&f.pool).await;
    let original: sqlx::types::Json<serde_json::Value> =
        sqlx::query_scalar("SELECT data FROM fl_billing.records WHERE kind='plan'")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    sqlx::query(
        "UPDATE fl_billing.records SET data=jsonb_set(data,'{active}','false') WHERE kind='plan'",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(
        f.store
            .snapshot_subscription_dependencies(
                &f.scope,
                f.customer,
                f.plan,
                f.plan_operation,
                "fixture-version"
            )
            .await
            .is_err()
    );
    let mut tx = tenant_tx(&f.pool).await;
    sqlx::query("UPDATE fl_billing.records SET data=$1 WHERE kind='plan'")
        .bind(original)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let foreign = PostgresBillingStore::new(
        f.pool.clone(),
        TenantContext::new("subscription-b").unwrap(),
        Arc::new(FieldEncryptor::new("owned-subscription-foundation-key").unwrap()),
        AuditContext::actor("service:subscription", "foreign"),
    )
    .await
    .unwrap();
    assert!(
        foreign
            .snapshot_subscription_dependencies(
                &f.scope,
                f.customer,
                f.plan,
                f.plan_operation,
                "fixture-version"
            )
            .await
            .is_err()
    );
    assert_eq!(f.count("subscription_creation_intents").await, 0);
}
async fn lost_ack(f: &Fixture) {
    let options = (*f.pool.connect_options()).clone();
    let (port, armed, proxy) =
        commit_ack_proxy::start(options.get_host().into(), options.get_port()).await;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            options
                .host("127.0.0.1")
                .port(port)
                .ssl_mode(PgSslMode::Disable),
        )
        .await
        .unwrap();
    let store = Fixture::store(pool.clone(), "service:subscription").await;
    let operation = Uuid::new_v4();
    let snapshot = f.snapshot().await;
    armed.store(true, Ordering::SeqCst);
    assert!(
        store
            .reserve_subscription_creation(snapshot, operation, &json!({"quantity":1}), || Ok(()))
            .await
            .is_err()
    );
    assert!(!armed.load(Ordering::SeqCst));
    assert!(
        f.store
            .inspect_subscription_creation(operation)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(f.count("subscription_dependency_claims").await, 8);
    assert!(matches!(
        f.store
            .reserve_subscription_creation(
                f.snapshot().await,
                operation,
                &json!({"quantity":1}),
                || Ok(())
            )
            .await
            .unwrap(),
        SubscriptionCreationClaim::Existing(_)
    ));
    pool.close().await;
    proxy.abort();
}

#[tokio::test]
#[ignore = "owned isolated PostgreSQL database, deterministic row/advisory lock races"]
async fn subscription_dependency_lock_races() {
    let f = Arc::new(Fixture::new().await);
    let test = f.clone();
    let result = tokio::task::LocalSet::new()
        .run_until(
            async move { tokio::task::spawn_local(async move { lock_races(test).await }).await },
        )
        .await;
    Arc::try_unwrap(f).ok().unwrap().cleanup().await;
    assert!(
        result.is_ok(),
        "race fixture failed; isolated database/roles cleaned"
    );
}
async fn tenant_tx(pool: &PgPool) -> sqlx::Transaction<'_, sqlx::Postgres> {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('forklaunch.tenant_id','subscription-a',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx
}
async fn wait_for(f: &Fixture, event: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting:bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=$1 AND usename=$2 AND wait_event=$3)")
                .bind(&f.database).bind(&f.runtime_role).bind(event).fetch_one(&f.admin).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
}
#[allow(clippy::too_many_lines)]
async fn lock_races(f: Arc<Fixture>) {
    // Mutation takes the local row first: reservation must reject its old body.
    let snapshot = f.snapshot().await;
    let mut mutation = tenant_tx(&f.pool).await;
    sqlx::query("UPDATE fl_billing.records SET data=jsonb_set(data,'{updatedAt}','\"2026-10-14T01:00:00Z\"') WHERE kind='customer'")
        .execute(&mut *mutation).await.unwrap();
    let store = f.store.clone();
    let reserve = tokio::spawn(async move {
        store
            .reserve_subscription_creation(snapshot, Uuid::new_v4(), &json!({"quantity":1}), || {
                Ok(())
            })
            .await
    });
    wait_for(&f, "transactionid").await;
    mutation.commit().await.unwrap();
    assert!(reserve.await.unwrap().is_err());
    assert_eq!(f.count("subscription_creation_intents").await, 0);

    // Expiry is checked after awaited dependency locks, not just before BEGIN.
    let snapshot = f.snapshot().await;
    let mut lock = tenant_tx(&f.pool).await;
    sqlx::query("SELECT id FROM fl_billing.records WHERE kind='customer' FOR UPDATE")
        .execute(&mut *lock)
        .await
        .unwrap();
    let store = f.store.clone();
    let expired = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let check = expired.clone();
    let reserve = tokio::spawn(async move {
        store
            .reserve_subscription_creation(
                snapshot,
                Uuid::new_v4(),
                &json!({"quantity":1}),
                move || {
                    if check.load(Ordering::SeqCst) {
                        Err(BlueprintError::new("expired", "Expired"))
                    } else {
                        Ok(())
                    }
                },
            )
            .await
    });
    wait_for(&f, "transactionid").await;
    expired.store(true, Ordering::SeqCst);
    lock.rollback().await.unwrap();
    assert!(reserve.await.unwrap().is_err());
    assert_eq!(f.count("subscription_creation_intents").await, 0);

    // Pause reservation after four claims have been inserted but before COMMIT.
    // Both mutation statements start with a snapshot that predates those claims.
    sqlx::raw_sql("CREATE FUNCTION fl_billing.fixture_pause_reservation() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(1729101); RETURN NEW; END $$; CREATE TRIGGER fixture_pause BEFORE INSERT ON fl_billing.subscription_creation_audit FOR EACH ROW WHEN(NEW.operation='reserve') EXECUTE FUNCTION fl_billing.fixture_pause_reservation()")
        .execute(&f.owner).await.unwrap();
    let mut blocker = f.owner.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(1729101)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let snapshot = f.snapshot().await;
    let store = f.store.clone();
    let reserve = tokio::spawn(async move {
        store
            .reserve_subscription_creation(snapshot, Uuid::new_v4(), &json!({"quantity":1}), || {
                Ok(())
            })
            .await
    });
    wait_for(&f, "advisory").await;
    let pool = f.pool.clone();
    let read_committed = tokio::spawn(async move {
        let mut tx = tenant_tx(&pool).await;
        let result = sqlx::query("UPDATE fl_billing.records SET data=data WHERE kind='customer'")
            .execute(&mut *tx)
            .await;
        tx.rollback().await.unwrap();
        result.is_err()
    });
    let pool = f.pool.clone();
    let repeatable = tokio::spawn(async move {
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT set_config('forklaunch.tenant_id','subscription-a',true)")
            .execute(&mut *tx)
            .await
            .unwrap();
        let result = sqlx::query("UPDATE fl_billing.records SET data=data WHERE kind='plan'")
            .execute(&mut *tx)
            .await;
        tx.rollback().await.unwrap();
        result.is_err()
    });
    // There must be two row mutations waiting, not merely a scheduling delay.
    tokio::time::timeout(std::time::Duration::from_secs(5),async {
        loop {
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=$1 AND usename=$2 AND wait_event='transactionid'")
                .bind(&f.database).bind(&f.runtime_role).fetch_one(&f.admin).await.unwrap();
            if count==2 {break;}
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    blocker.commit().await.unwrap();
    assert!(matches!(
        reserve.await.unwrap().unwrap(),
        SubscriptionCreationClaim::Reserved(_)
    ));
    assert!(read_committed.await.unwrap());
    assert!(repeatable.await.unwrap());
    assert_eq!(f.count("subscription_dependency_claims").await, 4);
}
