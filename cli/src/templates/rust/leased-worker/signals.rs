// Optional scoped Pub/Sub control. Disconnects drain; restarts begin running.
use std::{env, future::Future, sync::Arc};
use forklaunch_core::persistence::TenantContext;
use forklaunch_worker_database::{
    control::{WorkerControl, forward_signals},
    http::{self, HttpConfig, SignalPublisher},
    retention::{RetainedRunReport, RetentionPolicy},
    runner::{JobHandler, RunnerConfig},
};
use forklaunch_worker_interface::{AsyncWorkerAdmin, DistributedSignal};
use forklaunch_worker_redis::signals::{RedisSignalBus, RedisSignalPublisher, SignalOptions};

type Error = Box<dyn std::error::Error>;

fn configuration() -> Result<Option<(String, String, TenantContext, String)>, Error> {
    let url = match env::var("WORKER_SIGNAL_REDIS_URL") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let namespace = env::var("WORKER_SIGNAL_NAMESPACE")
        .map_err(|_| "WORKER_SIGNAL_NAMESPACE is required with WORKER_SIGNAL_REDIS_URL")?;
    let tenant = TenantContext::new(env::var("TENANT_ID").map_err(|_| "TENANT_ID is required")?)?;
    let queue = env::var("WORKER_QUEUE").unwrap_or_else(|_| super::WORKER_NAME.to_owned());
    Ok(Some((url, namespace, tenant, queue)))
}

pub async fn publish(action: &str) -> Result<(), Error> {
    let signal = match action {
        "pause" => DistributedSignal::Pause,
        "resume" => DistributedSignal::Resume,
        "drain" => DistributedSignal::Drain,
        _ => return Err("Expected --signal pause, resume, or drain".into()),
    };
    let (url, namespace, tenant, queue) = configuration()?
        .ok_or("--signal requires WORKER_SIGNAL_REDIS_URL and WORKER_SIGNAL_NAMESPACE")?;
    let publisher = RedisSignalPublisher::connect(&url, &namespace, &tenant, &queue, &SignalOptions::default()).await?;
    let subscribers = publisher.broadcast(signal).await?;
    println!("{}", serde_json::json!({"subscribers": subscribers}));
    Ok(())
}

pub async fn run<Q, H, F>(
    queue: &Q,
    handler: &H,
    config: &RunnerConfig,
    policy: Option<&RetentionPolicy>,
    http_config: Option<&HttpConfig>,
    shutdown: F,
) -> Result<RetainedRunReport, Error>
where
    Q: AsyncWorkerAdmin,
    H: JobHandler,
    F: Future<Output = ()> + Send,
{
    let Some((url, namespace, tenant, queue_name)) = configuration()? else {
        return Ok(http::run(queue, handler, config, policy, http_config, shutdown).await?);
    };
    let mut bus = RedisSignalBus::connect(&url, &namespace, &tenant, &queue_name, SignalOptions::default()).await?;
    let publisher = bus.publisher();
    let publish: SignalPublisher = Arc::new(move |signal| {
        let publisher = publisher.clone();
        Box::pin(async move { publisher.broadcast(signal).await })
    });
    let control = WorkerControl::new();
    let work = http::run_with_publisher(queue, handler, config, policy, http_config, &control, Some(publish), shutdown);
    let forwarding = forward_signals(&mut bus, &control, std::future::pending());
    tokio::pin!(work, forwarding);
    tokio::select! {
        result = &mut work => Ok(result?),
        result = &mut forwarding => {
            // Forwarding drains on failure. Await all active handlers before exiting.
            let work_result = work.await;
            result?;
            Ok(work_result?)
        }
    }
}
