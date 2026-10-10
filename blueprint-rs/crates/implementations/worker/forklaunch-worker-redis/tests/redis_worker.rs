#![cfg(feature = "redis-client")]
use forklaunch_blueprint_core::BlueprintResult;
use forklaunch_core::persistence::TenantContext;
use forklaunch_worker_database::runner::{self, JobHandler, RunnerConfig};
use forklaunch_worker_interface::{
    AsyncWorkerQueue, FailureDisposition, Job, JobStatus, RetryPolicy,
};
use forklaunch_worker_redis::leased::RedisLeasedQueue;
use redis::aio::MultiplexedConnection;
use std::{
    collections::HashSet,
    fmt::Write,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

fn encoded(value: &str) -> String {
    value.bytes().fold(String::new(), |mut output, byte| {
        let _ = write!(&mut output, "{byte:02x}");
        output
    })
}
fn prefix(namespace: &str, tenant: &str, queue: &str) -> String {
    format!(
        "fl_worker:{{{}_{}}}:{}",
        encoded(namespace),
        encoded(tenant),
        encoded(queue)
    )
}
async fn clean(connection: &mut MultiplexedConnection, namespace: &str) {
    let mut cursor = 0_u64;
    loop {
        let (next, keys): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg(format!("fl_worker:{{{}_*", encoded(namespace)))
            .arg("COUNT")
            .arg(100)
            .query_async(connection)
            .await
            .unwrap();
        if !keys.is_empty() {
            let _: usize = redis::cmd("DEL")
                .arg(keys)
                .query_async(connection)
                .await
                .unwrap();
        }
        cursor = next;
        if cursor == 0 {
            break;
        }
    }
}

struct SlowHandler;
impl JobHandler for SlowHandler {
    async fn process(&self, _job: Job) -> BlueprintResult<()> {
        tokio::time::sleep(Duration::from_millis(1800)).await;
        Ok(())
    }
}

struct ErrorDrainHandler(Arc<AtomicUsize>);
impl JobHandler for ErrorDrainHandler {
    async fn process(&self, job: Job) -> BlueprintResult<()> {
        tokio::time::sleep(Duration::from_millis(if job.id == "fault-0" {
            1000
        } else {
            1500
        }))
        .await;
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
#[ignore = "requires isolated Redis and FORKLAUNCH_TEST_REDIS_URL"]
#[allow(clippy::too_many_lines)]
async fn redis_worker_leases_batches_retries_isolation_and_runner() {
    let url = std::env::var("FORKLAUNCH_TEST_REDIS_URL").expect("Redis URL required");
    let namespace = format!("worker-{}", uuid::Uuid::new_v4());
    let queue = RedisLeasedQueue::connect(
        &url,
        &namespace,
        TenantContext::new("tenant-a").unwrap(),
        RetryPolicy::new(2, 1),
    )
    .await
    .unwrap();
    let other = queue.for_tenant(TenantContext::new("tenant-a:other").unwrap());
    let mut admin = redis::Client::open(url.as_str())
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap();
    queue
        .enqueue_batch(vec![
            Job::new("z", "email", "{\"message\":\"héllo\"}"),
            Job::new("a", "email", "{}"),
            Job::new("z", "secondary", "{}"),
        ])
        .await
        .unwrap();
    other.enqueue(Job::new("z", "email", "null")).await.unwrap();
    assert_eq!(
        queue
            .peek("email", 10)
            .await
            .unwrap()
            .iter()
            .map(|job| job.id.as_str())
            .collect::<Vec<_>>(),
        ["z", "a"]
    );
    assert_eq!(queue.stats("email").await.unwrap().queued, 2);
    // A mixed-queue duplicate aborts the entire batch before the first write.
    assert!(
        queue
            .enqueue_batch(vec![
                Job::new("rollback", "secondary", "{}"),
                Job::new("z", "email", "{}")
            ])
            .await
            .is_err()
    );
    assert!(
        queue
            .inspect("secondary", "rollback")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        queue
            .enqueue_batch(vec![
                Job::new("duplicate", "email", "{}"),
                Job::new("duplicate", "email", "{}")
            ])
            .await
            .is_err()
    );
    assert!(queue.inspect("email", "duplicate").await.unwrap().is_none());

    let first = queue
        .claim("email", Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.job.id, "z");
    assert_eq!(first.job.attempts, 1);
    assert!(first.job.payload_json.contains("héllo"));
    assert!(!other.ack(&first).await.unwrap());
    assert!(!other.renew(&first, Duration::from_secs(30)).await.unwrap());
    assert_eq!(
        other.fail(&first, "forged").await.unwrap(),
        FailureDisposition::LeaseLost
    );
    assert!(queue.renew(&first, Duration::from_secs(60)).await.unwrap());
    let deadline = queue
        .inspect("email", "z")
        .await
        .unwrap()
        .unwrap()
        .lease_expires_at_unix_ms;
    assert!(queue.renew(&first, Duration::from_millis(1)).await.unwrap());
    assert_eq!(
        deadline,
        queue
            .inspect("email", "z")
            .await
            .unwrap()
            .unwrap()
            .lease_expires_at_unix_ms
    );
    let restarted = RedisLeasedQueue::connect(
        &url,
        &namespace,
        TenantContext::new("tenant-a").unwrap(),
        RetryPolicy::new(100, 0),
    )
    .await
    .unwrap();
    assert!(restarted.ack(&first).await.unwrap());
    assert!(!queue.ack(&first).await.unwrap());
    let second = queue
        .claim("email", Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        queue.fail(&second, "transient").await.unwrap(),
        FailureDisposition::RetryScheduled
    );
    assert!(queue.peek("email", 10).await.unwrap().is_empty());
    assert!(
        queue
            .claim("email", Duration::from_secs(30))
            .await
            .unwrap()
            .is_none()
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let retry = restarted
        .claim("email", Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retry.job.attempts, 2);
    assert_ne!(retry.token, second.token);
    assert!(!queue.ack(&second).await.unwrap());
    assert_eq!(
        restarted.fail(&retry, "permanent").await.unwrap(),
        FailureDisposition::DeadLetter
    );
    let failed = queue.inspect("email", "a").await.unwrap().unwrap();
    assert_eq!(failed.status, JobStatus::Failed);
    assert_eq!(failed.last_error.as_deref(), Some("permanent"));

    queue
        .enqueue(Job::new("crash", "crashes", "null"))
        .await
        .unwrap();
    let abandoned = queue
        .claim("crashes", Duration::from_millis(20))
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert!(!queue.ack(&abandoned).await.unwrap());
    assert!(
        !queue
            .renew(&abandoned, Duration::from_secs(1))
            .await
            .unwrap()
    );
    let recovered = queue
        .claim("crashes", Duration::from_millis(20))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.job.attempts, 2);
    assert_ne!(recovered.token, abandoned.token);
    assert_eq!(
        queue.fail(&abandoned, "obsolete").await.unwrap(),
        FailureDisposition::LeaseLost
    );
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert!(
        queue
            .claim("crashes", Duration::from_secs(30))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        queue
            .inspect("crashes", "crash")
            .await
            .unwrap()
            .unwrap()
            .status,
        JobStatus::Failed
    );

    queue
        .enqueue_batch(
            (0..50)
                .map(|id| Job::new(format!("job-{id}"), "concurrent", "{}"))
                .collect(),
        )
        .await
        .unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..10 {
        let consumer = queue.clone();
        tasks.spawn(async move {
            let mut ids = Vec::new();
            while let Some(lease) = consumer
                .claim("concurrent", Duration::from_secs(30))
                .await
                .unwrap()
            {
                assert_eq!(lease.job.attempts, 1);
                assert!(consumer.ack(&lease).await.unwrap());
                ids.push(lease.job.id);
            }
            ids
        });
    }
    let mut ids = HashSet::new();
    while let Some(result) = tasks.join_next().await {
        for id in result.unwrap() {
            assert!(ids.insert(id), "duplicate live delivery");
        }
    }
    assert_eq!(ids.len(), 50);
    assert_eq!(queue.stats("concurrent").await.unwrap().processing, 0);

    // Wrong key types are rejected before writes, including another queue in
    // the same batch. Script errors must not leave partially inserted jobs.
    let broken = prefix(&namespace, "tenant-a", "broken");
    let _: () = redis::cmd("SET")
        .arg(format!("{broken}:ready"))
        .arg("wrong-type")
        .query_async(&mut admin)
        .await
        .unwrap();
    assert!(
        queue
            .enqueue_batch(vec![
                Job::new("rollback", "healthy", "{}"),
                Job::new("broken", "broken", "{}")
            ])
            .await
            .is_err()
    );
    assert!(
        queue
            .inspect("healthy", "rollback")
            .await
            .unwrap()
            .is_none()
    );
    let absent: Option<String> = redis::cmd("HGET")
        .arg(format!("{broken}:jobs"))
        .arg("broken")
        .query_async(&mut admin)
        .await
        .unwrap();
    assert!(absent.is_none());
    let _: usize = redis::cmd("DEL")
        .arg(format!("{broken}:ready"))
        .query_async(&mut admin)
        .await
        .unwrap();
    assert_eq!(queue.stats("broken").await.unwrap().queued, 0);
    assert!(
        queue
            .enqueue(Job::new("bad", "email", "not-json"))
            .await
            .is_err()
    );
    assert!(queue.peek("email", 0).await.is_err());
    assert!(queue.claim("email", Duration::ZERO).await.is_err());
    assert_eq!(other.stats("email").await.unwrap().queued, 1);

    queue
        .enqueue(Job::new("runner", "runner", "{}"))
        .await
        .unwrap();
    let config = RunnerConfig {
        queue: "runner".into(),
        concurrency: 1,
        poll_interval: Duration::from_millis(25),
        lease_duration: Duration::from_millis(1200),
        renew_interval: Duration::from_millis(300),
        drain_timeout: Duration::from_secs(3),
    };
    let (report, ()) = tokio::join!(
        runner::run(&queue, &SlowHandler, &config, async {
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }),
        async {
            tokio::time::sleep(Duration::from_millis(1400)).await;
            assert!(
                queue
                    .claim("runner", Duration::from_secs(30))
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    );
    assert_eq!(report.unwrap().completed, 1);
    assert_eq!(
        queue
            .inspect("runner", "runner")
            .await
            .unwrap()
            .unwrap()
            .job
            .attempts,
        1
    );
    queue
        .enqueue_batch(
            (0..5)
                .map(|index| Job::new(format!("parallel-{index}"), "parallel", "{}"))
                .collect(),
        )
        .await
        .unwrap();
    let config = RunnerConfig {
        queue: "parallel".into(),
        concurrency: 4,
        ..config
    };
    let (report, ()) = tokio::join!(
        runner::run(&queue, &SlowHandler, &config, async {
            tokio::time::sleep(Duration::from_millis(400)).await;
        }),
        async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let stats = queue.stats("parallel").await.unwrap();
            assert_eq!(stats.processing, 4);
            assert_eq!(stats.queued, 1);
        }
    );
    assert_eq!(report.unwrap().completed, 4);
    let pending = queue
        .inspect("parallel", "parallel-4")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, JobStatus::Queued);
    assert_eq!(pending.job.attempts, 0);
    for index in 0..4 {
        let completed = queue
            .inspect("parallel", &format!("parallel-{index}"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(completed.status, JobStatus::Completed);
        assert_eq!(completed.job.attempts, 1);
    }
    // A storage error in one consumer must drain the others, not drop their
    // handler futures immediately. Corrupt only this test's isolated queue.
    queue
        .enqueue_batch(
            (0..4)
                .map(|index| Job::new(format!("fault-{index}"), "fault", "{}"))
                .collect(),
        )
        .await
        .unwrap();
    let completions = Arc::new(AtomicUsize::new(0));
    let handler = ErrorDrainHandler(Arc::clone(&completions));
    let config = RunnerConfig {
        concurrency: 4,
        ..RunnerConfig::new("fault")
    };
    let ready_key = format!("{}:ready", prefix(&namespace, "tenant-a", "fault"));
    let (result, ()) = tokio::join!(
        tokio::time::timeout(
            Duration::from_secs(5),
            runner::run(&queue, &handler, &config, std::future::pending())
        ),
        async {
            tokio::time::timeout(Duration::from_secs(2), async {
                while queue.stats("fault").await.unwrap().processing != 4 {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            let _: () = redis::cmd("SET")
                .arg(&ready_key)
                .arg("wrong-type")
                .query_async(&mut admin)
                .await
                .unwrap();
        }
    );
    assert_eq!(result.unwrap().unwrap_err().code, "worker_storage_error");
    assert_eq!(completions.load(Ordering::SeqCst), 4);
    let _: usize = redis::cmd("DEL")
        .arg(&ready_key)
        .query_async(&mut admin)
        .await
        .unwrap();
    let error = RedisLeasedQueue::connect(
        "secret-invalid-url",
        &namespace,
        TenantContext::new("tenant-a").unwrap(),
        RetryPolicy::default(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.message, "Worker storage operation failed");
    clean(&mut admin, &namespace).await;
}
