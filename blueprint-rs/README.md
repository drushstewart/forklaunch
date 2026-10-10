# blueprint-rs

Rust blueprint crates for ForkLaunch.

This workspace mirrors the TypeScript `blueprint` style with shared core primitives, contract and interface crates, SDK generation, and reference implementations for IAM, CAC, messaging, billing, and workers. IAM provides an optional PostgreSQL backend with tenant RLS, Argon2id credentials, persistent sessions, an async Axum adapter, and generated modules. The database worker provides an optional PostgreSQL queue implementing async leases, acknowledgments, retry backoff, crash recovery, and failed-job retention. Both integrations use the `postgres` feature. The database worker also provides a `runtime` feature with a polling runner, lease renewal, bounded queue operations, configurable concurrent deliveries, and graceful drain; newly generated PostgreSQL workers use both features.

The Redis worker provides `leased::RedisLeasedQueue` behind its `redis-client` feature, with atomic claims and batches, delivery leases, renewal, retries, and crash recovery. It uses the same async queue interface and runner as PostgreSQL; newly generated Redis workers use this adapter.

Generic service storage, additional provider clients, billing persistence, worker interoperability and remaining modules are tracked in [the implementation plan](../plan/RUST-FRAMEWORK-BLUEPRINT-IMPLEMENTATION.md). See [Rust project documentation](../docs/adding-projects/rust.md) for setup and current boundaries.

## Commands

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

## Package Policy

- Interface crates define contracts and traits.
- Implementation crates depend on interfaces, not the other way around.
- Optional integrations should be feature-gated at the implementation crate that owns them.
- Generated TypeScript and Rust SDKs should come from framework contract artifacts.

## Worker administration

`PostgresWorkerQueue` and `RedisLeasedQueue` implement `AsyncWorkerAdmin`: bounded dead-letter listing, atomic requeue with an explicit retry policy, and age-based purging of completed or failed jobs. Operations stay within the queue's trusted tenant. Retry clears prior attempts and failure reasons; purging permanently frees the job identity. Generated worker binaries expose `--dead-letters`, `--retry`, `--purge-completed`, and `--purge-failed` commands. Automatic retention is disabled by default. Enable `retention::run_worker` with an explicit `RetentionPolicy` to coordinate bounded periodic cleanup with job processing and graceful shutdown.

The shared runner supports `control::WorkerControl` for process-local pause/resume and irreversible drain. The optional `http-control` feature adds health/readiness and bearer-authenticated controls; `http::run` coordinates server lifetime, retention, and active handler drain. Generated workers enable the listener only when `WORKER_HTTP_ADDR` is set; see the [Rust project documentation](../docs/adding-projects/rust.md#worker-http-health-and-controls).

Worker handlers can override `JobHandler::process_batch` to return failures per job. `RunnerConfig::batch_size` and generated `WORKER_BATCH_SIZE` default to one, preserving single handlers. Concurrent batch sizes are bounded to 1000 total deliveries; all leases renew and batch cancellation preserves expiry recovery. See the [batch handler documentation](../docs/adding-projects/rust.md#worker-batch-handlers).

## Messaging

`forklaunch-messaging-interface` defines SMS DTOs, asynchronous service/store traits and base/Twilio module contracts. `forklaunch-messaging-base` provides the TypeScript-compatible hooks-only service and an optional encrypted Postgres store. `forklaunch-messaging-twilio` performs bounded, single-attempt Messages REST dispatch and signed delivery callbacks. `forklaunch-messaging-axum` mounts request-HMAC SMS routes, asymmetric-JWT/system-role compliance routes, public health and verified callbacks. Generated modules, both SDKs, tenant RLS, audit failures, key rotation and persistence are exercised by the [messaging fixture](../cli/tests/rust_messaging_runtime.sh). See [setup and behavioral boundaries](../docs/adding-projects/rust.md#sms-messaging-modules).
