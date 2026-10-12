# blueprint-rs

Rust blueprint crates for ForkLaunch.

This workspace mirrors the TypeScript `blueprint` style with shared core primitives, contract and interface crates, SDK generation, and reference implementations for IAM, CAC, messaging, billing, and workers. IAM provides an optional PostgreSQL backend with tenant RLS, Argon2id credentials, persistent sessions, an async Axum adapter, and generated modules. The database worker provides an optional PostgreSQL queue implementing async leases, acknowledgments, retry backoff, crash recovery, and failed-job retention. Both integrations use the `postgres` feature. The database worker also provides a `runtime` feature with a polling runner, lease renewal, bounded queue operations, configurable concurrent deliveries, and graceful drain; newly generated PostgreSQL workers use both features.

The Redis worker provides `leased::RedisLeasedQueue` behind its `redis-client` feature, with atomic claims and batches, delivery leases, renewal, retries, and crash recovery. It uses the same async queue interface and runner as PostgreSQL; newly generated Redis workers use this adapter.

Generic service storage, additional provider clients, billing provider/module wiring, worker interoperability and remaining modules are tracked in [the implementation plan](../plan/RUST-FRAMEWORK-BLUEPRINT-IMPLEMENTATION.md). See [Rust project documentation](../docs/adding-projects/rust.md) for setup and current boundaries.

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

The official BullMQ adapter also supports [lazy age retention](https://docs.bullmq.io/guide/queues/auto-removal-of-jobs) and [stalled-job recovery](https://docs.bullmq.io/guide/jobs/stalled), tested against actual TypeScript producers/consumers under Redis ACLs. Configure upstream queue/job retention policies through `BullMqOptions.queue`; age values are seconds. Worker options select lock/stall intervals and maximum stalls/starts. Failure observers receive original processing and preprocessor events even after automatic removal. A feature-gated patch to the pinned official worker emits outcome data before its existing failure notification; both notifications use the same original snapshot and Redis wire semantics. The vendored dependency retains upstream source/license provenance and documents the outcome hook and option compatibility patch in `crates/vendor/bullmq-official/FORKLAUNCH_PATCH.md`.

BullMQ queue defaults also support simple deduplication, TTL throttling and
replacement of delayed jobs (`deduplication` with `ttl`, `extend`, `replace` and
job `delay`). Actual TypeScript/native producers share the same deduplication
keys in both directions, including release after completion and TTL expiry.
`keep_last_if_active` retains the newest duplicate submitted during processing
and runs one follow-up after either successful completion or terminal failure.
Configure these through `BullMqOptions.queue.default_job_options`. Failed
attempts retain their reason and diagnostic history through immediate/delayed
retries and terminal failure; `stack_trace_limit` controls the newest retained
entries (zero clears history). Native entries contain Rust error diagnostics.

The official queue available from `BullMqProducer::inner()` also provides
persistent interval/cron schedulers and inspection/removal. Actual TypeScript
and native workers exchange three-iteration schedules for both success and
failure. The next iteration is scheduled before processing starts, so removal
cancels queued work even while the current job is active; it lets that active job
finish. A preprocessor rejection still advances the schedule. Zero
`max_started_attempts` disables that limit, as in TypeScript. Scheduler templates
and all iterations preserve `keep_logs`, `stack_trace_limit` and `size_limit`;
actual workers verify bounded logs and failure histories in both directions.
Iterations also retain creation timestamps and the scheduled instant for cron.

Worker handlers can override `JobHandler::process_batch` to return failures per job. `RunnerConfig::batch_size` and generated `WORKER_BATCH_SIZE` default to one, preserving single handlers. Concurrent batch sizes are bounded to 1000 total deliveries; all leases renew and batch cancellation preserves expiry recovery. See the [batch handler documentation](../docs/adding-projects/rust.md#worker-batch-handlers).

## Messaging

`forklaunch-messaging-interface` defines SMS DTOs, asynchronous service/store traits and base/Twilio module contracts. `forklaunch-messaging-base` provides the TypeScript-compatible hooks-only service and an optional encrypted Postgres store. `forklaunch-messaging-twilio` performs bounded, single-attempt Messages REST dispatch and signed delivery callbacks. `forklaunch-messaging-axum` mounts request-HMAC SMS routes, asymmetric-JWT/system-role compliance routes, public health and verified callbacks. Generated modules, both SDKs, tenant RLS, audit failures, key rotation and persistence are exercised by the [messaging fixture](../cli/tests/rust_messaging_runtime.sh). See [setup and behavioral boundaries](../docs/adding-projects/rust.md#sms-messaging-modules).


## Ecommerce providers

`forklaunch-ecommerce-stripe` creates and retrieves PaymentIntents, resumes checkout without creating another intent, and supports direct Connect charges with bounded integer application fees. `StripePaymentDto` includes an optional response-only client secret; the shared store persists only the provider reference. `forklaunch-ecommerce-paypal` implements OAuth, Orders creation/capture and remote webhook signature verification. Both clients use a bounded TLS transport with redirects and automatic retries disabled, and sanitized errors.

Both payment wrappers share the base ecommerce store. PayPal uses its atomic confirmation claim before capture and releases the claim on provider failure, matching the TypeScript wrapper. The claim provisionally marks a payment succeeded; process interruption during capture requires reconciliation. Generated provider webhooks verify signatures, serialize durable event handling and drive atomic order/inventory delivery. Interrupted-capture reconciliation remains tracked in the [implementation plan](../plan/RUST-FRAMEWORK-BLUEPRINT-IMPLEMENTATION.md). Actual TypeScript conformance fixtures cover [Stripe](../cli/tests/rust_ecommerce_stripe_parity.mjs) and [PayPal](../cli/tests/rust_ecommerce_paypal_parity.mjs), alongside native live HTTP tests.

`BaseEcommerceService::search_products` applies the storefront's variant price, option and stock filters before the product query. `list_catalog` returns products with variants and optional stock in stable createdAt/id order, with limits clamped to 1–100. PostgreSQL pages before loading product DTOs and scopes bulk variant/stock reads to that page. Integral pagination parameters and storage errors are checked explicitly; missing inventory remains absent. The [catalog conformance fixture](../cli/tests/rust_ecommerce_catalog_parity.mjs) executes the actual TypeScript product controllers.

The ecommerce base `worker` feature exposes `events::OrderInventoryHandler` for the leased database/Redis/Kafka runner. Paid events decrement inventory; cancellation from paid/fulfilled restocks it. All item adjustments and an event receipt commit together, so retry after a failed item or lost queue acknowledgment cannot apply partial or duplicate stock changes. Receipt identity ignores delivery metadata but rejects reused IDs with different logical payloads. Workers validate positive quantities, legal transitions and matching job/event/queue identities. PostgreSQL runtime roles also need `SELECT, INSERT` on `fl_ecommerce.inventory_event_receipts`; use the owner migration command to apply versioned tables/indexes.

With both `postgres` and `worker` enabled, `PostgresEcommerceStore::transition_order_with_event` updates the order and enqueues its inventory event in one tenant-bound transaction. Apply both ecommerce and database-worker migrations with an owner connection. Runtime roles need the documented table/sequence grants for both schemas. Queue-write failures roll back the transition and its PII read audit. The underlying `PostgresWorkerQueue::enqueue_in_transaction` checks the caller transaction's tenant and leaves commit/rollback ownership with the caller.

`forklaunch-ecommerce-axum::ecommerce_router` mounts 31 application routes plus health, including import, checkout and provider callbacks. It creates request-scoped audited stores from verified principals, selects Stripe/PayPal from the payment request and uses atomic order/event production for transitions. `contracts::module_registry()` adds the health route and documented validation/storage/provider errors to the core TypeScript-compatible route registry. Checked JSON bodies return 400 for invalid typed inputs; query arrays use the SDK/MCP JSON wire form. The [live adapter fixture](crates/implementations/ecommerce/forklaunch-ecommerce-axum/tests/http.rs) starts HTTP and MCP against restricted-role PostgreSQL and owned provider endpoints. Generated module fixtures cover checkout/import/provider callbacks, both SDKs, worker delivery and lifecycle. Compliance and automated worker orchestration remain in the implementation plan.

Billing base services are available in `forklaunch_billing_base::services` with typed asynchronous interfaces in `forklaunch_billing_interface::services`. Plans and subscriptions are durable; checkout sessions, payment links and portal sessions use a configurable TTL cache independent of provider expiry, with optional database backups. The memory and PostgreSQL stores preserve TypeScript lifecycle differences: checkout completion retains backup status, payment completion removes backup rows, and explicit portal/checkout expiry removes only the cache entry. PostgreSQL requires a restricted runtime role, encrypts provider payloads and opaque session URIs, and commits plaintext reads with actor audits. Provider-ID lookup reads only indexed metadata. The [actual TS base comparison](../cli/tests/rust_billing_base_parity.mjs) covers 168 operations over generic mapper/cache/ORM boundaries. Configured Stripe mappers, real Stripe service wiring and generated billing HTTP/MCP modules remain tracked; the older root-level billing classes are reference scaffolds.

BullMQ producers inherit configured job defaults while the wrapper overrides retry attempts/backoff and intentionally leaves job IDs auto-generated, matching TypeScript. Metadata includes log limits, stack-trace limits, timestamps and size limits. Actual cross-language fixtures exercise persisted options, oversized-job rejection, bounded log retention, delayed/priority/LIFO ordering and worker concurrency. Repeat/deduplication, rate limiting and further option behavior remain under audit.

IAM directory parity now includes typed user/role/permission/organization CRUD interfaces, transactional memory and PostgreSQL stores, nested relationship hydration, encrypted sensitive fields, retained-key uniqueness claims and atomic actor audits. Generated IAM modules expose thirty configured directory operations over HTTP and MCP alongside the existing opaque session endpoints. Directory users and authentication identities remain separate, matching the configured TypeScript CRUD mapper's omission of passwords. Cookie/CSRF, JWT issuance, verification/reset, OAuth/OIDC, Better Auth and application-specific compliance integration remain separate gates. Base discovery returns 204; its system-role compliance routes return 404 under the configured all-`none` entity policy.
