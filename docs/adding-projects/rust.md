---
title: Adding Projects - Rust Backends
category: Guides
description: Generate Rust services, contracts, and TypeScript or Rust SDKs.
---

# Rust backend generation

The Rust port lives on `rohin/rust-framework`. It adds Rust services, workers, libraries, routers, contract exports, and generated SDKs to the existing CLI.

## Create and run a service

```sh
forklaunch init application my-app --language rust -D "My app" -A "My team" -L MIT
cd my-app
forklaunch init service accounts --language rust -d postgresql -D "Accounts service"
forklaunch init router users --language rust --path crates/accounts
cargo test --workspace
cargo run -p accounts
```

The service serves `/health`, `/users`, `/openapi.json`, and a Scalar documentation UI at `/docs`. `HOST`/`PORT` (or `SERVICE_HOST`/`SERVICE_PORT`) configure the listener. `DOCS_PATH`, `DOCS_SERVER_URLS`, and `DOCS_SERVER_DESCRIPTIONS` configure documentation. Ctrl-C and SIGTERM stop the HTTP server gracefully.

Network requests use Axum and the same contract handlers as `local_router`. Path parameters, decoded URL queries, headers, binary bodies, body limits, CORS, and request IDs pass through the framework adapter.

## Export contracts and SDKs

```sh
forklaunch openapi export --path .
forklaunch sdk generate --path . --target all --from contract
forklaunch sdk generate --path . --target all --from contract --check
```

Exports are written under `.forklaunch/contracts/<service>/contract.json` and `.forklaunch/openapi/<service>/openapi.json`. TypeScript output lives in `sdks/typescript`; Rust SDK crates live in `sdks/rust`. `--sdk-targets rust` on app initialization omits the TypeScript package scaffold. Rust services require generated SDKs; live TypeScript SDK mode is rejected.

`sync`, `change`, and `delete` update Cargo membership, local library dependencies, Compose services, manifest entries, and SDK references. Initialization and sync preserve existing application code. Service/worker conversion replaces the generated project shape.

## Portable source and containers

Generated applications include framework source in `.forklaunch/framework-rs`. Commit this directory with the application. Cargo uses relative dependencies, so moving the application or building it inside Docker does not require the CLI author's repository checkout. Existing bundled source is preserved when initialization is repeated.

```sh
docker compose build accounts
docker compose up accounts
```

The generated Dockerfile builds workspace binaries, and Compose selects the service binary, port, and health check. A relocated generated-service image passes local build, health/OpenAPI, and SIGTERM shutdown checks. Container deployment to ECS has not yet been verified.

## Current boundaries

Generic generated service persistence is an in-memory example. Selecting a database records configuration; it does not create a production database repository or migrations. The PostgreSQL IAM module below provides persistent storage and migrations. New PostgreSQL database workers use the durable queue and runner described below. New Redis workers also use the durable leased queue and runner. Other generated worker backends retain their demonstration batch. Billing, provider integrations, and durable Kafka workers remain unfinished. Full auth parity, streaming and multipart parity, cross-language conformance, crate publication, and production deployment validation remain tracked in `plan/RUST-FRAMEWORK-BLUEPRINT-IMPLEMENTATION.md`.

## Validate the port

```sh
cd framework-rs
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd ../blueprint-rs
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd ../cli
cargo test
cargo build --release
cd tests
export FORKLAUNCH_CLI="$(cd ../target/release && pwd)/forklaunch"
bash init_rust_projects.sh
for script in rust*.sh; do bash "$script"; done
```

The Rust E2E job runs generation and lifecycle scripts, checks generated TypeScript SDKs, and calls a relocated Rust service through its generated TypeScript SDK. SDK checks require TypeScript (`TSC_BIN` can point to an installed compiler); the live HTTP check also requires Node.js with built-in `fetch`.

## Authentication and verified sessions

Axum enforces route `auth`, RBAC, and non-public access requirements before running handlers. A protected route without a configured verifier returns 401. Disabling request schema validation does not disable authentication; implicit HEAD requests use their GET route's policy.

Configure `RouterConfig::with_authenticator` with an async credential verifier. It receives the framework request and the route contract, including the declared auth method and custom header settings. Return `AuthenticatedSession` only after verification; Axum then evaluates the route's roles, permissions, scope hierarchy, features, and subscription requirements. Native handlers can extract `Session<MyClaims>`; local framework handlers receive verified claims in `request.session`.

`forklaunch-auth::jwt::verify_jwk` and `verify_jwks` accept typed claims and an explicit `jsonwebtoken::Validation` policy. Set the allowed algorithms, issuer, and audience in that policy. The optional `remote-jwks` feature adds `RemoteJwks`, with one cache per configured endpoint, bounded response sizes/timeouts, Cache-Control and Age handling, refresh deduplication, and a refresh cooldown for unknown key IDs. Remote verification rejects symmetric algorithms and never follows token-provided key URLs. Share one verifier across requests rather than constructing a cache for each request.

Basic login and HMAC verification remain application-configured callbacks. `parse_basic`, `hmac_verify`, and JWT verification helpers provide credential primitives; timestamp/nonce replay policy, session persistence, and provider integrations require application configuration and remain separate parity tasks.

## Telemetry

`forklaunch_core::telemetry::OpenTelemetryCollector` records counters, gauges, histograms, and up/down counters, and exposes its meter for observable instruments. It supplies structured trace/debug/info/warn/error/fatal logging through `tracing`.

Inject a collector through `RouterConfig.telemetry` to use application-owned OpenTelemetry providers. `with_meter` and `with_tracer` accept configured SDK instruments without replacing process-global providers. Configure exporters, a tracing subscriber, and a text-map propagator at application startup. With no configured providers, OpenTelemetry uses its standard no-op implementation.

HTTP middleware emits server spans, request counts/duration, active requests, actual response-body bytes, auth failures, and error counts. Streaming responses retain their span and active-request measurement until completion, body failure, or cancellation, without buffering. Body failures and client cancellation preserve the HTTP status already sent while recording a failed span. HEAD and bodyless statuses finish without awaiting a body. Metric dimensions use matched route templates instead of user identifiers. Correlation IDs are attached to spans/logs rather than metric dimensions. Active requests and spans also close when handler futures are cancelled. Configure exporters in the application; generated OTLP exporter bootstrap remains a follow-up task.

## Redis adapter

Enable the `redis-client` feature of `forklaunch-infrastructure-redis` to use `client::RedisTtlCache` and `client::RedisQueue`. The cache uses Redis expiration, binary-safe reads/writes, atomic batch writes, and SCAN-based literal prefix listing. The queue uses atomic Redis list operations and preserves FIFO order across clients. Connections have bounded connect/read/write timeouts and support TLS URLs.

These adapters implement synchronous operations; call them from a blocking executor in async services. The basic FIFO queue removes items on dequeue. It does not provide worker leases, acknowledgment, retry scheduling, or dead-letter handling; use the async Redis worker adapter below for those operations.

## PostgreSQL IAM

Enable the `postgres` feature on `forklaunch-iam-base` to use `postgres::PostgresIamService`. Apply `PostgresIamService::migrate(&owner_pool)` with a schema-owner connection, then create the service with an application pool and a validated `forklaunch_core::persistence::TenantContext`. Runtime connections must use a role without SUPERUSER or BYPASSRLS. Bind the tenant from verified application context.

The asynchronous service persists users, organizations, roles, and sessions. Passwords use salted Argon2id; hashing runs on blocking tasks. Session tokens are random and stored as SHA-256 digests, expire after one hour, and are checked against current user status. Password changes revoke existing sessions atomically. Every query includes its tenant and uses transaction-local context; migrations enable and force row-level security for reads and writes. The container test exercises a restricted role and connection-pool reuse.

`AsyncIamService`, `AsyncIamSessions`, and `AsyncIamRepository` expose asynchronous operations independently of the synchronous reference interface. `forklaunch-iam-axum` serves these traits with opaque bearer authentication and typed contract schemas. Runtime construction rejects PostgreSQL SUPERUSER and BYPASSRLS roles.

Generate a persistent IAM module in an existing Rust or mixed application:

```sh
forklaunch init module --language rust -m iam-base -d postgresql --path .
# Alternatively, pass --modules iam-base when initializing a Rust application.
DATABASE_URL=postgres://schema_owner:password@localhost/app cargo run -p iam -- --migrate
DATABASE_URL=postgres://restricted_app:password@localhost/app TENANT_ID=acme cargo run -p iam
```

Replace the example URLs with your own credentials. Migration and runtime connections use separate database roles. Generated modules bundle blueprint source in `.forklaunch/blueprint-rs`; commit it alongside the bundled framework. Contract/OpenAPI export works without a database connection.

The scaffold binds one trusted `TENANT_ID` per process. Login, logout, paginated users and organizations, and permission lookup use PostgreSQL. Provision users through the administrative repository API; the generator creates no default account. Directory endpoints require authentication; add your application's authorization policy before exposing them. Tenant selection from verified identity, cookie/CSRF sessions, email/reset/verification flows, JWT issuance, and OAuth/OIDC providers remain pending.

`cli/tests/rust_iam_runtime.sh` checks a relocated generated module against PostgreSQL through its TypeScript SDK, including cross-tenant rejection, session persistence across restart, revocation, and SIGTERM shutdown. It accepts `FORKLAUNCH_TEST_POSTGRES_URL` or starts its own isolated PostgreSQL container.

## Field encryption compatibility

Enable `forklaunch-core`'s `encryption` feature for `persistence::encryption::FieldEncryptor`. It supports the TypeScript field encryptor's tenant-derived HKDF keys, v1–v4 envelopes, previous-key rings, rotation, and blind indexes. Deterministic v2/v3 ciphertext and blind indexes match TypeScript byte for byte. Sealed v4 values use random nonces. Use a high-entropy master key and preserve previous keys while rotating stored data.

The existing v4 format contains clear, unauthenticated anonymous metadata. Treat it as display metadata and never as an authorization claim. Deterministic encryption and blind indexes reveal equality within their key/tenant scope.

`cli/tests/rust_field_encryption_parity.mjs` runs the actual TypeScript implementation alongside a Rust harness, testing both directions with empty, Unicode, binary-containing, current-key, previous-key, and legacy values. It needs a TypeScript compiler that exposes the compiler API; CI uses TypeScript 5.9.3 via `TSC_BIN`.

## PostgreSQL worker queue

Enable `forklaunch-worker-database`'s `postgres` feature and import `PostgresWorkerQueue` from its `postgres` module. It implements `forklaunch_worker_interface::AsyncWorkerQueue`; the synchronous `DatabaseWorkerQueue` remains an in-memory reference implementation.

Apply `PostgresWorkerQueue::migrate(&owner_pool)` before starting consumers. Worker tables and the checksummed migration ledger live in the `fl_worker` schema, independently of IAM migrations. Concurrent migration runners serialize through a transaction-scoped advisory lock, and migration transactions restore the connection's search path.

Runtime roles need `USAGE` on `fl_worker`, `SELECT, INSERT, UPDATE, DELETE` on `fl_worker.jobs`, and `USAGE` on its sequences. The constructor rejects SUPERUSER and BYPASSRLS roles. Construct the queue with a validated, trusted `TenantContext` and a `RetryPolicy`; every operation includes its tenant and runs with transaction-local RLS context.

`enqueue_batch` is atomic and preserves insertion order. IDs are unique within a tenant and queue; duplicates return an error. Enqueued jobs must start with zero attempts and valid JSON payloads of at most 1 MiB. Batches and peek limits are bounded at 1,000 jobs.

`claim` uses `FOR UPDATE SKIP LOCKED` and returns a `JobLease` with a fresh opaque token. Claims increment attempts. `ack`, `fail`, and `renew` check the token and the database deadline; an expired or replaced consumer cannot change the delivery. Leases range from 1 millisecond to 24 hours. Renewal never shortens the current deadline; inspect the job to read an updated deadline.

Call `ack` only after successful processing. On failure, `fail` schedules the configured fixed backoff or retains an exhausted job with `JobStatus::Failed` and its failure reason. Retry policy is stored when enqueuing, so another consumer cannot change an existing job's budget. Expired leases become eligible for redelivery; a claim also moves expired final attempts to failed state. `inspect` reads completed and failed jobs without exposing the lease token. Failure reasons are persisted; use application-safe text.

Delivery is at least once. Make handlers idempotent because a consumer can crash after performing a side effect and before acknowledging it. Renew leases during long-running handlers. Queued statistics include delayed retries; processing statistics include expired leases until the next claim recovers or fails them. `peek` returns currently eligible jobs without claiming them.

The adapter is available to application code. New PostgreSQL worker binaries use the runner below. Dead-letter administration and explicit retention are described below; Kafka durable adapters and distributed control signals remain tracked separately. CI runs `postgres_worker_leases_retries_isolation_and_concurrency` against PostgreSQL, covering concurrent consumers, tenant RLS, retries, crash recovery, stale tokens, batch rollback, and compatibility with IAM migrations.

## Generated PostgreSQL workers

```sh
forklaunch init worker jobs --language rust -t database -d postgresql --path . -D "Jobs"
```

New workers with this backend bundle blueprint source and run a continuous polling loop through `forklaunch-worker-database`'s `runtime` feature. Apply migrations with `cargo run -p jobs -- --migrate` using the owner connection. Start the worker with a restricted `DATABASE_URL` and trusted `TENANT_ID`. Implement application work in `crates/jobs/src/handler.rs`; the initial handler fails with `worker_handler_not_implemented` and leaves jobs subject to their retry budget instead of discarding them.

The runner processes up to `WORKER_CONCURRENCY` jobs at once (1–256, default 1) and renews each active lease while its handler runs. Multiple processes can consume the same queue. `WORKER_QUEUE` defaults to the crate name. Retry policy uses `WORKER_MAX_ATTEMPTS` and `WORKER_BACKOFF_SECONDS`; polling, leases, renewal, and shutdown drain use `WORKER_POLL_MS`, `WORKER_LEASE_MS`, `WORKER_RENEW_MS`, and `WORKER_DRAIN_MS`. Invalid settings fail startup. Queue operations have bounded timeouts. Renewal must run at least three times per lease duration.

Ctrl-C or SIGTERM stops new claims and drains the active job while renewing its lease. If draining times out, the runner cancels the handler and leaves the delivery recoverable after lease expiry. It also cancels a handler when lease renewal reports an obsolete token. Keep all handler work inside its returned future and make external side effects idempotent. Application error codes are retained; detailed handler errors are not automatically persisted.

With runtime environment variables set, use `cargo run -p jobs -- --enqueue ID JSON` to submit a job and `--inspect ID` to read its state. Inspection omits payloads and lease tokens. Contract/OpenAPI export works without a database connection. Reinitialization, sync, and worker rename preserve the application handler and Cargo metadata; rename updates generated SDKs and contract artifacts.

Existing demonstration workers are preserved when reinitialized. To upgrade explicitly, run `forklaunch change worker --path crates/jobs -t database -d postgresql --confirm`; backend conversion installs the runner while retaining an existing handler and unrelated Cargo metadata. Batch-handler configuration, worker HTTP health/control endpoints, distributed signals, and durable Kafka backends remain parity tasks. `cli/tests/rust_worker_runtime.sh` tests a relocated generated worker against PostgreSQL, including renewal, drain, retries, forced cancellation, process crashes, tenant isolation, source preservation, rename, and delete.

## Redis leased worker queue

Enable `forklaunch-worker-redis`'s `redis-client` feature and import `leased::RedisLeasedQueue`. Call `RedisLeasedQueue::connect(url, namespace, trusted_tenant, retry_policy).await` and use `AsyncWorkerQueue` operations. Enable `forklaunch-worker-database`'s `runtime` feature to run the shared async runner with this adapter.

Redis scripts atomically claim jobs, fence acknowledgments and renewal with delivery tokens, schedule retries, and retain completed and failed jobs. A job's retry policy is persisted at enqueue time. Mixed-queue batches validate duplicate identities and Redis key types before writing; identities are unique within a tenant, namespace, and queue. Each claim or peek recovers up to 128 expired leases to bound script execution; larger expired backlogs recover over successive polls. Delivery is at least once, so handlers must be idempotent.

Tenant and namespace names are encoded into Redis keys. This provides logical isolation through trusted application code; configure Redis ACLs separately. Persistence and replication settings determine durability across Redis server restarts. The adapter uses a standalone Redis connection manager with bounded waits and TLS URL support. Keys share a cluster hash slot, but Redis Cluster redirection support is not implemented. Keep payloads small; the adapter caps each payload at 1 MiB and batches at 1,000 jobs.

Real Redis integration tests cover client restart, mixed-queue batches, collision rollback, FIFO ordering, ten concurrent consumers, retry budgets, stale leases, tenant isolation, and shared runner renewal during shutdown drain. Generated Redis workers use this adapter and the shared async runner.

## Generated Redis workers

```sh
forklaunch init worker jobs --language rust -t redis --path . -D "Jobs"
REDIS_URL=redis://localhost:6379/ WORKER_NAMESPACE=my-app TENANT_ID=acme cargo run -p jobs
```

The worker bundles portable framework and blueprint sources, requires a trusted tenant and namespace, and runs continuously until Ctrl-C or SIGTERM. Implement idempotent application work in `crates/jobs/src/handler.rs`; the initial handler fails explicitly until implemented. Timing, retry settings, `--enqueue ID JSON`, and `--inspect ID` match the PostgreSQL worker. Redis requires no migration command. OpenAPI and contract export work without Redis credentials.

Reinitialization and sync preserve application code. Rename updates imports, artifacts, SDK names, and Compose entries. Explicit backend changes support both Redis and PostgreSQL while preserving existing handlers and unrelated Cargo metadata. Upgrade an existing demonstration worker with `forklaunch change worker --path crates/jobs -t redis --confirm`. Existing documentation and example env files are preserved during conversion; configure the new backend's environment variables before starting it. `bullmq` remains a separate demonstration backend without BullMQ wire compatibility.

`cli/tests/rust_redis_worker_runtime.sh` validates a relocated generated worker against Redis, including process crashes, lease recovery, renewal, shutdown drain, retry exhaustion, tenant isolation, rename, backend conversion, and delete.

## Worker concurrency

Both generated PostgreSQL and Redis workers accept `WORKER_CONCURRENCY` (1–256, default 1). Each active job has its own lease; batches renew their leases together. On shutdown every consumer stops claiming jobs and drains its active handler within `WORKER_DRAIN_MS`; work still queued remains available for the next process. A storage failure stops the other consumers and drains their active work before the runner returns the first error.

For direct library use, set `RunnerConfig::concurrency`. `RunReport` combines counts from all consumers. Handlers share the same `JobHandler` instance and must support concurrent calls, keep asynchronous work inside the returned future, and make external side effects idempotent. This setting controls concurrent async handler batches; use separate processes or a blocking executor for CPU-heavy work. Batch handlers are described below. Distributed worker signals remain a separate parity task.

## Dead letters and retention

Both durable queue adapters implement `forklaunch_worker_interface::AsyncWorkerAdmin`. `dead_letters(queue, limit)` returns retained failures in terminal-time order; ordering within a timestamp tie is backend-specific. `retry_dead_letter(queue, id, policy)` atomically requeues a failed job with an explicit new budget, resets attempts and failure text, and fences old lease capabilities. Concurrent retries of the same failure have one winner.

`purge_terminal(queue, TerminalStatus::Completed | TerminalStatus::Failed, minimum_age, limit)` permanently deletes at most the requested number of terminal jobs older than the given duration. It preserves queued and processing jobs. Limits must be 1–1000 and ages must be at least 1 ms and at most 3650 days. Deletion frees identities for resubmission. Invoke bounded calls from an operator or scheduler, or opt into the periodic policy below. Automatic retention is disabled by default.

Generated workers expose these commands using their configured tenant and queue:

```sh
cargo run -p jobs -- --dead-letters 100
WORKER_MAX_ATTEMPTS=3 WORKER_BACKOFF_SECONDS=30 cargo run -p jobs -- --retry JOB_ID
cargo run -p jobs -- --purge-completed 604800 100
cargo run -p jobs -- --purge-failed 2592000 100
```

Listing and inspection print summaries without payloads or lease tokens. Retry uses `WORKER_MAX_ATTEMPTS` and `WORKER_BACKOFF_SECONDS`. Purge command ages are positive integer seconds; output reports the number removed. Run PostgreSQL migrations with the owner connection before upgrading runtime workers; the new migration adds the terminal-retention index without changing existing terminal timestamps.

Redis adds terminal indexes and incrementally upgrades legacy retained records on first administration use. Legacy terminal timestamps were not recorded, so their retention age starts when first observed; existing jobs are not prematurely deleted. Each indexing script scans one page and the client bounds the entire operation by its timeout. An interrupted upgrade resumes from its stored cursor on the next call. Upgrade all consumers before using administration: older consumers do not maintain the new terminal indexes. Redis isolation continues to depend on trusted tenant binding and deployment ACLs.

Integration tests cover stale capabilities after retry, changed retry budgets, concurrent retry/purge races, tenant isolation, bounded purges, protection of active and queued jobs, identity reuse, and legacy Redis indexing. Generated-process tests exercise the commands against both Redis and PostgreSQL.

## Automatic worker retention

Generated Redis and PostgreSQL workers opt into periodic cleanup when `WORKER_COMPLETED_RETENTION_SECONDS` and/or `WORKER_FAILED_RETENTION_SECONDS` is set to a positive age. Each unset status is retained indefinitely. The defaults are no automatic deletion, a 60000 ms interval, at most 100 deletions per enabled status per tick, and a 5000 ms operation timeout. Configure the latter settings with `WORKER_RETENTION_INTERVAL_MS`, `WORKER_RETENTION_BATCH_LIMIT`, and `WORKER_RETENTION_TIMEOUT_MS`. Invalid ages, limits, intervals, and timeouts fail startup.

For direct library use, enable the database worker's `runtime` feature and call `retention::run_worker` with a `RetentionPolicy`, or use `retention::run` for standalone maintenance. Cleanup starts after the first interval and remains scoped to the queue's tenant. Reports count completed and failed deletions separately. Shutdown cancels maintenance and drains current handlers with lease renewal; a cleanup error or timeout also stops new claims and drains active handlers before returning the first error. Newly completed jobs during shutdown drain are retained because maintenance has already stopped.

Real backend tests check default preservation, independent completed/failed ages, tenant isolation, bounded cleanup, and active/queued job protection. Runtime tests check maintenance timeouts and cancellation; a Redis fault test verifies handler drain after maintenance failure. Generated-process suites validate opt-in cleanup and coordinated shutdown against both backends.

## Process-local worker controls

With the worker database crate's `runtime` feature, create a `control::WorkerControl` and pass it to `runner::run_controlled` or `retention::run_worker_controlled`. Clones share state across the process's consumers. `pause()` stops future claims while active handlers and claims already submitted to storage continue. Leases renew throughout processing. `resume()` restarts polling. `drain()` stops new claims and finishes active handlers within the configured drain timeout; it cannot be reversed. Pause/resume return false once draining starts. A new run needs a new control.

Retention continues while paused and stops as soon as draining starts. Shutdown signals and storage failures also set the control to draining. These controls are local to one runner process; distributed signaling remains a separate task. Real Redis and PostgreSQL integration tests verify paused startup, active lease renewal, pause without cancellation, resume, sticky drain, and preservation of queued work.

## Worker HTTP health and controls

Set `WORKER_HTTP_ADDR` to an explicit socket address, for example `127.0.0.1:8081`, to enable the internal worker listener. HTTP is disabled by default. `GET /health` returns 200 with the process state (`running`, `paused`, or `draining`). `GET /ready` returns 200 while running and 503 while paused or draining. Readiness describes consumer state; it does not probe storage connectivity. Responses contain no job payloads, credentials, or tenant identifiers.

Health and readiness need no token. Controls are disabled (404) unless `WORKER_CONTROL_TOKEN` is configured with 32–4096 printable ASCII characters without whitespace; use a randomly generated secret. Send `Authorization: Bearer <secret>` to `POST /control/pause`, `/control/resume`, or `/control/drain`. Missing, wrong, or duplicate credentials return 401. Accepted commands return 202 and the process state. Pause and resume return 409 after drain begins. A token without a listener address and invalid tokens fail startup. These internal endpoints use plain HTTP; bind them to a trusted interface and use deployment access controls or a TLS proxy when remote access is needed.

The listener stays available during handler drain with readiness 503, then shuts down with a five-second connection deadline. Bind failure prevents claims. A server failure stops new claims and drains handlers. Automatic retention continues during pause and stops at drain. Controls affect only the bound tenant/queue in this process; other replicas continue independently. These management routes are separate from application contracts and SDKs.

For library use, enable `forklaunch-worker-database`'s `http-control` feature and use `http::run` with an optional `HttpConfig` and retention policy, or mount `http::router` with your own `WorkerControl` in an existing server. Generated-process tests cover listener conflicts, health-only operation, bearer authentication, pause/resume, readiness during drain, and coordination with retention against both durable backends.

## Atomic batch delivery claims

Both durable adapters implement `AsyncWorkerQueue::claim_batch(queue, lease_duration, limit)` for limits 1–1000. Each atomic operation returns currently eligible jobs in delivery order, with a fresh independent token per job. Concurrent claimers receive disjoint live deliveries. Each lease is acknowledged, renewed, or failed independently, and the existing retry budget and tenant fences apply. An empty queue returns an empty vector. PostgreSQL uses a locked selection and transactional update; Redis validates the entire selected batch before acquiring its deliveries. PostgreSQL requires the built-in `gen_random_uuid` function available in PostgreSQL 13 and later; integration tests use PostgreSQL 16.

The trait's compatibility default supports a limit of one and rejects larger batches before acquiring any delivery. Custom adapters must override it to provide atomic batches. The runner and generated settings below support batch handler execution.

## Worker batch handlers

Set `WORKER_BATCH_SIZE` (default 1) to claim and process up to that many ready jobs in each handler call. `WORKER_CONCURRENCY` now limits simultaneous handler batches; their product must be at most 1000, and batch size must be 1–1000. Batches contain currently eligible jobs and can be smaller than the limit; the runner does not wait to fill a batch. With size 1, existing `JobHandler::process` behavior is preserved.

Override `JobHandler::process_batch(Vec<Job>) -> BlueprintResult<Vec<JobFailure>>` for bulk application work. Return only failed job IDs and errors; omitted jobs are acknowledged. A whole-batch error retries every job. An unknown or duplicate failure ID invalidates the entire result before any acknowledgments and fails all deliveries with `worker_invalid_batch_result`. Each job follows its own persisted retry budget. The compatibility implementation calls `process` sequentially for each job, so existing generated handlers remain usable. Only bounded, sanitized error codes are stored.

Every claimed lease renews while the batch future runs. If any delivery loses its lease, the runner cancels the entire batch future, counts lost leases and abandons its other deliveries for expiry recovery. Stop all work when the future is dropped and keep external effects idempotent. Shutdown drains active batches with renewal; exceeding the drain timeout cancels the batch and leaves unacknowledged jobs recoverable. Acknowledgments and failures are independently fenced and are not an atomic transaction across the batch. Storage operations, including parallel lease renewal and settlement, remain bounded by the renewal interval; choose timing and batch sizes suitable for the backend connection pool.

Redis and PostgreSQL tests cover partial failures, whole-batch errors, invalid failure identities, concurrency, renewal, and drain. Generated-process suites verify grouped handler calls, per-job retries, long-batch renewal, cancellation/recovery, queued-job protection, and invalid settings.

## Release builds

Release creation builds Rust workspaces with `cargo build --workspace --release`. When a Rust application also has a JavaScript workspace at its application or modules root, release creation installs and builds that workspace too. Mixed applications must provide its `package.json`. Existing root build scripts are honored; generated SDK workspaces without one build their packages recursively. PNPM and Bun use frozen installation when their lockfile is present. Build/install failure stops before export and release manifest creation.

Mixed applications using Tokio export their TypeScript services through Node; Bun and Node applications retain their configured JavaScript runtime. Rust components use the framework contract exporter. `--prebuilt-openapi` continues to skip installation, builds, and application execution. `rust_release_runtime.sh` exercises these paths through the actual CLI with owned command/platform fixtures; it validates orchestration rather than compiling the fixture applications. Generated service runtime and export suites validate real compiler/runtime behavior separately. No test publishes a release.

## Packaged CLI sources

`cli/tests/rust_packaged_install.sh` creates a Cargo source archive, installs that archive outside the checkout, then generates and relocates an application with a service and both durable worker backends. It checks the complete workspace with Cargo, exports real framework contracts and OpenAPI without storage credentials, generates both SDK targets, and checks them again. Bundled framework/blueprint files must contain no symlinks or checkout paths. The CLI build CI job runs this regression before uploading its binary.

For local reuse of debug artifacts, set `FORKLAUNCH_PACKAGE_PROFILE=debug`. `FORKLAUNCH_PACKAGE_INSTALL_TARGET` and `FORKLAUNCH_PACKAGE_GENERATED_TARGET` can select existing Cargo caches. This verifies source-archive installation; registry publication and installation of published crate versions remain separate release work.
