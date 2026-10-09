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

The generated Dockerfile builds workspace binaries, and Compose selects the service binary, port, and health check. Container deployment to ECS has not yet been verified.

## Current boundaries

Generated service persistence is an in-memory example. Selecting a database records configuration; it does not create a production database repository or migrations. Generated workers execute one demonstration batch. The blueprint IAM, billing, and queue implementations also have unfinished production integrations. Full auth parity, streaming and multipart parity, cross-language conformance, crate publication, and production deployment validation remain tracked in `plan/RUST-FRAMEWORK-BLUEPRINT-IMPLEMENTATION.md`.

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

HTTP middleware emits server spans, request counts/duration, active requests, response sizes when Content-Length is known, auth failures, and error counts. Metric dimensions use matched route templates instead of user identifiers. Correlation IDs are attached to spans/logs rather than metric dimensions. Active requests and spans close when futures are cancelled. HTTP duration currently measures time to response headers; full streaming-body lifetime metrics and generated OTLP exporter bootstrap remain follow-up work.

## Redis adapter

Enable the `redis-client` feature of `forklaunch-infrastructure-redis` to use `client::RedisTtlCache` and `client::RedisQueue`. The cache uses Redis expiration, binary-safe reads/writes, atomic batch writes, and SCAN-based literal prefix listing. The queue uses atomic Redis list operations and preserves FIFO order across clients. Connections have bounded connect/read/write timeouts and support TLS URLs.

These adapters implement synchronous operations; call them from a blocking executor in async services. The basic FIFO queue removes items on dequeue. It does not provide worker leases, acknowledgment, retry scheduling, or dead-letter handling; those belong to the production worker implementation still tracked in the parity plan.

## PostgreSQL IAM

Enable the `postgres` feature on `forklaunch-iam-base` to use `postgres::PostgresIamService`. Apply `PostgresIamService::migrate(&owner_pool)` with a schema-owner connection, then create the service with an application pool and a validated `forklaunch_core::persistence::TenantContext`. Runtime connections must use a role without SUPERUSER or BYPASSRLS. Bind the tenant from verified application context.

The asynchronous service persists users, organizations, roles, and sessions. Passwords use salted Argon2id; hashing runs on blocking tasks. Session tokens are random and stored as SHA-256 digests, expire after one hour, and are checked against current user status. Password changes revoke existing sessions atomically. Every query includes its tenant and uses transaction-local context; migrations enable and force row-level security for reads and writes. The container test exercises a restricted role and connection-pool reuse.

This adapter exposes async methods separately from the synchronous in-memory IAM interface. Generated services still use their example repository. Generator wiring, cookie/CSRF sessions, email/reset/verification flows, and OAuth/OIDC providers remain pending.

## Field encryption compatibility

Enable `forklaunch-core`'s `encryption` feature for `persistence::encryption::FieldEncryptor`. It supports the TypeScript field encryptor's tenant-derived HKDF keys, v1–v4 envelopes, previous-key rings, rotation, and blind indexes. Deterministic v2/v3 ciphertext and blind indexes match TypeScript byte for byte. Sealed v4 values use random nonces. Use a high-entropy master key and preserve previous keys while rotating stored data.

The existing v4 format contains clear, unauthenticated anonymous metadata. Treat it as display metadata and never as an authorization claim. Deterministic encryption and blind indexes reveal equality within their key/tenant scope.

`cli/tests/rust_field_encryption_parity.mjs` runs the actual TypeScript implementation alongside a Rust harness, testing both directions with empty, Unicode, binary-containing, current-key, previous-key, and legacy values. It needs a TypeScript compiler that exposes the compiler API; CI uses TypeScript 5.9.3 via `TSC_BIN`.
