use std::{env, time::Duration};
use forklaunch_core::persistence::TenantContext;
use forklaunch_worker_database::runner::{self, RunnerConfig};
use forklaunch_worker_redis::leased::RedisLeasedQueue;
use forklaunch_worker_interface::{AsyncWorkerQueue, Job, RetryPolicy};
use {{crate_module}}::{handler::ApplicationHandler, WORKER_NAME, write_contract_artifacts};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if matches!(env::var("FORKLAUNCH_MODE").as_deref(), Ok("openapi" | "contract")) {
        write_contract_artifacts(
            env::var("FORKLAUNCH_CONTRACT_OUTPUT").unwrap_or_else(|_| ".forklaunch/contracts/contract.json".to_owned()),
            env::var("FORKLAUNCH_OPENAPI_OUTPUT").unwrap_or_else(|_| ".forklaunch/openapi/openapi.json".to_owned()),
        )?;
        return Ok(());
    }
    let args: Vec<_> = env::args().skip(1).collect();
    if !args.is_empty() && !(args.len() == 3 && args[0] == "--enqueue")
        && !(args.len() == 2 && args[0] == "--inspect") {
        return Err("Expected no arguments, --enqueue ID JSON, or --inspect ID".into());
    }
    let url = env::var("REDIS_URL").map_err(|_| "REDIS_URL is required")?;
    let namespace = env::var("WORKER_NAMESPACE").map_err(|_| "WORKER_NAMESPACE is required")?;
    let tenant = TenantContext::new(env::var("TENANT_ID").map_err(|_| "TENANT_ID is required")?)?;
    let policy = RetryPolicy::new(
        u32::try_from(number("WORKER_MAX_ATTEMPTS", 3)?)?,
        number("WORKER_BACKOFF_SECONDS", 30)?,
    );
    let queue = RedisLeasedQueue::connect(&url, &namespace, tenant, policy).await?;
    let queue_name = env::var("WORKER_QUEUE").unwrap_or_else(|_| WORKER_NAME.to_owned());
    if args.first().map(String::as_str) == Some("--enqueue") {
        queue.enqueue(Job::new(&args[1], &queue_name, &args[2])).await?;
        println!("job enqueued");
    } else if args.first().map(String::as_str) == Some("--inspect") {
        let snapshot = queue.inspect(&queue_name, &args[1]).await?;
        println!("{}", snapshot.map_or(serde_json::Value::Null, |snapshot| serde_json::json!({
            "id":snapshot.job.id, "queue":snapshot.job.queue,
            "attempts":snapshot.job.attempts, "status":format!("{:?}",snapshot.status),
            "last_error":snapshot.last_error,
        })));
    } else {
        let config = RunnerConfig {
            queue: queue_name,
            poll_interval: Duration::from_millis(number("WORKER_POLL_MS", 250)?),
            lease_duration: Duration::from_millis(number("WORKER_LEASE_MS", 30000)?),
            renew_interval: Duration::from_millis(number("WORKER_RENEW_MS", 10000)?),
            drain_timeout: Duration::from_millis(number("WORKER_DRAIN_MS", 30000)?),
        };
        config.validate()?;
        println!("{} worker ready", WORKER_NAME);
        let report = runner::run(&queue, &ApplicationHandler, &config, shutdown()).await?;
        println!("worker stopped: completed={} retried={} dead_lettered={} lease_lost={} abandoned={}",
            report.completed, report.retried, report.dead_lettered, report.lease_lost, report.abandoned);
    }
    Ok(())
}

fn number(name: &str, default: u64) -> Result<u64, Box<dyn std::error::Error>> {
    match env::var(name) {
        Ok(value) => Ok(value.parse()?),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut signal) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = signal.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
