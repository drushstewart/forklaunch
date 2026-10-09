#![cfg(feature = "postgres")]
use forklaunch_core::persistence::TenantContext;
use forklaunch_iam_base::postgres::PostgresIamService;
use forklaunch_worker_database::postgres::PostgresWorkerQueue;
use forklaunch_worker_interface::{
    AsyncWorkerQueue, FailureDisposition, Job, JobStatus, RetryPolicy,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{collections::HashSet, time::Duration};

struct Fixture {
    owner: PgPool,
    pool: PgPool,
    role: String,
    queue: PostgresWorkerQueue,
}
impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("FORKLAUNCH_TEST_POSTGRES_URL").expect("PostgreSQL URL required");
        let owner = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .unwrap();
        // Both components must migrate repeatedly in the same database without
        // sharing SQLx's migration ledger or leaking a changed search_path.
        PostgresIamService::migrate(&owner).await.unwrap();
        let before: (String,) = sqlx::query_as("SHOW search_path")
            .fetch_one(&owner)
            .await
            .unwrap();
        PostgresWorkerQueue::migrate(&owner).await.unwrap();
        PostgresWorkerQueue::migrate(&owner).await.unwrap();
        PostgresIamService::migrate(&owner).await.unwrap();
        let after: (String,) = sqlx::query_as("SHOW search_path")
            .fetch_one(&owner)
            .await
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(
            PostgresWorkerQueue::new(
                owner.clone(),
                TenantContext::new("unsafe").unwrap(),
                RetryPolicy::default()
            )
            .await
            .err()
            .unwrap()
            .code,
            "worker_unsafe_database_role"
        );
        let role = format!(
            "fl_jobs_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        for statement in [
            format!("CREATE ROLE {role} NOLOGIN NOSUPERUSER NOBYPASSRLS"),
            format!("GRANT USAGE ON SCHEMA fl_worker TO {role}"),
            format!("GRANT SELECT,INSERT,UPDATE,DELETE ON fl_worker.jobs TO {role}"),
            format!("GRANT USAGE ON ALL SEQUENCES IN SCHEMA fl_worker TO {role}"),
        ] {
            sqlx::query(&statement).execute(&owner).await.unwrap();
        }
        let set_role = format!("SET ROLE {role}");
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .after_connect(move |connection, _| {
                let statement = set_role.clone();
                Box::pin(async move {
                    sqlx::query(&statement).execute(connection).await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .unwrap();
        let queue = PostgresWorkerQueue::new(
            pool.clone(),
            TenantContext::new(&role).unwrap(),
            RetryPolicy::new(2, 1),
        )
        .await
        .unwrap();
        Self {
            owner,
            pool,
            role,
            queue,
        }
    }
    async fn close(self) {
        self.pool.close().await;
        sqlx::query("DELETE FROM fl_worker.jobs WHERE tenant_id=$1 OR tenant_id=$2")
            .bind(&self.role)
            .bind(format!("{}-b", self.role))
            .execute(&self.owner)
            .await
            .unwrap();
        sqlx::query(&format!("DROP OWNED BY {}", self.role))
            .execute(&self.owner)
            .await
            .unwrap();
        sqlx::query(&format!("DROP ROLE {}", self.role))
            .execute(&self.owner)
            .await
            .unwrap();
        self.owner.close().await;
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
#[ignore = "requires isolated PostgreSQL and a schema-owner test URL"]
async fn postgres_worker_leases_retries_isolation_and_concurrency() {
    let fixture = Fixture::new().await;
    let queue = &fixture.queue;
    let other = queue.for_tenant(TenantContext::new(format!("{}-b", fixture.role)).unwrap());
    queue
        .enqueue_batch(vec![
            Job::new("first", "email", "{\"message\":\"héllo\"}"),
            Job::new("second", "email", "{}"),
        ])
        .await
        .unwrap();
    other
        .enqueue(Job::new("first", "email", "{\"tenant\":\"other\"}"))
        .await
        .unwrap();
    let peek = queue.peek("email", 10).await.unwrap();
    assert_eq!(
        peek.iter().map(|job| job.id.as_str()).collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert_eq!(queue.stats("email").await.unwrap().queued, 2);
    let first = queue
        .claim("email", Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.job.id, "first");
    assert_eq!(first.job.attempts, 1);
    assert!(!other.ack(&first).await.unwrap());
    assert_eq!(
        other.fail(&first, "forged").await.unwrap(),
        FailureDisposition::LeaseLost
    );
    assert!(!other.renew(&first, Duration::from_secs(30)).await.unwrap());
    let snapshot = queue.inspect("email", "first").await.unwrap().unwrap();
    assert_eq!(snapshot.status, JobStatus::Processing);
    assert!(queue.renew(&first, Duration::from_secs(60)).await.unwrap());
    let renewed = queue.inspect("email", "first").await.unwrap().unwrap();
    assert!(renewed.lease_expires_at_unix_ms > snapshot.lease_expires_at_unix_ms);
    assert!(queue.renew(&first, Duration::from_millis(1)).await.unwrap());
    assert_eq!(
        queue
            .inspect("email", "first")
            .await
            .unwrap()
            .unwrap()
            .lease_expires_at_unix_ms,
        renewed.lease_expires_at_unix_ms
    );
    // New consumer handle observes the persistent delivery and can acknowledge.
    let restarted = PostgresWorkerQueue::new(
        fixture.pool.clone(),
        TenantContext::new(&fixture.role).unwrap(),
        RetryPolicy::new(100, 0),
    )
    .await
    .unwrap();
    assert!(restarted.ack(&first).await.unwrap());
    assert!(!queue.ack(&first).await.unwrap());
    assert_eq!(
        queue.fail(&first, "late").await.unwrap(),
        FailureDisposition::LeaseLost
    );
    assert_eq!(
        queue
            .inspect("email", "first")
            .await
            .unwrap()
            .unwrap()
            .status,
        JobStatus::Completed
    );
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
    assert_eq!(retry.job.id, "second");
    assert_eq!(retry.job.attempts, 2);
    assert_ne!(retry.token, second.token);
    assert!(!queue.ack(&second).await.unwrap());
    assert_eq!(
        restarted.fail(&retry, "permanent").await.unwrap(),
        FailureDisposition::DeadLetter
    );
    let failed = queue.inspect("email", "second").await.unwrap().unwrap();
    assert_eq!(failed.status, JobStatus::Failed);
    assert_eq!(failed.last_error.as_deref(), Some("permanent"));
    assert!(
        queue
            .claim("email", Duration::from_secs(30))
            .await
            .unwrap()
            .is_none()
    );

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
    assert_ne!(abandoned.token, recovered.token);
    assert_eq!(
        queue.fail(&abandoned, "stale").await.unwrap(),
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

    // Duplicate batches roll back every insert, including earlier valid jobs.
    assert!(
        queue
            .enqueue_batch(vec![
                Job::new("rollback", "email", "{}"),
                Job::new("first", "email", "{}")
            ])
            .await
            .is_err()
    );
    assert!(queue.inspect("email", "rollback").await.unwrap().is_none());
    assert!(
        queue
            .enqueue(Job::new("bad", "email", "not-json"))
            .await
            .is_err()
    );
    assert!(queue.peek("email", 0).await.is_err());
    assert!(queue.claim("email", Duration::ZERO).await.is_err());
    assert!(queue.enqueue_batch(Vec::new()).await.unwrap().is_empty());

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
    assert_eq!(queue.stats("concurrent").await.unwrap().queued, 0);
    assert_eq!(queue.stats("concurrent").await.unwrap().processing, 0);
    // RLS also protects direct queries without a tenant context, and pooled
    // transaction-local settings do not escape to a subsequent borrower.
    let count: (i64,) = sqlx::query_as("SELECT count(*) FROM fl_worker.jobs")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
    let context: (Option<String>,) =
        sqlx::query_as("SELECT nullif(current_setting('forklaunch.tenant_id', true),'')")
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert!(context.0.is_none());
    let denied = sqlx::query("INSERT INTO fl_worker.jobs (tenant_id,queue,id,payload,max_attempts,backoff_ms) VALUES ('foreign','email','foreign','{}',1,0)").execute(&fixture.pool).await;
    assert!(denied.is_err());
    assert_eq!(other.stats("email").await.unwrap().queued, 1);
    fixture.pool.close().await;
    let error = queue.stats("email").await.unwrap_err();
    assert_eq!(error.code, "worker_storage_error");
    assert_eq!(error.message, "Worker storage operation failed");
    fixture.close().await;
}
