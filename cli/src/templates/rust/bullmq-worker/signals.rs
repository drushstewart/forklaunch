// Optional scoped Pub/Sub control. Disconnects drain; restarts begin running.
use std::{env, future::Future, sync::Arc};
use forklaunch_core::persistence::TenantContext;
use forklaunch_worker_database::{
    control::{WorkerControl, forward_signals},
    http::{self, HttpConfig, SignalPublisher},
};
use forklaunch_worker_interface::{DistributedSignal};
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

pub async fn run<F>(worker: &forklaunch_worker_redis::bullmq::BullMqConsumer, http_config: Option<&HttpConfig>, shutdown: F) -> Result<(), Error>
where F: Future<Output = ()> + Send {
    let mut bus = if let Some((url, namespace, tenant, queue)) = configuration()? {
        Some(RedisSignalBus::connect(&url, &namespace, &tenant, &queue, SignalOptions::default()).await?)
    } else { None };
    let publish: Option<SignalPublisher> = bus.as_ref().map(|bus| {
        let publisher = bus.publisher();
        let callback: SignalPublisher = Arc::new(move |signal| {
            let publisher = publisher.clone();
            Box::pin(async move { publisher.broadcast(signal).await })
        });
        callback
    });
    let control = WorkerControl::new();
    let consuming = worker.run_until(async {
        loop {
            let state = control.state();
            match state {
                forklaunch_worker_database::control::WorkerState::Running => worker.inner().resume(),
                forklaunch_worker_database::control::WorkerState::Paused => worker.inner().pause(),
                forklaunch_worker_database::control::WorkerState::Draining => return,
            }
            control.changed(state).await;
        }
    });
    let work = http::serve_controlled(&control, http_config, publish, consuming, shutdown);
    let forwarding = async {
        if let Some(bus) = bus.as_mut() { forward_signals(bus, &control, std::future::pending()).await }
        else { std::future::pending().await }
    };
    tokio::pin!(work, forwarding);
    tokio::select! {
        result = &mut work => Ok(result?),
        result = &mut forwarding => {
            let work_result = work.await;
            result?;
            Ok(work_result?)
        }
    }
}
