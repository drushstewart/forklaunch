//! Owned disposable DB only. `FIELDNOTES_WORKER_TEST_DATABASE_URL` must name it.
use apalis::prelude::*;
use apalis_postgres::{Config, PostgresStorage};
use fieldnotes_insights::{QUEUE, SealedNoteJob};
use fieldnotes_insights::persistence::dispatch_batch;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires owned disposable PostgreSQL database"]
async fn atomic_dispatch_concurrency_rollback_and_actual_apalis_consumer() {
    let url = std::env::var("FIELDNOTES_WORKER_TEST_DATABASE_URL").expect("owned test DB");
    let options: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
    assert!(matches!(options.get_host(), "127.0.0.1" | "localhost"));
    assert_eq!(options.get_database(), Some("fieldnotes_worker_proof"));
    let pool = PgPool::connect(&url).await.unwrap();
    PostgresStorage::setup(&pool).await.unwrap();
    sqlx::raw_sql("CREATE TABLE fieldnotes_outbox(id uuid PRIMARY KEY,workspace_id uuid NOT NULL,note_id uuid NOT NULL,revision bigint NOT NULL,kind text NOT NULL,payload text NOT NULL,created_at timestamptz NOT NULL DEFAULT now(),processed_at timestamptz);").execute(&pool).await.unwrap();
    sqlx::raw_sql("DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='fieldnotes_worker') THEN CREATE ROLE fieldnotes_worker NOLOGIN NOSUPERUSER NOBYPASSRLS; END IF; END $$; ALTER TABLE fieldnotes_outbox ENABLE ROW LEVEL SECURITY; ALTER TABLE fieldnotes_outbox FORCE ROW LEVEL SECURITY; CREATE POLICY dispatch ON fieldnotes_outbox TO fieldnotes_worker USING(true) WITH CHECK(true); GRANT SELECT,UPDATE(processed_at) ON fieldnotes_outbox TO fieldnotes_worker; GRANT USAGE ON SCHEMA apalis TO fieldnotes_worker; GRANT SELECT,INSERT,UPDATE,DELETE ON ALL TABLES IN SCHEMA apalis TO fieldnotes_worker; GRANT USAGE,SELECT ON ALL SEQUENCES IN SCHEMA apalis TO fieldnotes_worker; GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA apalis TO fieldnotes_worker;").execute(&pool).await.unwrap();
    let runtime = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE fieldnotes_worker")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&std::env::var("FIELDNOTES_WORKER_TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE fieldnotes_outbox SET payload='forged'")
            .execute(&runtime)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM fieldnotes_outbox")
            .execute(&runtime)
            .await
            .is_err()
    );
    let operation = Uuid::new_v4();
    let tenant = Uuid::new_v4();
    let note = Uuid::new_v4();
    // Simulates a producer transaction rollback: neither note intent nor job leaks.
    let mut tx = pool.begin().await.unwrap();
    insert(&mut *tx, operation, tenant, note).await;
    tx.rollback().await.unwrap();
    assert_eq!(dispatch_batch(&runtime).await.unwrap(), 0);
    insert(&pool, operation, tenant, note).await;
    // Force failure after queue INSERT, proving the dispatcher never commits one half.
    sqlx::raw_sql("CREATE FUNCTION reject_processed() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture rollback'; END $$; CREATE TRIGGER reject_processed BEFORE UPDATE ON fieldnotes_outbox FOR EACH ROW EXECUTE FUNCTION reject_processed();").execute(&pool).await.unwrap();
    assert!(dispatch_batch(&runtime).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM apalis.jobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    sqlx::query("DROP TRIGGER reject_processed ON fieldnotes_outbox")
        .execute(&pool)
        .await
        .unwrap();
    let (a, b) = tokio::join!(dispatch_batch(&runtime), dispatch_batch(&runtime));
    assert_eq!(a.unwrap() + b.unwrap(), 1);
    assert_eq!(dispatch_batch(&runtime).await.unwrap(), 0);
    let row = sqlx::query("SELECT job,idempotency_key FROM apalis.jobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    let bytes: Vec<u8> = row.get("job");
    let sealed: SealedNoteJob = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(sealed.operation_id, operation);
    assert_eq!(
        row.get::<String, _>("idempotency_key"),
        operation.to_string()
    );
    assert_eq!(sealed.payload, "v1.sealed.fixture");
    async fn consume(job: SealedNoteJob, context: WorkerContext) -> Result<(), BoxDynError> {
        assert_eq!(job.payload, "v1.sealed.fixture");
        context.stop()?;
        Ok(())
    }
    let backend = PostgresStorage::<SealedNoteJob>::new(&runtime).with_config(
        Config::default()
            .queue(QUEUE)
            .batch_size(1)
            .persist_results(true),
    );
    let worker = WorkerBuilder::new("fieldnotes-proof")
        .backend(backend)
        .build(consume);
    tokio::time::timeout(std::time::Duration::from_secs(20), worker.run())
        .await
        .unwrap()
        .unwrap();
    let status: String =
        sqlx::query_scalar("SELECT status FROM apalis.jobs WHERE idempotency_key=$1")
            .bind(operation.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "Done");
    // Delivery can repeat after publication/ack uncertainty. The handler must be idempotent.
    let retry_op = Uuid::new_v4();
    insert(&pool, retry_op, tenant, note).await;
    assert_eq!(dispatch_batch(&runtime).await.unwrap(), 1);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    async fn flaky(
        _: SealedNoteJob,
        calls: Data<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
        context: WorkerContext,
    ) -> Result<(), BoxDynError> {
        if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            return Err(worker::apalis::queue_error(worker::ProcessorError::Failed));
        }
        context.stop().unwrap();
        Ok(())
    }
    let backend = PostgresStorage::<SealedNoteJob>::new(&runtime).with_config(
        Config::default()
            .queue(QUEUE)
            .batch_size(1)
            .persist_results(true),
    );
    let worker = WorkerBuilder::new("fieldnotes-retry-proof")
        .backend(backend)
        .data(calls.clone())
        .retry(fieldnotes_insights::retry_policy().unwrap())
        .build(flaky);
    let retry_started = std::time::Instant::now();
    tokio::time::timeout(std::time::Duration::from_secs(30), worker.run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert!(retry_started.elapsed() >= std::time::Duration::from_secs(1));
    let status: String =
        sqlx::query_scalar("SELECT status FROM apalis.jobs WHERE idempotency_key=$1")
            .bind(retry_op.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "Done");
    // Simulated dead process: a task locked by an expired worker is reclaimed on restart.
    let orphan_op = Uuid::new_v4();
    insert(&pool, orphan_op, tenant, note).await;
    assert_eq!(dispatch_batch(&runtime).await.unwrap(), 1);
    sqlx::query(
        "UPDATE apalis.workers SET last_seen=now()-interval '1 hour' WHERE id='fieldnotes-proof'",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE apalis.jobs SET status='Running',lock_by='fieldnotes-proof',lock_at=now()-interval '1 hour' WHERE idempotency_key=$1").bind(orphan_op.to_string()).execute(&pool).await.unwrap();
    let backend = PostgresStorage::<SealedNoteJob>::new(&runtime).with_config(
        Config::default()
            .queue(QUEUE)
            .batch_size(1)
            .persist_results(true),
    );
    let worker = WorkerBuilder::new("fieldnotes-restart-proof")
        .backend(backend)
        .build(consume);
    tokio::time::timeout(std::time::Duration::from_secs(20), worker.run())
        .await
        .unwrap()
        .unwrap();
    let status: String =
        sqlx::query_scalar("SELECT status FROM apalis.jobs WHERE idempotency_key=$1")
            .bind(orphan_op.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "Done");
}
async fn insert<'a, E>(executor: E, id: Uuid, tenant: Uuid, note: Uuid)
where
    E: sqlx::Executor<'a, Database = sqlx::Postgres>,
{
    sqlx::query("INSERT INTO fieldnotes_outbox(id,workspace_id,note_id,revision,kind,payload) VALUES($1,$2,$3,1,'note-insight-v2','v1.sealed.fixture')").bind(id).bind(tenant).bind(note).execute(executor).await.unwrap();
}
