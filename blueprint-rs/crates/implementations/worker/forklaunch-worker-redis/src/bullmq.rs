//! Official `BullMQ` wire protocol, separate from the native fenced-lease storage layout.
use crate::tenant_isolation::RedisKeyScope;
pub use ::bullmq as protocol;
use ::bullmq::{JobOptions, Queue, QueueOptions, Worker, WorkerOptions};
use ::bullmq::{types::BackoffStrategy, worker::WorkerEvent};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use std::{
    future::{Future, IntoFuture},
    time::Duration,
};

/// Configuration deliberately omits Debug, because connection options contain credentials.
pub struct BullMqOptions {
    pub queue: QueueOptions,
    pub worker: WorkerOptions,
    pub retries: u32,
    pub backoff: BackoffStrategy,
    pub operation_timeout: Duration,
    pub close_timeout: Duration,
}
impl BullMqOptions {
    #[must_use]
    pub fn new(url: &str) -> Self {
        Self {
            queue: QueueOptions::default()
                .connection(protocol::options::RedisConnectionOptions::default().url(url)),
            worker: WorkerOptions::default(),
            retries: 3,
            backoff: BackoffStrategy::Exponential(5000),
            operation_timeout: Duration::from_secs(5),
            close_timeout: Duration::from_secs(30),
        }
    }
    fn scoped(mut self) -> BlueprintResult<Self> {
        if self.retries == 0
            || self.operation_timeout.is_zero()
            || self.operation_timeout > Duration::from_secs(60)
            || self.close_timeout.is_zero()
            || self.close_timeout > Duration::from_secs(86400)
        {
            return Err(invalid_input());
        }
        if let Some(prefix) = RedisKeyScope::from_env()?.prefix() {
            prefix.clone_into(&mut self.queue.prefix);
        }
        // TypeScript passes the same queue connection/prefix to its producer and worker.
        self.worker.connection = self.queue.connection.clone();
        self.worker.prefix = self.queue.prefix.clone();
        self.worker.autorun = false;
        Ok(self)
    }
    fn job_options(&self) -> JobOptions {
        JobOptions::default()
            .attempts(self.retries)
            .backoff(self.backoff.clone())
    }
}

pub struct BullMqProducer {
    queue: Queue,
    job_options: JobOptions,
    timeout: Duration,
}
impl BullMqProducer {
    /// # Errors
    /// Rejects invalid configuration and returns sanitized connection/ACL errors.
    pub async fn connect(name: &str, options: BullMqOptions) -> BlueprintResult<Self> {
        let options = options.scoped()?;
        let job_options = options.job_options();
        let queue = bounded(
            options.operation_timeout,
            Queue::with_options(name, options.queue),
        )
        .await?;
        Ok(Self {
            queue,
            job_options,
            timeout: options.operation_timeout,
        })
    }

    /// Uses the event's id as `BullMQ`'s job name, leaving job IDs auto-generated like TypeScript.
    /// # Errors
    /// Rejects absent event IDs and propagates sanitized serialization/Redis errors.
    pub async fn enqueue_job(&self, event: Value) -> BlueprintResult<String> {
        let name = event_name(&event)?.to_owned();
        bounded(
            self.timeout,
            self.queue
                .add(&name, event)
                .options(self.job_options.clone())
                .into_future(),
        )
        .await
        .map(|job| job.id().to_owned())
    }

    /// Validates all event IDs before submitting the official bulk operation.
    /// # Errors
    /// Propagates validation and sanitized backend failures; backend writes may be partial.
    pub async fn enqueue_batch_jobs(&self, events: Vec<Value>) -> BlueprintResult<Vec<String>> {
        let jobs = events
            .into_iter()
            .map(|event| {
                Ok(protocol::queue::BulkJob::with_options(
                    event_name(&event)?.to_owned(),
                    event,
                    self.job_options.clone(),
                ))
            })
            .collect::<BlueprintResult<Vec<_>>>()?;
        bounded(self.timeout, self.queue.add_bulk(jobs))
            .await
            .map(|jobs| jobs.iter().map(|job| job.id().to_owned()).collect())
    }

    /// Lists waiting and active event bodies, matching the TypeScript consumer's peek surface.
    /// # Errors
    /// Returns sanitized backend failures.
    pub async fn peek_events(&self) -> BlueprintResult<Vec<Value>> {
        bounded(
            self.timeout,
            self.queue.get_jobs(&["waiting", "active"], 0, -1, false),
        )
        .await
        .map(|jobs| jobs.iter().map(|job| job.data().clone()).collect())
    }

    /// Advanced official options/administration remain available through the upstream API.
    #[must_use]
    pub fn inner(&self) -> &Queue {
        &self.queue
    }

    pub async fn close(&self) {
        self.queue.close().await;
    }
}

pub struct BullMqConsumer {
    queue: Queue,
    timeout: Duration,
    worker: Worker,
    close_timeout: Duration,
}
impl BullMqConsumer {
    /// Constructs an idle consumer; `run_until` starts processing and drains on shutdown.
    /// # Errors
    /// Returns sanitized configuration, connection and ACL errors.
    pub async fn connect<P: protocol::worker::IntoProcessor>(
        name: &str,
        processor: P,
        options: BullMqOptions,
    ) -> BlueprintResult<Self> {
        let options = options.scoped()?;
        let queue = bounded(
            options.operation_timeout,
            Queue::with_options(name, options.queue),
        )
        .await?;
        let worker = bounded(
            options.operation_timeout,
            Worker::with_options(name, processor, options.worker),
        )
        .await;
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                queue.close().await;
                return Err(error);
            }
        };
        Ok(Self {
            queue,
            timeout: options.operation_timeout,
            worker,
            close_timeout: options.close_timeout,
        })
    }

    /// Stops on the first storage error, drains active work and emits one sanitized error.
    /// Job failures retain official `BullMQ` retries/backoff/retention behavior.
    /// # Errors
    /// Returns sanitized startup, storage and bounded shutdown errors.
    pub async fn run_until<F: Future<Output = ()>>(&self, shutdown: F) -> BlueprintResult<()> {
        self.run_until_with_failure_handler(shutdown, |_, _| {})
            .await
    }

    /// Reports every failed processing attempt, including retries, with its original
    /// event even when `remove_on_fail` deletes the persisted job. Error details are
    /// redacted; observers can enqueue diagnostics for asynchronous handling.
    /// The pinned outcome-data hook also preserves preprocessor failure events
    /// when stalled/start limits or deferred failures remove their job.
    /// # Errors
    /// Returns sanitized startup, storage and bounded shutdown errors.
    pub async fn run_until_with_failure_handler<F, H>(
        &self,
        shutdown: F,
        failure_handler: H,
    ) -> BlueprintResult<()>
    where
        F: Future<Output = ()>,
        H: Fn(Value, BlueprintError),
    {
        self.worker
            .run()
            .await
            .map_err(|error| protocol_error(&error))?;
        tokio::pin!(shutdown);
        let result = loop {
            tokio::select! {
                biased;
                event=self.worker.next_event()=>match event {
                    Some(WorkerEvent::Error(error))=>break Err(message_error(&error)),
                    Some(WorkerEvent::Closed)|None=>break Ok(()),
                    Some(event)=>Self::report_event(event, &failure_handler),
                },
                ()=&mut shutdown=>break Ok(()),
            }
        };
        let closing = self.close();
        tokio::pin!(closing);
        let closed = loop {
            tokio::select! {
                biased;
                event = self.worker.next_event() => match event {
                    Some(event) => Self::report_event(event, &failure_handler),
                    None => break closing.await,
                },
                result = &mut closing => break result,
            }
        };
        result.and(closed)
    }

    fn report_event<H: Fn(Value, BlueprintError)>(event: WorkerEvent, failure_handler: &H) {
        if let WorkerEvent::FailedWithData { data, .. } = event {
            failure_handler(
                data,
                BlueprintError::new("worker_job_failed", "BullMQ processing attempt failed"),
            );
        }
    }

    /// Lists waiting/active event bodies without starting this consumer.
    /// # Errors
    /// Returns sanitized storage and timeout errors.
    pub async fn peek_events(&self) -> BlueprintResult<Vec<Value>> {
        bounded(
            self.timeout,
            self.queue.get_jobs(&["waiting", "active"], 0, -1, false),
        )
        .await
        .map(|jobs| jobs.iter().map(|job| job.data().clone()).collect())
    }

    #[must_use]
    pub fn inner(&self) -> &Worker {
        &self.worker
    }

    /// # Errors
    /// Returns sanitized errors if graceful drain exceeds its configured bound.
    pub async fn close(&self) -> BlueprintResult<()> {
        let milliseconds =
            u64::try_from(self.close_timeout.as_millis()).map_err(|_| invalid_input())?;
        let worker_result = bounded(
            self.close_timeout + Duration::from_secs(1),
            self.worker.close(milliseconds),
        )
        .await;
        let queue_result = bounded(self.timeout, async {
            self.queue.close().await;
            Ok(())
        })
        .await;
        worker_result.and(queue_result)
    }
}

fn event_name(event: &Value) -> BlueprintResult<&str> {
    event
        .get("id")
        .and_then(Value::as_str)
        .filter(|name| {
            !name.trim().is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
        })
        .ok_or_else(invalid_input)
}
fn invalid_input() -> BlueprintError {
    BlueprintError::new(
        "worker_invalid_input",
        "Invalid BullMQ worker configuration or event",
    )
}
fn message_error(message: &str) -> BlueprintError {
    if message
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|part| part.eq_ignore_ascii_case("NOPERM"))
    {
        BlueprintError::new(
            "worker_tenant_isolation_error",
            "Redis ACL denied worker operation; verify application prefix and permissions",
        )
    } else {
        BlueprintError::new(
            "worker_storage_error",
            "BullMQ worker storage operation failed",
        )
    }
}
fn protocol_error(error: &protocol::Error) -> BlueprintError {
    message_error(&error.to_string())
}
async fn bounded<T>(
    timeout: Duration,
    future: impl Future<Output = protocol::Result<T>>,
) -> BlueprintResult<T> {
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| BlueprintError::new("worker_storage_timeout", "BullMQ operation timed out"))?
        .map_err(|error| protocol_error(&error))
}
