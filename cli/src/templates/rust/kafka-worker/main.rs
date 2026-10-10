mod signals;
use std::{env, time::Duration};
use forklaunch_core::persistence::TenantContext;
use forklaunch_worker_database::http::HttpConfig;
use forklaunch_worker_interface::{Job, RetryPolicy};
use forklaunch_worker_kafka::client::{KafkaInspector, KafkaOptions, KafkaProducer, KafkaRunnerConfig, KafkaWorker, initialize_topics};
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
    if !args.is_empty() && args != ["--migrate"]
        && !(args.len() == 3 && args[0] == "--enqueue")
        && !(args.len() == 2 && matches!(args[0].as_str(), "--peek" | "--dead-letters" | "--signal")) {
        return Err("Expected no arguments, --migrate, --enqueue ID JSON, --peek LIMIT, --dead-letters LIMIT, or --signal pause/resume/drain".into());
    }
    if args.first().map(String::as_str) == Some("--signal") { signals::publish(&args[1]).await?; return Ok(()); }
    let mut options = KafkaOptions::new(
        env::var("KAFKA_BROKERS").map_err(|_| "KAFKA_BROKERS is required")?,
        env::var("WORKER_NAMESPACE").map_err(|_| "WORKER_NAMESPACE is required")?,
        TenantContext::new(env::var("TENANT_ID").map_err(|_| "TENANT_ID is required")?)?,
        env::var("WORKER_QUEUE").unwrap_or_else(|_| WORKER_NAME.to_owned()),
        if args.is_empty() { env::var("KAFKA_TRANSACTIONAL_ID").map_err(|_| "KAFKA_TRANSACTIONAL_ID is required for each worker replica")? }
        else if args[0] == "--enqueue" { env::var("KAFKA_PUBLISHER_ID").map_err(|_| "KAFKA_PUBLISHER_ID is required for each independent publisher")? }
        else { "inspection".to_owned() },
    );
    options.group = env::var("KAFKA_GROUP_ID").unwrap_or_else(|_| "workers".to_owned());
    options.operation_timeout = Duration::from_millis(number("KAFKA_OPERATION_TIMEOUT_MS", 5000)?);
    options.session_timeout = Duration::from_millis(number("KAFKA_SESSION_TIMEOUT_MS", 10000)?);
    options.max_payload_bytes = usize::try_from(number("WORKER_MAX_PAYLOAD_BYTES", 1048576)?)?;
    if let Ok(value) = env::var("KAFKA_CLIENT_PROPERTIES") {
        options.properties = serde_json::from_str(&value).map_err(|_| "KAFKA_CLIENT_PROPERTIES must be a JSON object of string properties")?;
    }
    options.validate()?;
    match args.first().map(String::as_str) {
        Some("--migrate") => initialize_topics(&options, i32::try_from(number("KAFKA_PARTITIONS", 3)?)?, i32::try_from(number("KAFKA_REPLICATION_FACTOR", 1)?)?).await?,
        Some("--enqueue") => {
            let queue = options.queue.clone();
            KafkaProducer::connect(options).await?.enqueue(Job::new(&args[1], queue, &args[2])).await?;
            println!("job enqueued");
        }
        Some("--peek") => {
            let jobs = KafkaInspector::new(options)?.peek(args[1].parse()?).await?;
            println!("{}", serde_json::to_string(&jobs.into_iter().map(|job| serde_json::json!({"id":job.id,"queue":job.queue,"attempts":job.attempts})).collect::<Vec<_>>())?);
        }
        Some("--dead-letters") => {
            let jobs = KafkaInspector::new(options)?.dead_letters(args[1].parse()?).await?;
            println!("{}", serde_json::to_string(&jobs.into_iter().map(|dead| serde_json::json!({"id":dead.job.id,"queue":dead.job.queue,"attempts":dead.job.attempts,"last_error":dead.error_code})).collect::<Vec<_>>())?);
        }
        None => {
            let config = KafkaRunnerConfig {
                retry: RetryPolicy::new(u32::try_from(number("WORKER_MAX_ATTEMPTS", 3)?)?, number("WORKER_BACKOFF_SECONDS", 30)?),
                handler_timeout: Duration::from_millis(number("WORKER_HANDLER_TIMEOUT_MS", 300000)?),
                drain_timeout: Duration::from_millis(number("WORKER_DRAIN_MS", 30000)?),
            };
            config.validate()?;
            let http = http_config()?;
            let worker = KafkaWorker::connect(options, &config).await?;
            println!("{} worker ready", WORKER_NAME);
            let report = signals::run(&worker, &ApplicationHandler, &config, http.as_ref(), shutdown()).await?;
            println!("worker stopped: completed={} retried={} dead_lettered={} abandoned={}", report.completed, report.retried, report.dead_lettered, report.abandoned);
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
