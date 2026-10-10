//! Polling and graceful drain for asynchronous, leased worker queues.
use crate::control::{WorkerControl, WorkerState};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use forklaunch_worker_interface::{AsyncWorkerQueue, FailureDisposition, Job, JobLease};
use futures_util::{StreamExt, stream::FuturesUnordered};
use std::{future::Future, time::Duration};
use tokio::sync::watch;
use tokio::time::{Instant, MissedTickBehavior, interval_at, timeout};

/// Application handlers must be idempotent and keep all work inside this future
/// so cancellation stops processing when a lease is lost or drain times out.
pub trait JobHandler: Send + Sync {
    /// # Errors
    /// Returns an application failure; the runner records its code and retries.
    fn process(&self, job: Job) -> impl Future<Output = BlueprintResult<()>> + Send;
}

#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub queue: String,
    /// Maximum in-flight deliveries in this process (1..=256).
    pub concurrency: usize,
    pub poll_interval: Duration,
    pub lease_duration: Duration,
    pub renew_interval: Duration,
    pub drain_timeout: Duration,
}

impl RunnerConfig {
    #[must_use]
    pub fn new(queue: impl Into<String>) -> Self {
        Self {
            queue: queue.into(),
            concurrency: 1,
            poll_interval: Duration::from_millis(250),
            lease_duration: Duration::from_secs(30),
            renew_interval: Duration::from_secs(10),
            drain_timeout: Duration::from_secs(30),
        }
    }

    /// # Errors
    /// Rejects invalid queue names, time bounds, and concurrency limits.
    pub fn validate(&self) -> BlueprintResult<()> {
        if !(1..=256).contains(&self.concurrency)
            || self.queue.trim().is_empty()
            || self.queue.len() > 256
            || self.queue.chars().any(char::is_control)
            || self.poll_interval.is_zero()
            || self.poll_interval > Duration::from_secs(60)
            || self.lease_duration < Duration::from_millis(3)
            || self.lease_duration > Duration::from_secs(86400)
            || self.renew_interval < Duration::from_millis(1)
            || self.renew_interval > self.lease_duration / 3
            || self.drain_timeout.is_zero()
            || self.drain_timeout > Duration::from_secs(86400)
        {
            return Err(BlueprintError::new(
                "worker_invalid_config",
                "Invalid worker runner configuration",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RunReport {
    pub completed: u64,
    pub retried: u64,
    pub dead_lettered: u64,
    pub lease_lost: u64,
    pub abandoned: u64,
}

#[derive(Clone, Copy)]
enum Outcome {
    Completed,
    Retried,
    DeadLettered,
    LeaseLost,
}

impl RunReport {
    fn record(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Completed => self.completed += 1,
            Outcome::Retried => self.retried += 1,
            Outcome::DeadLettered => self.dead_lettered += 1,
            Outcome::LeaseLost => self.lease_lost += 1,
        }
    }
}

fn operation_timeout() -> BlueprintError {
    BlueprintError::new(
        "worker_operation_timeout",
        "Worker queue operation timed out",
    )
}

/// Runs up to the configured number of concurrent deliveries, renewing each
/// lease while its handler runs. Shutdown stops new claims and drains all active
/// handlers while continuing renewal. A drain timeout cancels the handler and
/// leaves its delivery recoverable after lease expiry. A queue error stops all
/// consumers and drains their active work before returning the first error.
///
/// # Errors
/// Returns an error for invalid configuration or queue failure/timeout. Handler
/// errors instead schedule retries or retain a failed job after exhaustion.
pub async fn run<Q, H, F>(
    queue: &Q,
    handler: &H,
    config: &RunnerConfig,
    shutdown: F,
) -> BlueprintResult<RunReport>
where
    Q: AsyncWorkerQueue,
    H: JobHandler,
    F: Future<Output = ()> + Send,
{
    run_controlled(queue, handler, config, &WorkerControl::new(), shutdown).await
}

/// Runs with process-local pause/resume/drain control. Pause preserves active
/// handlers and claims already submitted to storage. Drain cannot be reversed.
/// # Errors
/// Returns invalid configuration or queue failures/timeouts after active drain.
pub async fn run_controlled<Q, H, F>(
    queue: &Q,
    handler: &H,
    config: &RunnerConfig,
    control: &WorkerControl,
    shutdown: F,
) -> BlueprintResult<RunReport>
where
    Q: AsyncWorkerQueue,
    H: JobHandler,
    F: Future<Output = ()> + Send,
{
    config.validate()?;
    let (stop, receiver) = watch::channel(false);
    let mut consumers: FuturesUnordered<_> = (0..config.concurrency)
        .map(|_| {
            let mut receiver = receiver.clone();
            run_single(queue, handler, config, control, async move {
                if !*receiver.borrow() {
                    let _ = receiver.changed().await;
                }
            })
        })
        .collect();
    let shutdown = async {
        tokio::select! { () = shutdown => {}, () = control.draining() => {} }
    };
    tokio::pin!(shutdown);
    let mut stopping = false;
    let mut first_error = None;
    let mut report = RunReport::default();
    loop {
        tokio::select! {
            biased;
            () = &mut shutdown, if !stopping => {
                stopping = true;
                control.drain();
                stop.send_replace(true);
            }
            result = consumers.next() => {
                let Some(result) = result else { break; };
                match result {
                    Ok(partial) => {
                        report.completed += partial.completed;
                        report.retried += partial.retried;
                        report.dead_lettered += partial.dead_lettered;
                        report.lease_lost += partial.lease_lost;
                        report.abandoned += partial.abandoned;
                    }
                    Err(error) => {
                        if first_error.is_none() { first_error = Some(error); }
                        stopping = true;
                        control.drain();
                        stop.send_replace(true);
                    }
                }
            }
        }
    }
    first_error.map_or(Ok(report), Err)
}

async fn run_single<Q, H, F>(
    queue: &Q,
    handler: &H,
    config: &RunnerConfig,
    control: &WorkerControl,
    shutdown: F,
) -> BlueprintResult<RunReport>
where
    Q: AsyncWorkerQueue,
    H: JobHandler,
    F: Future<Output = ()> + Send,
{
    tokio::pin!(shutdown);
    let mut report = RunReport::default();
    let mut state = control.subscribe();
    loop {
        loop {
            let current = *state.borrow_and_update();
            match current {
                WorkerState::Running => break,
                WorkerState::Draining => return Ok(report),
                WorkerState::Paused => {
                    tokio::select! {
                        biased;
                        () = &mut shutdown => return Ok(report),
                        result = state.changed() => { if result.is_err() { return Ok(report); } }
                    }
                }
            }
        }
        let lease = tokio::select! {
            biased;
            () = &mut shutdown => return Ok(report),
            result = timeout(config.renew_interval, queue.claim(&config.queue, config.lease_duration)) => {
                result.map_err(|_| operation_timeout())??
            }
        };
        let Some(lease) = lease else {
            tokio::select! {
                biased;
                () = &mut shutdown => return Ok(report),
                _ = state.changed() => {},
                () = tokio::time::sleep(config.poll_interval) => {},
            }
            continue;
        };
        let processing = process(queue, handler, config, &lease);
        tokio::pin!(processing);
        tokio::select! {
            biased;
            () = &mut shutdown => {
                match timeout(config.drain_timeout, &mut processing).await {
                    Ok(result) => report.record(result?),
                    Err(_) => report.abandoned += 1,
                }
                return Ok(report);
            }
            result = &mut processing => report.record(result?),
        }
    }
}

async fn process<Q: AsyncWorkerQueue, H: JobHandler>(
    queue: &Q,
    handler: &H,
    config: &RunnerConfig,
    lease: &JobLease,
) -> BlueprintResult<Outcome> {
    let work = handler.process(lease.job.clone());
    tokio::pin!(work);
    let mut renewal = interval_at(
        Instant::now() + config.renew_interval,
        config.renew_interval,
    );
    renewal.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let result = loop {
        tokio::select! {
            biased;
            result = &mut work => break result,
            _ = renewal.tick() => {
                let renewed = timeout(config.renew_interval, queue.renew(lease, config.lease_duration))
                    .await.map_err(|_| operation_timeout())??;
                if !renewed { return Ok(Outcome::LeaseLost); }
            }
        }
    };
    match result {
        Ok(()) => {
            let acknowledged = timeout(config.renew_interval, queue.ack(lease))
                .await
                .map_err(|_| operation_timeout())??;
            Ok(if acknowledged {
                Outcome::Completed
            } else {
                Outcome::LeaseLost
            })
        }
        Err(error) => {
            // Application error messages can contain credentials or payloads.
            // Persist a bounded code only; the handler owns detailed reporting.
            let reason: String = error
                .code
                .chars()
                .filter(|ch| !ch.is_control())
                .take(256)
                .collect();
            let disposition = timeout(config.renew_interval, queue.fail(lease, &reason))
                .await
                .map_err(|_| operation_timeout())??;
            Ok(match disposition {
                FailureDisposition::RetryScheduled => Outcome::Retried,
                FailureDisposition::DeadLetter => Outcome::DeadLettered,
                FailureDisposition::LeaseLost => Outcome::LeaseLost,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_timing_that_cannot_renew_safely() {
        let mut config = RunnerConfig::new("email");
        assert!(config.validate().is_ok());
        config.renew_interval = config.lease_duration;
        assert!(config.validate().is_err());
        config = RunnerConfig::new("email");
        config.drain_timeout = Duration::ZERO;
        assert!(config.validate().is_err());
        config = RunnerConfig::new("\n");
        assert!(config.validate().is_err());
        config = RunnerConfig::new("email");
        config.concurrency = 0;
        assert!(config.validate().is_err());
        config.concurrency = 257;
        assert!(config.validate().is_err());
        config = RunnerConfig::new("email");
        config.poll_interval = Duration::ZERO;
        assert!(config.validate().is_err());
    }
}
