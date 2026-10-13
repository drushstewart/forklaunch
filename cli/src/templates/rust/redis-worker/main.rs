mod signals;
use std::{env, time::Duration};
use forklaunch_worker_database::http::HttpConfig;
use forklaunch_core::persistence::TenantContext;
use forklaunch_worker_database::runner::RunnerConfig;
use forklaunch_worker_redis::leased::RedisLeasedQueue;
use forklaunch_worker_database::retention::RetentionPolicy;
use forklaunch_worker_interface::{AsyncWorkerAdmin, AsyncWorkerQueue, Job, JobSnapshot, RetryPolicy, TerminalStatus};
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
    let telemetry = forklaunch_core::telemetry::otlp::TelemetryRuntime::start(
        WORKER_NAME, env!("CARGO_PKG_VERSION"),
    ).await?;
    let result = run().await;
    let flushed = telemetry.shutdown().await;
    result?;
    flushed?;
    Ok(())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if !args.is_empty() && args != ["--control-service"] && !(args.len() == 3 && args[0] == "--enqueue")
        && !(args.len() == 2 && matches!(args[0].as_str(), "--inspect" | "--dead-letters" | "--retry" | "--signal"))
        && !(args.len() == 3 && matches!(args[0].as_str(), "--purge-completed" | "--purge-failed")) {
        return Err("Expected --signal pause/resume/drain, no arguments, --control-service, --enqueue ID JSON, --inspect ID, --dead-letters LIMIT, --retry ID, or --purge-completed/--purge-failed AGE_SECONDS LIMIT".into());
    }
    if args == ["--control-service"] {
        let http = http_config()?.ok_or("--control-service requires WORKER_HTTP_ADDR")?;
        signals::serve_management(&http, shutdown()).await?;
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("--signal") {
        signals::publish(&args[1]).await?;
        return Ok(());
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
        println!("{}", snapshot.map_or(serde_json::Value::Null, snapshot_json));
    } else if args.first().map(String::as_str) == Some("--dead-letters") {
        let jobs = queue.dead_letters(&queue_name, args[1].parse()?).await?;
        println!("{}", serde_json::to_string(&jobs.into_iter().map(snapshot_json).collect::<Vec<_>>())?);
    } else if args.first().map(String::as_str) == Some("--retry") {
        let requeued = queue.retry_dead_letter(&queue_name, &args[1], policy).await?;
        println!("{}", serde_json::json!({"requeued":requeued}));
    } else if args.first().is_some_and(|arg| matches!(arg.as_str(), "--purge-completed" | "--purge-failed")) {
        let status = if args[0] == "--purge-completed" { TerminalStatus::Completed } else { TerminalStatus::Failed };
        let purged = queue.purge_terminal(&queue_name, status, Duration::from_secs(args[1].parse()?), args[2].parse()?).await?;
        println!("{}", serde_json::json!({"purged":purged}));
    } else {
        let config = RunnerConfig {
            queue: queue_name,
            concurrency: usize::try_from(number("WORKER_CONCURRENCY", 1)?)?,
            batch_size: usize::try_from(number("WORKER_BATCH_SIZE", 1)?)?,
            poll_interval: Duration::from_millis(number("WORKER_POLL_MS", 250)?),
            lease_duration: Duration::from_millis(number("WORKER_LEASE_MS", 30000)?),
            renew_interval: Duration::from_millis(number("WORKER_RENEW_MS", 10000)?),
            drain_timeout: Duration::from_millis(number("WORKER_DRAIN_MS", 30000)?),
        };
        config.validate()?;
        let retention = retention_policy()?;
        let http = http_config()?;
        println!("{} worker ready", WORKER_NAME);
        let result = signals::run(&queue, &ApplicationHandler, &config, retention.as_ref(), http.as_ref(), shutdown()).await?;
        if retention.is_some() {
            println!("retention stopped: completed_purged={} failed_purged={}", result.retention.completed_purged, result.retention.failed_purged);
        }
        let report = result.worker;
        println!("worker stopped: completed={} retried={} dead_lettered={} lease_lost={} abandoned={}",
            report.completed, report.retried, report.dead_lettered, report.lease_lost, report.abandoned);
    }
    Ok(())
}

fn snapshot_json(snapshot: JobSnapshot) -> serde_json::Value {
    serde_json::json!({"id":snapshot.job.id, "queue":snapshot.job.queue,
        "attempts":snapshot.job.attempts, "status":format!("{:?}",snapshot.status),
        "last_error":snapshot.last_error})
}

fn http_config() -> Result<Option<HttpConfig>, Box<dyn std::error::Error>> {
    let address = match env::var("WORKER_HTTP_ADDR") {
        Ok(value) => Some(value.parse()?),
        Err(env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    let control_token = match env::var("WORKER_CONTROL_TOKEN") {
        Ok(value) => Some(value),
        Err(env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    let Some(address) = address else {
        if control_token.is_some() { return Err("WORKER_CONTROL_TOKEN requires WORKER_HTTP_ADDR".into()); }
        return Ok(None);
    };
    let config = HttpConfig { address, control_token };
    config.validate()?;
    Ok(Some(config))
}

fn retention_policy() -> Result<Option<RetentionPolicy>, Box<dyn std::error::Error>> {
    let policy = RetentionPolicy {
        completed_age: optional_number("WORKER_COMPLETED_RETENTION_SECONDS")?.map(Duration::from_secs),
        failed_age: optional_number("WORKER_FAILED_RETENTION_SECONDS")?.map(Duration::from_secs),
        interval: Duration::from_millis(number("WORKER_RETENTION_INTERVAL_MS", 60000)?),
        batch_limit: usize::try_from(number("WORKER_RETENTION_BATCH_LIMIT", 100)?)?,
        operation_timeout: Duration::from_millis(number("WORKER_RETENTION_TIMEOUT_MS", 5000)?),
    };
    policy.validate()?;
    Ok((policy.completed_age.is_some() || policy.failed_age.is_some()).then_some(policy))
}

fn optional_number(name: &str) -> Result<Option<u64>, Box<dyn std::error::Error>> {
    match env::var(name) {
        Ok(value) => Ok(Some(value.parse()?)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error.into()),
    }
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
