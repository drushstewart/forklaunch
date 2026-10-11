use forklaunch_ecommerce_base::{BaseEcommerceService, store::EcommerceStore};
use forklaunch_ecommerce_interface::{AdjustStockDto, InventoryService, events::OrderEvent};
use serde_json::json;
use std::sync::Arc;
fn event(id: &str, from: &str, to: &str, items: &[(&str, i64)]) -> OrderEvent {
    serde_json::from_value(json!({"id":id,"orderId":"event-order","fromStatus":from,"toStatus":to,"items":items.iter().map(|(variant,quantity)|json!({"variantId":variant,"quantity":quantity,"unitPriceCents":100})).collect::<Vec<_>>()})).unwrap()
}
#[allow(clippy::too_many_lines)]
pub async fn event_rules<S: EcommerceStore + 'static>(store: Arc<S>) {
    let service = BaseEcommerceService::new(Arc::clone(&store));
    for (variant, stock) in [("event-a", 10), ("event-b", 2)] {
        service
            .create_inventory(
                serde_json::from_value(json!({"variantId":variant,"stock":stock})).unwrap(),
            )
            .await
            .unwrap();
    }
    let payment = event(
        "event-paid",
        "pending",
        "paid",
        &[("event-a", 2), ("event-b", 3)],
    );
    assert!(service.apply_order_event(&payment).await.is_err());
    assert_eq!(
        service.get_inventory("event-a").await.unwrap().stock,
        10,
        "later failure rolls back earlier delta"
    );
    service
        .adjust_stock(AdjustStockDto {
            variant_id: "event-b".into(),
            delta: 8,
        })
        .await
        .unwrap();
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let worker = service.clone();
        let payload = payment.clone();
        tasks.push(tokio::spawn(async move {
            worker.apply_order_event(&payload).await.unwrap()
        }));
    }
    let mut applied = 0;
    for task in tasks {
        applied += usize::from(task.await.unwrap());
    }
    assert_eq!(applied, 1);
    assert_eq!(service.get_inventory("event-a").await.unwrap().stock, 8);
    assert_eq!(service.get_inventory("event-b").await.unwrap().stock, 7);
    let mut redelivery = payment.clone();
    redelivery.retry_count = 4;
    redelivery.processed = true;
    assert!(!service.apply_order_event(&redelivery).await.unwrap());
    let mut conflict = payment.clone();
    conflict.items[0].quantity = 1;
    assert_eq!(
        service.apply_order_event(&conflict).await.unwrap_err().code,
        "ecommerce_event_conflict"
    );
    let cancellation = event(
        "event-cancel",
        "paid",
        "cancelled",
        &[("event-a", 2), ("event-b", 3)],
    );
    assert!(service.apply_order_event(&cancellation).await.unwrap());
    assert!(!service.apply_order_event(&cancellation).await.unwrap());
    assert_eq!(service.get_inventory("event-a").await.unwrap().stock, 10);
    assert_eq!(service.get_inventory("event-b").await.unwrap().stock, 10);
    assert!(
        service
            .apply_order_event(&event(
                "event-unpaid-cancel",
                "pending",
                "cancelled",
                &[("event-a", 3)]
            ))
            .await
            .unwrap()
    );
    assert_eq!(service.get_inventory("event-a").await.unwrap().stock, 10);
    assert!(
        service
            .apply_order_event(&event(
                "event-fulfilled-cancel",
                "fulfilled",
                "cancelled",
                &[("event-a", 2)]
            ))
            .await
            .unwrap()
    );
    assert_eq!(service.get_inventory("event-a").await.unwrap().stock, 12);
    assert!(
        service
            .apply_order_event(&event(
                "event-duplicate-items",
                "pending",
                "paid",
                &[("event-a", 2), ("event-a", 3)]
            ))
            .await
            .unwrap()
    );
    assert_eq!(service.get_inventory("event-a").await.unwrap().stock, 7);
    for invalid in [
        event("event-zero", "pending", "paid", &[("event-a", 0)]),
        event("event-negative", "pending", "paid", &[("event-a", -1)]),
        event(
            "event-overflow",
            "pending",
            "paid",
            &[("event-a", i64::MAX), ("event-a", 1)],
        ),
        event("event-illegal", "delivered", "paid", &[("event-a", 1)]),
        event(
            "event-missing-stock",
            "pending",
            "paid",
            &[("event-a", 1), ("event-missing", 1)],
        ),
    ] {
        assert!(service.apply_order_event(&invalid).await.is_err());
        assert_eq!(service.get_inventory("event-a").await.unwrap().stock, 7);
    }
    #[cfg(feature = "worker")]
    {
        use forklaunch_ecommerce_base::events::OrderInventoryHandler;
        use forklaunch_worker_database::runner::JobHandler;
        use forklaunch_worker_interface::Job;
        let handler = OrderInventoryHandler::new(Arc::clone(&store), "order-events");
        handler
            .process(Job::new(
                &payment.id,
                "order-events",
                serde_json::to_string(&payment).unwrap(),
            ))
            .await
            .unwrap();
        for job in [
            Job::new(
                "different-id",
                "order-events",
                serde_json::to_string(&payment).unwrap(),
            ),
            Job::new(
                &payment.id,
                "wrong-queue",
                serde_json::to_string(&payment).unwrap(),
            ),
            Job::new("malformed", "order-events", "{}"),
        ] {
            assert!(handler.process(job).await.is_err());
        }
        assert_eq!(service.get_inventory("event-a").await.unwrap().stock, 7);
    }
}

#[cfg(all(feature = "postgres", feature = "worker"))]
pub async fn lost_ack_redelivery(
    owner: &sqlx::PgPool,
    runtime: &sqlx::PgPool,
    tenant: forklaunch_core::persistence::TenantContext,
    store: Arc<forklaunch_ecommerce_base::postgres::PostgresEcommerceStore>,
) {
    use forklaunch_ecommerce_base::events::OrderInventoryHandler;
    use forklaunch_worker_database::{postgres::PostgresWorkerQueue, runner::JobHandler};
    use forklaunch_worker_interface::{AsyncWorkerQueue, Job, RetryPolicy};
    use std::time::Duration;
    let service = BaseEcommerceService::new(Arc::clone(&store));
    let before = service.get_inventory("event-a").await.unwrap().stock;
    let payload = event("event-lost-ack", "pending", "paid", &[("event-a", 1)]);
    let queue = PostgresWorkerQueue::new(runtime.clone(), tenant.clone(), RetryPolicy::new(3, 0))
        .await
        .unwrap();
    queue
        .enqueue(Job::new(
            &payload.id,
            "order-events",
            serde_json::to_string(&payload).unwrap(),
        ))
        .await
        .unwrap();
    let handler = OrderInventoryHandler::new(store, "order-events");
    let first = queue
        .claim("order-events", Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    handler.process(first.job.clone()).await.unwrap();
    // Deterministically model a crash after commit and before queue ack.
    sqlx::query("UPDATE fl_worker.jobs SET lease_until=clock_timestamp()-INTERVAL '1 second' WHERE tenant_id=$1 AND queue='order-events' AND id=$2")
        .bind(tenant.id()).bind(&payload.id).execute(owner).await.unwrap();
    assert!(!queue.ack(&first).await.unwrap());
    let second = queue
        .claim("order-events", Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_ne!(first.token, second.token);
    let reopened = forklaunch_ecommerce_base::postgres::PostgresEcommerceStore::new(
        runtime.clone(),
        tenant,
        Arc::new(
            forklaunch_core::persistence::encryption::FieldEncryptor::new("unused-inventory-key")
                .unwrap(),
        ),
        forklaunch_blueprint_core::AuditContext::system("redelivery-fixture"),
    )
    .await
    .unwrap();
    OrderInventoryHandler::new(Arc::new(reopened), "order-events")
        .process(second.job.clone())
        .await
        .unwrap();
    assert!(queue.ack(&second).await.unwrap());
    assert_eq!(
        service.get_inventory("event-a").await.unwrap().stock,
        before - 1
    );
    assert!(
        queue
            .claim("order-events", Duration::from_secs(30))
            .await
            .unwrap()
            .is_none()
    );
}
