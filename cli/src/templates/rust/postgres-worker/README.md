# {{{name}}}

This PostgreSQL worker polls durable jobs, renews leases, retries failures, and drains its current delivery on Ctrl-C or SIGTERM. Implement idempotent application work in `src/handler.rs` before consuming jobs. The initial handler returns `worker_handler_not_implemented`; it never silently acknowledges unimplemented work.

```sh
DATABASE_URL=postgres://schema_owner:password@localhost/app cargo run -p {{{name}}} -- --migrate
DATABASE_URL=postgres://restricted_app:password@localhost/app TENANT_ID=acme cargo run -p {{{name}}}
```

Use your own credentials. Runtime roles require schema USAGE, table DML, and sequence USAGE in `fl_worker`; SUPERUSER/BYPASSRLS roles are rejected. Bind `TENANT_ID` from trusted application configuration. The example env file is not loaded automatically. Commit bundled `.forklaunch/framework-rs` and `.forklaunch/blueprint-rs` source with the app.

With runtime configuration set, `--enqueue ID JSON` submits a job and `--inspect ID` reports its state without the payload or lease token. `WORKER_QUEUE` defaults to the crate name. Timing and retry settings are listed in `.env.example`; invalid values fail startup. Contract/OpenAPI export requires no database connection.

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

Set `WORKER_HTTP_ADDR=127.0.0.1:8081` to opt into internal `GET /health` and `/ready`. Health returns process state; readiness is 200 when running and 503 while paused or draining, without probing storage. Controls are disabled unless `WORKER_CONTROL_TOKEN` contains a random secret of 32–4096 printable ASCII characters without whitespace. With `Authorization: Bearer <secret>`, POST to `/control/pause`, `/control/resume`, or `/control/drain`. Accepted commands return 202; pause/resume after drain return 409. Pause preserves active work; drain is irreversible and stops retention immediately. Health remains available during drain. HTTP is disabled by default; a token without an address fails startup. Bind this plain HTTP listener to a trusted interface and protect remote access through deployment controls or a TLS proxy. Controls affect this process only, not other replicas. Internal management endpoints are separate from application contracts and SDKs.
