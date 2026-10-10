# {{{name}}}

This Redis worker polls durable jobs, renews leases, retries failures, and drains its current delivery on Ctrl-C or SIGTERM. Implement idempotent application work in `src/handler.rs` before consuming jobs. The initial handler returns `worker_handler_not_implemented`; it never silently acknowledges unimplemented work.

```sh
REDIS_URL=redis://localhost:6379/ WORKER_NAMESPACE=my-app TENANT_ID=acme cargo run -p {{{name}}}
```

Configure Redis persistence and replication for your durability requirements. Namespace and tenant keys provide logical isolation; configure Redis ACLs separately. Bind `TENANT_ID` and `WORKER_NAMESPACE` from trusted application configuration. The adapter uses a standalone Redis connection manager; Redis Cluster redirection support is not implemented. The example env file is not loaded automatically. Commit bundled `.forklaunch/framework-rs` and `.forklaunch/blueprint-rs` source with the app.

With runtime configuration set, `--enqueue ID JSON` submits a job and `--inspect ID` reports its state without the payload or lease token. `WORKER_QUEUE` defaults to the crate name. Timing and retry settings are listed in `.env.example`; invalid values fail startup. Contract/OpenAPI export requires no Redis connection.

The runner allows `WORKER_CONCURRENCY` simultaneous deliveries per process (1–256, default 1). On shutdown it stops claiming jobs, renews each active lease, and waits up to `WORKER_DRAIN_MS` for all active handlers. A timeout cancels the handler and leaves the job recoverable after lease expiry. Keep all handler work inside its returned future. Delivery is at least once; external side effects must be idempotent. Application error codes are retained as failure reasons; detailed error reporting belongs in the handler.

Administration commands use the configured tenant and queue:

```sh
cargo run -p {{{name}}} -- --dead-letters 100
WORKER_MAX_ATTEMPTS=3 WORKER_BACKOFF_SECONDS=30 cargo run -p {{{name}}} -- --retry JOB_ID
cargo run -p {{{name}}} -- --purge-completed 604800 100
cargo run -p {{{name}}} -- --purge-failed 2592000 100
```

Listing omits payloads and lease tokens. Retry acts only on a failed job and resets its attempts and failure reason using the configured retry budget. Purge permanently removes at most the requested number of completed or failed jobs older than the given age in seconds; queued and processing jobs are preserved. Limits must be 1–1000 and ages must be positive and at most 3650 days. Purged identities may be submitted again. Automatic retention is disabled by default; explicit purge commands remain available.

To enable periodic retention, set `WORKER_COMPLETED_RETENTION_SECONDS` and/or `WORKER_FAILED_RETENTION_SECONDS` to a positive age. An unset age keeps that status indefinitely. `WORKER_RETENTION_INTERVAL_MS` defaults to 60000, `WORKER_RETENTION_BATCH_LIMIT` to 100 per status per tick, and `WORKER_RETENTION_TIMEOUT_MS` to 5000. Invalid settings fail startup. Cleanup begins after the first interval, stops on shutdown, and never purges queued or processing jobs. A cleanup failure stops new claims and drains active handlers before the worker exits with an error.
