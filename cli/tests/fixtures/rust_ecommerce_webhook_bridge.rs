//! Test-only native service boundary for executing the actual TS webhook controller.
//! Uses the same restricted role/tenant/encryption as the generated service.
use std::{
    io::{self, Read},
    sync::Arc,
};

use forklaunch_blueprint_core::{AuditContext, BlueprintError, BlueprintResult};
use forklaunch_core::persistence::{TenantContext, encryption::FieldEncryptor};
use forklaunch_ecommerce_base::{BaseEcommerceService, postgres::PostgresEcommerceStore};
use forklaunch_ecommerce_interface::{OrderService, PaymentService};
use forklaunch_ecommerce_paypal::{PaypalClient, PaypalPaymentService};
use forklaunch_worker_database::postgres::PostgresWorkerQueue;
use forklaunch_worker_interface::{AsyncWorkerQueue, Job, RetryPolicy};
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut raw = String::new();
    io::stdin().read_to_string(&mut raw)?;
    let input: Value = serde_json::from_str(&raw)?;
    let provider = input["provider"].as_str().ok_or("missing provider")?;
    assert!(matches!(provider, "stripe" | "paypal"));
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let tenant = TenantContext::new(std::env::var("TENANT_ID")?)?;
    let store = PostgresEcommerceStore::new(
        pool.clone(),
        tenant.clone(),
        Arc::new(FieldEncryptor::from_env()?),
        AuditContext::actor(
            format!("provider:{provider}"),
            format!("webhook-bridge-{}", std::process::id()),
        ),
    )
    .await?;
    let service = BaseEcommerceService::new(Arc::new(store));
    let queue = PostgresWorkerQueue::new(pool.clone(), tenant, RetryPolicy::new(3, 0)).await?;
    let result = run(&input, service, &queue).await;
    println!(
        "{}",
        match result {
            Ok(value) => json!({"ok":value}),
            Err(error) => json!({"error":error.code}),
        }
    );
    pool.close().await;
    Ok(())
}
async fn run(
    input: &Value,
    service: BaseEcommerceService<PostgresEcommerceStore>,
    queue: &PostgresWorkerQueue,
) -> BlueprintResult<Value> {
    match input["operation"].as_str().unwrap_or("") {
        "confirm" => {
            let request = serde_json::from_value(input["body"].clone()).map_err(|_| invalid())?;
            let result = if input["provider"] == "paypal" {
                PaypalPaymentService::new(
                    service,
                    PaypalClient::new(
                        "paypal-id".to_owned(),
                        "paypal-secret".to_owned(),
                        &std::env::var("FORKLAUNCH_FIXTURE_PROVIDER_BASE")
                            .map_err(|_| invalid())?,
                    )?,
                )
                .confirm_payment(request)
                .await?
            } else {
                service.confirm_payment(request).await?
            };
            serde_json::to_value(result).map_err(|_| invalid())
        }
        "fail" => serde_json::to_value(
            service
                .fail_payment(serde_json::from_value(input["body"].clone()).map_err(|_| invalid())?)
                .await?,
        )
        .map_err(|_| invalid()),
        "transition" => serde_json::to_value(
            service
                .transition_order(
                    serde_json::from_value(input["body"].clone()).map_err(|_| invalid())?,
                )
                .await?,
        )
        .map_err(|_| invalid()),
        "enqueue" => {
            let id = input["body"]["id"].as_str().ok_or_else(invalid)?;
            queue
                .enqueue(Job::new(id, "order-events", input["body"].to_string()))
                .await?;
            Ok(Value::Null)
        }
        _ => Err(invalid()),
    }
}
fn invalid() -> BlueprintError {
    BlueprintError::new("fixture_invalid_input", "Invalid webhook fixture input")
}
