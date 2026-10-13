# {{{name}}}

This Redis worker polls durable jobs, renews leases, retries failures, and drains its current delivery on Ctrl-C or SIGTERM. Implement idempotent application work in `src/handler.rs` before consuming jobs. The initial handler returns `worker_handler_not_implemented`; it never silently acknowledges unimplemented work.

```sh
REDIS_URL=redis://localhost:6379/ WORKER_NAMESPACE=my-app TENANT_ID=acme cargo run -p {{{name}}}
```

Configure Redis persistence and replication for your durability requirements. Namespace and tenant keys provide logical isolation; configure Redis ACLs separately. Bind `TENANT_ID` and `WORKER_NAMESPACE` from trusted application configuration. The adapter uses a standalone Redis connection manager; Redis Cluster redirection support is not implemented. The example env file is not loaded automatically. Commit bundled `.forklaunch/framework-rs` and `.forklaunch/blueprint-rs` source with the app.

With runtime configuration set, `--enqueue ID JSON` submits a job and `--inspect ID` reports its state without the payload or lease token. `WORKER_QUEUE` defaults to the crate name. Timing and retry settings are listed in `.env.example`; invalid values fail startup. Contract/OpenAPI export requires no Redis connection.

The runner allows `WORKER_CONCURRENCY` simultaneous handler batches per process (1–256, default 1). `WORKER_BATCH_SIZE` defaults to 1; their product must not exceed 1000. On shutdown it stops claiming jobs, renews each active lease, and waits up to `WORKER_DRAIN_MS` for all active handlers. A timeout cancels the handler and leaves the job recoverable after lease expiry. Keep all handler work inside its returned future. Delivery is at least once; external side effects must be idempotent. Application error codes are retained as failure reasons; detailed error reporting belongs in the handler.

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

Set `WORKER_BATCH_SIZE` to 1–1000 to claim currently eligible jobs in batches; the runner does not wait to fill them. Override `JobHandler::process_batch(Vec<Job>) -> BlueprintResult<Vec<JobFailure>>` for bulk work, returning only failed job IDs and errors. Omitted jobs are acknowledged; a whole-batch error or unknown/duplicate failure IDs fails every member. With size 1 the runner calls `process`; the default batch implementation calls it sequentially for compatibility. Each job keeps its own retry budget and lease token. All leases renew while the batch runs. Loss of any lease cancels the batch and leaves other deliveries recoverable after expiry. Handler drain timeouts cancel the full batch. Keep effects idempotent and all work inside the returned future. Batch acknowledgments are independently fenced rather than atomic across jobs.

## Distributed controls

Set `WORKER_SIGNAL_REDIS_URL` and `WORKER_SIGNAL_NAMESPACE` on every participating
worker to enable Redis Pub/Sub controls. The group is scoped by signal namespace,
trusted `TENANT_ID`, and `WORKER_QUEUE` (defaults to this worker's name). PostgreSQL
workers may use a separate Redis deployment for controls. Without these variables,
HTTP controls remain process-local and no signal connection is opened.

Run `cargo run -- --signal pause`, `--signal resume`, or `--signal drain` with the
same scope and signal environment. These commands do not connect to queue storage.
When enabled, bearer-authenticated HTTP controls publish to the same group and
apply through the ordered subscription; their JSON response reports `subscribers`. This count means Redis
subscribers, not application acknowledgements. Health/readiness reflects local state.
Publication failure returns HTTP 503; delivery can remain uncertain after a timeout. Drain is
irreversible for each running process.

Delivery is ephemeral: disconnected or late subscribers miss commands. Subscription
disconnect, overflow, or malformed messages cause graceful drain and an error exit.
There is no automatic reconnect or durable desired-state restoration: restart the
worker explicitly; a restarted worker begins running. Use a unique namespace for
each environment and Redis ACLs; Redis database numbers do not isolate Pub/Sub.


## Separate control service

Run this executable with `--control-service` and `WORKER_HTTP_ADDR` to serve
health/readiness without connecting to job storage, claiming jobs or running
retention. Without `WORKER_CONTROL_TOKEN`, controls return 404. Authenticated
controls additionally require the distributed signal configuration above; they
publish to consumers without becoming a consumer or signal subscriber. Readiness
describes this control process, not remote queue health. Draining consumers does
not stop the control service; SIGTERM stops it cleanly.

New release manifests advertise distinct control and consumer roles when the
installed generated sources support this entry point. Older/custom sources keep
their existing command. The health/control server does not expose MCP tools.
