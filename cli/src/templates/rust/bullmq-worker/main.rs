mod signals;
use std::{env, time::Duration};
use forklaunch_worker_database::http::HttpConfig;
use forklaunch_worker_interface::Job;
use forklaunch_worker_database::runner::JobHandler;
use forklaunch_worker_redis::bullmq::{BullMqOptions, BullMqProducer, BullMqConsumer, protocol};
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
    if !args.is_empty() && args != ["--migrate"]
        && !(args.len() == 3 && args[0] == "--enqueue")
        && !(args.len() == 2 && matches!(args[0].as_str(), "--peek" | "--dead-letters" | "--signal")) {
        return Err("Expected no arguments, --migrate, --enqueue ID JSON, --peek LIMIT, --dead-letters LIMIT, or --signal pause/resume/drain".into());
    }
    if args.first().map(String::as_str) == Some("--signal") { signals::publish(&args[1]).await?; return Ok(()); }
    let queue_name = env::var("WORKER_QUEUE").unwrap_or_else(|_| WORKER_NAME.to_owned());
    let mut options = BullMqOptions::new(&env::var("REDIS_URL").map_err(|_| "REDIS_URL is required")?);
    options.retries = u32::try_from(number("WORKER_MAX_ATTEMPTS", 3)?)?;
    let interval = number("WORKER_BACKOFF_MS", 5000)?;
    options.backoff = match env::var("WORKER_BACKOFF_TYPE").as_deref() {
        Ok("fixed") => protocol::types::BackoffStrategy::Fixed(interval),
        Ok("exponential") | Err(env::VarError::NotPresent) => protocol::types::BackoffStrategy::Exponential(interval),
        _ => return Err("WORKER_BACKOFF_TYPE must be fixed or exponential".into()),
    };
    options.operation_timeout = Duration::from_millis(number("WORKER_OPERATION_TIMEOUT_MS", 5000)?);
    options.close_timeout = Duration::from_millis(number("WORKER_DRAIN_MS", 30000)?);
    let concurrency = usize::try_from(number("WORKER_CONCURRENCY", 1)?)?;
    if concurrency == 0 || concurrency > 1024 { return Err("WORKER_CONCURRENCY must be between 1 and 1024".into()); }
    options.worker.concurrency = concurrency;
    match args.first().map(String::as_str) {
        Some("--migrate") => {
            let producer = BullMqProducer::connect(&queue_name, options).await?;
            producer.close().await;
        }
        Some("--enqueue") => {
            let payload: serde_json::Value = serde_json::from_str(&args[2])?;
            let mut event = payload.as_object().cloned().ok_or("BullMQ event payload must be a JSON object")?;
            event.insert("id".into(), args[1].clone().into());
            let producer = BullMqProducer::connect(&queue_name, options).await?;
            let id = producer.enqueue_job(event.into()).await?;
            producer.close().await;
            println!("{}", serde_json::json!({"id": id}));
        }
        Some("--peek" | "--dead-letters") => {
            let limit: usize = args[1].parse()?;
            if limit > 10000 { return Err("Inspection limit must be at most 10000".into()); }
            let producer = BullMqProducer::connect(&queue_name, options).await?;
            let result = async {
                if limit == 0 { return Ok::<_, Box<dyn std::error::Error>>(Vec::new()); }
                if args[0] == "--peek" { return Ok(producer.peek_events().await?.into_iter().take(limit).collect()); }
                let jobs = producer.inner().get_jobs(&["failed"], 0, i64::try_from(limit)? - 1, false).await
                    .map_err(|_| "BullMQ inspection failed")?;
                Ok(jobs.into_iter().map(|job| serde_json::json!({"id":job.id(),"event":job.data(),"attempts":job.attempts_made()})).collect())
            }.await;
            producer.close().await;
            println!("{}", serde_json::to_string(&result?)?);
        }
        None => {
            let handler_timeout = Duration::from_millis(number("WORKER_HANDLER_TIMEOUT_MS", 300000)?);
            if handler_timeout.is_zero() { return Err("WORKER_HANDLER_TIMEOUT_MS must be positive".into()); }
            let http = http_config()?;
            let processing_queue = queue_name.clone();
            let worker = BullMqConsumer::connect(&queue_name, move |job: protocol::Job, _token| {
                let queue_name = processing_queue.clone();
                async move {
                    let event = Job::new(job.name(), queue_name, job.data().to_string()).with_attempts(job.attempts_started());
                    match tokio::time::timeout(handler_timeout, ApplicationHandler.process(event)).await {
                        Ok(Ok(())) => Ok(serde_json::Value::Null),
                        Ok(Err(error)) => Err(protocol::Error::ProcessingError(error.code.to_owned())),
                        Err(_) => Err(protocol::Error::ProcessingError("worker_handler_timeout".into())),
                    }
                }
            }, options).await?;
            println!("{} worker ready", WORKER_NAME);
            signals::run(&worker, http.as_ref(), shutdown()).await?;
            println!("worker stopped");
        }
        _ => unreachable!("arguments validated"),
    }
    Ok(())
}

fn number(name: &str, default: u64) -> Result<u64, Box<dyn std::error::Error>> {
    match env::var(name) { Ok(value) => Ok(value.parse()?), Err(env::VarError::NotPresent) => Ok(default), Err(error) => Err(error.into()) }
}

fn http_config() -> Result<Option<HttpConfig>, Box<dyn std::error::Error>> {
    let address = match env::var("WORKER_HTTP_ADDR") { Ok(value) => Some(value.parse()?), Err(env::VarError::NotPresent) => None, Err(error) => return Err(error.into()) };
    let control_token = match env::var("WORKER_CONTROL_TOKEN") { Ok(value) => Some(value), Err(env::VarError::NotPresent) => None, Err(error) => return Err(error.into()) };
    let Some(address) = address else {
        if control_token.is_some() { return Err("WORKER_CONTROL_TOKEN requires WORKER_HTTP_ADDR".into()); }
        return Ok(None);
    };
    let config = HttpConfig { address, control_token }; config.validate()?; Ok(Some(config))
}

async fn shutdown() {
    #[cfg(unix)] {
        if let Ok(mut signal) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = signal.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
