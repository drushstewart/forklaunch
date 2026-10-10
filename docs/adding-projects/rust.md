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

Generic generated service persistence is an in-memory example. Selecting a database records configuration; it does not create a production database repository or migrations. The PostgreSQL IAM module below provides persistent storage and migrations. New PostgreSQL database workers use the durable queue and runner described below. Other generated worker backends retain their demonstration batch. Billing, provider integrations, and durable Redis/Kafka workers remain unfinished. Full auth parity, streaming and multipart parity, cross-language conformance, crate publication, and production deployment validation remain tracked in `plan/RUST-FRAMEWORK-BLUEPRINT-IMPLEMENTATION.md`.

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

These adapters implement synchronous operations; call them from a blocking executor in async services. The basic FIFO queue removes items on dequeue. It does not provide worker leases, acknowledgment, retry scheduling, or dead-letter handling; those belong to the production worker implementation still tracked in the parity plan.

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

The adapter is available to application code. New PostgreSQL worker binaries use the runner below. Dead-letter administration and retention, Redis/Kafka durable adapters, and distributed control signals remain tracked separately. CI runs `postgres_worker_leases_retries_isolation_and_concurrency` against PostgreSQL, covering concurrent consumers, tenant RLS, retries, crash recovery, stale tokens, batch rollback, and compatibility with IAM migrations.

## Generated PostgreSQL workers

```sh
forklaunch init worker jobs --language rust -t database -d postgresql --path . -D "Jobs"
```

New workers with this backend bundle blueprint source and run a continuous polling loop through `forklaunch-worker-database`'s `runtime` feature. Apply migrations with `cargo run -p jobs -- --migrate` using the owner connection. Start the worker with a restricted `DATABASE_URL` and trusted `TENANT_ID`. Implement application work in `crates/jobs/src/handler.rs`; the initial handler fails with `worker_handler_not_implemented` and leaves jobs subject to their retry budget instead of discarding them.

The runner processes one job at a time and renews its lease while the handler runs. Run multiple processes for parallel consumption. `WORKER_QUEUE` defaults to the crate name. Retry policy uses `WORKER_MAX_ATTEMPTS` and `WORKER_BACKOFF_SECONDS`; polling, leases, renewal, and shutdown drain use `WORKER_POLL_MS`, `WORKER_LEASE_MS`, `WORKER_RENEW_MS`, and `WORKER_DRAIN_MS`. Invalid settings fail startup. Queue operations have bounded timeouts. Renewal must run at least three times per lease duration.

Ctrl-C or SIGTERM stops new claims and drains the active job while renewing its lease. If draining times out, the runner cancels the handler and leaves the delivery recoverable after lease expiry. It also cancels a handler when lease renewal reports an obsolete token. Keep all handler work inside its returned future and make external side effects idempotent. Application error codes are retained; detailed handler errors are not automatically persisted.

With runtime environment variables set, use `cargo run -p jobs -- --enqueue ID JSON` to submit a job and `--inspect ID` to read its state. Inspection omits payloads and lease tokens. Contract/OpenAPI export works without a database connection. Reinitialization, sync, and worker rename preserve the application handler and Cargo metadata; rename updates generated SDKs and contract artifacts.

Existing demonstration workers are preserved when reinitialized. To upgrade explicitly, run `forklaunch change worker --path crates/jobs -t database -d postgresql --confirm`; backend conversion installs the runner while retaining an existing handler and unrelated Cargo metadata. Batch-handler configuration, worker HTTP health/control endpoints, distributed signals, and durable Redis/Kafka backends remain parity tasks. `cli/tests/rust_worker_runtime.sh` tests a relocated generated worker against PostgreSQL, including renewal, drain, retries, forced cancellation, process crashes, tenant isolation, source preservation, rename, and delete.
