# Rust Framework And Blueprint Implementation Plan

## Purpose

Build a Rust-native ForkLaunch stack with two new workspaces:

- `framework-rs`: the Rust framework runtime, contracts, adapters, SDK generation, infrastructure traits, and testing utilities.
- `blueprint-rs`: Rust blueprint libraries that mirror the style of the existing TypeScript `blueprint` workspace: shared core primitives, interface crates, and implementation crates for IAM, billing, workers, monitoring, and generated service scaffolds.

The Rust stack should interoperate with the TypeScript stack through shared contract artifacts. A Rust service should be able to generate TypeScript SDKs for frontend consumers and Rust SDKs for service-to-service consumers.

## Active Task Queue (2026-10-09)

- [x] Merge `origin/main` (`fae3d3e06`) into `rohin/rust-framework` and resolve CLI API/manifest conflicts; combined CLI compiles. Fetched and rechecked on 2026-10-09: no newer `main` commits remain to merge.
- [x] Replace generated services' TCP stub with Axum and graceful shutdown.
- [x] Remove build-machine Cargo paths by bundling framework source in generated applications.
- [x] Fix SDK mode's missing path argument and sync discovery's optional-argument panic.
- [x] Revalidate the combined workspaces: latest CLI 927 tests pass; latest all-feature framework 91 and blueprint 51 tests pass. Formatting and all-feature Clippy checks pass in both Rust workspaces. Real Redis/PostgreSQL integration and six TypeScript encryption interoperability fixtures pass separately.
- [x] Complete the post-merge generation rerun: init, change, sync, delete, OpenAPI/SDK, and relocated-service HTTP/TypeScript SDK suites pass.
- [x] Validate a real optimized generated Rust service release build through the shared exporter, including framework contracts and OpenAPI artifacts.
- [x] Validate Rust/mixed release build order, shared export dispatch, SDK package builds, failure stops, mixed crate locations, and prebuilt isolation using the actual CLI with owned platform/command fixtures.
- [x] Validate real Cargo/PNPM mixed release builds with a generated Rust service/TypeScript SDK and a minimal tsx service export hook; reuse those artifacts with intentionally unbuildable source, preserving prebuilt isolation and bounded mixed crate paths.
- [x] Complete relocated generated-service container build, health/OpenAPI, and SIGTERM shutdown checks. The Docker capacity blocker is resolved; `rust_container_runtime.sh` passes locally and runs in CI. Builder/runtime both use Debian bookworm.
- [ ] Validate generated ECS defaults, health checks, and service startup separately from the local container checks.
- [x] Verify the CLI Cargo package file list includes all 115 embedded framework and blueprint source/template files; use `.template` manifest names to avoid Cargo excluding nested packages, and add a CI regression check.
- [ ] Finish mixed-app orchestration and one shared SDK surface (phases 13–15).
- [ ] Finish AsyncAPI export and cross-language contract conformance checks (phases 13–15).
- [x] Install the CLI from its Cargo source archive outside the checkout, generate and relocate service/PostgreSQL/Redis workers, compile them, and export real framework contracts/OpenAPI plus both SDK targets; run the regression in the CLI build CI job.
- [ ] Validate registry publication and installation of published crate versions (phases 13–15). Source-archive installation is checked; no crate has been published during this port.

PostgreSQL IAM is wired into generated modules. General service storage, auth/provider lifecycle, billing and Kafka queue implementations remain explicit tasks in phase 12.

## Full TypeScript Parity Scope

The requested target is full runtime, infrastructure, blueprint, CLI, SDK, release, and deployment parity. Generation checks alone are insufficient. Every remaining phase below stays in scope; a check is complete only when behavior is implemented and validated.

Current implementation work:

- [x] Enforce route authentication and RBAC in Axum, including implicit HEAD and routes with schema validation disabled; expose verified typed sessions and local-handler claims.
- [x] Implement direct JWK and cached remote JWKS verification with explicit algorithm/claim policy, rotation, response bounds, expiry, and concurrent-fetch tests.
- [x] Implement Redis-backed TTL records, binary batches, literal prefix scanning, and FIFO operations; validate against an isolated Redis container.
- [x] Add an OpenTelemetry collector and HTTP instruments/spans with SDK export assertions, correlation, and cancellation handling.
- [x] Add framework/blueprint CI covering all features, real Redis/PostgreSQL integration, and TypeScript encryption interoperability.
- [x] Add PostgreSQL IAM CRUD, role/permission resolution, persistent sessions, salted Argon2id credentials, and tenant RLS.
- [x] Port field encryption with byte-compatible deterministic envelopes/blind indexes and bidirectional legacy/rotation checks against the TypeScript implementation.
- [x] Complete streaming-body telemetry with measured frame bytes, producer context propagation, normal completion, body errors, cancellation, HEAD handling, and balanced active-request counts.
- [x] Generate portable PostgreSQL IAM modules with explicit migrations, safe runtime-role checks, typed schemas, and async opaque bearer authentication.
- [x] Add an async PostgreSQL worker queue with leases/retries and validate 50 deliveries across ten concurrent consumers, tenant isolation, and stale-token fencing.
- [ ] Add generated OTLP provider/exporter bootstrap.
- [ ] Finish production persistence and encryption, IAM/provider/session lifecycle, billing/Stripe, and acknowledged database/Redis/Kafka workers (phases 9 and 12).
- [ ] Finish streaming/multipart/WebSocket runtime and SDK behavior, mixed-app orchestration, AsyncAPI export, and cross-language conformance (phases 3–8 and 13–14).
- [ ] Finish container/ECS, published-source installation, documentation, benchmarks, and release validation (phase 15).

## Backend Generation Completion Gate

Generation is tracked separately from production framework and blueprint parity below.

- [x] Bundle framework source with generated applications and use relative Cargo dependencies.
- [x] Serve generated service handlers over Axum with body limits, request IDs, CORS, and documentation UI.
- [x] Shut down generated HTTP services gracefully on Ctrl-C and SIGTERM.
- [x] Exercise init, change, sync, delete, contract/OpenAPI export, and TypeScript/Rust SDK generation in a dedicated CI matrix group.
- [x] Add a relocated-app HTTP test calling the generated Rust service through its generated TypeScript SDK.
- [x] Generate durable PostgreSQL workers and verify renewal, retries, drain, restart recovery, source preservation, backend conversion, rename, and delete against a real database.
- [x] Document usage and distinguish generated examples from production storage and queue integrations.

## Core Principles

- Contracts are the source of truth.
- Rust types define request and response shapes.
- Runtime validation still happens at wire boundaries.
- OpenAPI, AsyncAPI, MCP tools, TypeScript SDKs, and Rust SDKs are generated from the same contract registry.
- `axum` is the first HTTP adapter, not the identity of the framework.
- `blueprint-rs` mirrors the TypeScript blueprint package style, but uses idiomatic Rust crates, traits, derive macros, and generated SDK crates.

## Proposed Repository Layout

```text
framework-rs/
  Cargo.toml
  crates/
    forklaunch-common/
    forklaunch-core/
    forklaunch-contract/
    forklaunch-contract-macros/
    forklaunch-http/
    forklaunch-axum/
    forklaunch-openapi/
    forklaunch-asyncapi/
    forklaunch-mcp/
    forklaunch-sdk-codegen/
    forklaunch-sdk-rust/
    forklaunch-sdk-typescript/
    forklaunch-ws/
    forklaunch-infrastructure-redis/
    forklaunch-infrastructure-s3/
    forklaunch-testing/

blueprint-rs/
  Cargo.toml
  crates/
    forklaunch-blueprint-core/
    interfaces/
      forklaunch-iam-interface/
      forklaunch-billing-interface/
      forklaunch-worker-interface/
      forklaunch-monitoring-interface/
    implementations/
      iam/
        forklaunch-iam-base/
      billing/
        forklaunch-billing-base/
        forklaunch-billing-stripe/
      worker/
        forklaunch-worker-database/
        forklaunch-worker-redis/
        forklaunch-worker-kafka/
    client-sdk/
    sample-app/
    sample-service/
    sample-worker/
```

## Library Choices

- HTTP runtime: `tokio`, `axum`, `tower`, `tower-http`.
- HTTP extras: `axum-extra`.
- Serialization: `serde`, `serde_json`.
- JSON Schema: `schemars`.
- Validation: `garde`.
- OpenAPI: custom ForkLaunch registry with `aide` or `utoipa` as emitters where useful.
- Telemetry: `tracing`, `tracing-subscriber`, `opentelemetry`, `opentelemetry-otlp`, `tracing-opentelemetry`.
- Auth: `jsonwebtoken`, `reqwest`, `moka`, `hmac`, `sha2`, `base64`, `subtle`.
- Secrets and encryption: `secrecy`, `zeroize`, `aes-gcm` or `chacha20poly1305`.
- MCP: `rmcp`.
- WebSockets: Axum WebSocket extractor plus `tokio-tungstenite`.
- Redis: `redis`.
- S3/object storage: `aws-sdk-s3`, with optional `object_store` adapter later.
- Persistence: `sqlx` first; optional `sea-orm` adapter for CRUD-heavy blueprints.
- Config: `config`, `dotenvy`.
- Testing: `testcontainers`, `rstest`, `wiremock`, `insta`, `proptest`.
- Proc macros: `syn`, `quote`, `proc-macro2`, `darling`.

## Rust Auth Framework Findings

There does not appear to be a direct Rust equivalent to Better Auth that should be adopted wholesale without review. The Rust ecosystem has useful auth building blocks and some higher-level frameworks, but ForkLaunch should own the `blueprint-rs` IAM contract and expose adapters where useful.

### Candidates To Evaluate

- `loco-rs`: Rails-style Rust framework with built-in user auth, password reset, verification, JWT/API auth, and SaaS templates. Useful as a reference, but likely too opinionated to embed in ForkLaunch.
- `rs-auth`: composable auth facade with core, Postgres, Axum, OAuth, hooks, email, events, and rate-limit modules. Promising, but young and needs code review before adoption.
- `auth-framework`: broad OAuth2 client/server framework. Potentially relevant for OAuth provider/server use cases, but needs security and maintenance review.
- `tower-sessions`: strong Tower/Axum session middleware with pluggable stores. Good candidate for session storage, not a full identity framework.
- `axum-login`, `axum_session_auth`: Axum session/auth helpers. Useful references or optional adapters, not a complete Better Auth replacement.
- `openidconnect`: strong candidate for OIDC/OAuth relying-party and provider primitives.
- `oauth2` / `oauth-axum`: useful for OAuth provider flows, especially if we want narrower control than a full framework.
- `webgates` / `authkestra`: newer auth orchestration options worth evaluating, but not default choices until audited.
- `webauthn-rs`: candidate for passkeys/WebAuthn later.

### Recommendation

- [ ] Implement `forklaunch-iam-base` as ForkLaunch-owned auth domain logic.
- [x] Use `argon2` and `password-hash` for salted Argon2id password hashing; reject malformed hashes and disabled users.
- [ ] Use `tower-sessions` for server-side sessions when cookie/session auth is enabled.
- [ ] Use `openidconnect` and `oauth2` for OAuth/OIDC provider integrations.
- [ ] Use `jsonwebtoken` for JWT validation/issuance where JWTs are enough.
- [ ] Keep Better Auth parity at the behavior/API level, not as a crate dependency.
- [ ] Add optional adapters for promising frameworks only after security review.

### Auth Evaluation Checklist

- [ ] Maintenance activity and release cadence.
- [ ] Security posture and dependency risk.
- [ ] Password hashing defaults and upgrade strategy.
- [ ] Session storage model and CSRF behavior.
- [ ] OAuth/OIDC provider coverage.
- [ ] Multi-tenant organization support.
- [ ] Role, permission, scope, and feature entitlement integration.
- [ ] WebAuthn/passkey support path.
- [ ] Framework independence from Axum.
- [ ] Fit with generated OpenAPI, SDKs, and MCP auth boundaries.

## Phase 0 - Contract Shape And Compatibility

- [x] Define the Rust contract IR for HTTP routes.
- [x] Define the Rust contract IR for WebSocket/event channels. (`forklaunch-ws`: `WebSocketContract`/`EventContract`)
- [x] Map TypeScript `ContractDetails` fields to Rust equivalents.
- [x] Map TypeScript typed body variants to Rust equivalents: JSON, text, file, binary, multipart, URL-encoded, SSE, unknown.
- [x] Map response status code schemas and response headers.
- [x] Map route options: request validation, response validation, OpenAPI inclusion, MCP inclusion.
- [x] Map auth and access levels: public, authenticated, protected, internal.
- [x] Define contract artifact format emitted by Rust services.
- [ ] Confirm TypeScript and Rust contract artifacts can normalize to the same OpenAPI and AsyncAPI outputs.
- [ ] Add snapshot fixtures from representative TypeScript routes for parity testing.

## Phase 1 - `framework-rs` Workspace Scaffold

- [x] Create `framework-rs/Cargo.toml` workspace.
- [x] Add shared lint, format, clippy, test, and docs commands.
- [x] Add crate publishing metadata conventions.
- [x] Add CI commands for `cargo fmt`, `cargo clippy`, `cargo test`, and doc generation.
- [x] Add crate feature policy: keep optional integrations behind feature flags.
- [x] Add README explaining Rust framework scope and relationship to TypeScript framework.

## Phase 2 - Core Crates

### `forklaunch-common`

- [x] Port path helpers: slash cleanup, base path joins, OpenAPI path conversion.
- [x] Port string helpers needed by SDK and route names: camel case, pretty camel case, identifier sanitization.
- [x] Add safe JSON helpers and deterministic JSON sorting for snapshots.
- [x] Add common error and result helpers.

### `forklaunch-core`

- [x] Add service metadata, runtime environment, and runtime config validation primitives.

### `forklaunch-contract`

- [x] Define `Contract`, `RouteContract`, `VersionedContract`, `RequestContract`, `ResponseContract`.
- [x] Define `BodySchema`, `HeaderSchema`, `ParamSchema`, `QuerySchema`, `ResponseSchema`.
- [x] Define `AccessLevel`, `AuthConfig`, `JwtAuth`, `BasicAuth`, `HmacAuth`, `RbacConfig`.
- [x] Define `ContractRegistry`.
- [x] Define a language-neutral serializable contract artifact.
- [x] Add conversion to OpenAPI-friendly structures.

### `forklaunch-contract-macros`

- [x] Add derive macro for request and response schemas.
- [x] Add route macro or builder helpers for low-boilerplate route registration.
- [x] Add compile-time checks for route params matching `/:param` path entries.
- [x] Add operation name normalization for SDK generation.

## Phase 3 - Validation And Schema Integration

- [x] Use `serde::Deserialize` for parsing wire input into Rust types.
- [x] Use `garde::Validate` for runtime validation.
- [x] Use `schemars::JsonSchema` for schema metadata.
- [x] Add a framework trait that combines `DeserializeOwned + Serialize + JsonSchema + Validate`. (`forklaunch-validation::FrameworkSchema`)
- [x] Standardize parse errors into ForkLaunch validation errors.
- [x] Support coercion where appropriate and explicit: path/query string to numeric, bool, uuid, date/time. (`forklaunch-validation::coerce`)
- [x] Decide which TypeScript coercions should not be copied because Rust should be stricter. (documented in crate root)
- [x] Add test fixtures for validation errors and coercion behavior.

## Phase 4 - HTTP Core And Axum Adapter

### `forklaunch-http`

- [x] Define request context: correlation ID, idempotency key, trace context.
- [x] Define framework request and response wrappers.
- [x] Define typed handler traits.
- [x] Define middleware traits.
- [x] Define local route execution API equivalent to TypeScript local `fetch`.
- [x] Define error response model matching TypeScript behavior.
- [x] Validate required params, query, headers, body presence, JSON content-type, and documented response statuses in local route execution.

### `forklaunch-axum`

- [x] Add initial Axum adapter facade with route summaries and response conversion helpers.
- [x] Add Axum request-parts conversion into `FrameworkRequest`.
- [x] Add Axum preflight route matching with path-param extraction and shared request validation.
- [x] Implement `ForkLaunchRouter` over `axum::Router`.
- [x] Implement nested routers and base paths.
- [x] Implement typed route registration for GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS, TRACE.
- [x] Implement request validation for params, query, headers, and body.
- [x] Implement response validation by status code.
- [ ] Implement content-type handling for JSON, text, binary, multipart, URL-encoded, SSE. (request content-type validated; typed multipart/SSE extractors pending Phase 3)
- [x] Implement CORS, request body limits, request IDs, and tracing layers through `tower-http`.
- [x] Implement docs and OpenAPI endpoints.
- [x] Implement local route execution for tests and service-internal calls. (via `tower::ServiceExt::oneshot`)
- [ ] Add parity tests against TypeScript route behavior.

## Phase 5 - OpenAPI, Docs, And MCP

### `forklaunch-openapi`

- [x] Generate OpenAPI 3.1 from `ContractRegistry`.
- [ ] Support versioned APIs.
- [x] Support servers, descriptions, tags, summaries, operation IDs, auth schemes, response codes.
- [x] Serve raw OpenAPI JSON.
- [x] Support file output mode for build and CI.
- [x] Add snapshot tests for OpenAPI output.

### Documentation UI

- [x] Serve Scalar UI from generated OpenAPI.
- [x] Optionally serve Swagger UI. (`RouterConfig::swagger_enabled`)
- [x] Allow docs disabling.
- [x] Match TypeScript service/runtime environment variables for generated Rust docs path, host, and port.
- [x] Match OpenAPI server URL and description env configuration.

### `forklaunch-mcp`

- [x] Generate MCP tools from eligible HTTP contracts. (`mcp_tools`)
- [ ] Use `rmcp` for server implementation. (reserved behind the `rmcp-server` feature)
- [x] Support per-route MCP opt-in and opt-out. (`route.options.mcp`)
- [x] Support transport authentication callback. (`McpAuthCallback`)
- [x] Support additional manually registered tools. (`McpToolRegistry::with_tool`)
- [x] Map input/output schemas from `schemars` metadata. (`inputSchema`/`outputSchema` from contract `SchemaRef`)

## Phase 6 - SDK Generation

### Shared SDK Codegen

- [x] Create `forklaunch-sdk-codegen` around the contract artifact and OpenAPI.
- [x] Resolve operation names consistently across TypeScript and Rust.
- [x] Generate response models from JSON Schema for TypeScript and Rust SDKs.
- [x] Generate TypeScript request field/body types from contract request schemas.
- [x] Preserve status-code-specific response typing.
- [x] Carry params, query, headers, body content type, response headers, and response bodies in `SdkGenerationPlan`.
- [x] Support params, query, headers, and JSON body transport in deterministic TypeScript and Rust SDK generators.
- [ ] Support multipart, binary, text responses, and file uploads/downloads.
- [x] Add contract snapshot tests to ensure generated SDKs stay stable. (`client-sdk::tests::sdk_generation_is_deterministic`)

### TypeScript SDKs For Frontend

- [x] Add initial deterministic TypeScript fetch-client source generator from `SdkGenerationPlan`.
- [x] Generate browser-safe TypeScript fetch clients for Rust services.
- [x] Preserve the same consumer syntax as pure live-typed TypeScript SDKs.
- [x] Make generated TypeScript SDKs assignable to the same TypeScript shape as the live `MapToSdk` output.
- [x] Ensure callers can switch between TypeScript live mode and generated SDK mode without changing application call sites.
- [x] Generate SDK methods from route operation names using the same casing and grouping rules as live TypeScript SDKs.
- [x] Preserve the same request object shape: `params`, `query`, `body`, `headers`, and version fields where applicable.
- [x] Preserve status-code-specific response typing and discriminated response handling.
- [x] Emit ESM output with no Node-only dependencies by default.
- [x] Support custom `fetch`, base URL, default headers, auth hooks, and request interceptors.
- [x] Generate request and response TypeScript types.
- [x] Generate discriminated response unions by status code.
- [ ] Support file upload and binary download in browser contexts.
- [ ] Add optional TanStack Query helpers as a later feature.
- [ ] Add SDK tests against a Rust test server.

### TypeScript SDK Compatibility Contract

Rust-generated TypeScript SDKs should feel like the existing pure live-typed TypeScript SDKs. The implementation mechanism changes, but the frontend syntax should not.

```typescript
import { usersService } from '@/sdk';

const response = await usersService.users.createUser({
  body: {
    email: 'test@example.com',
    name: 'Test User'
  },
  headers: {
    authorization: token
  }
});
```

Requirements:

- [x] The generated SDK exports the same service/module shape that live TypeScript SDK mode would expose.
- [x] The generated SDK uses network calls under the hood for Rust services.
- [ ] The generated SDK may use direct live imports for TypeScript services in live mode.
- [ ] Mixed apps can import one SDK surface even when some services are TypeScript and others are Rust.
- [x] A Rust contract change requires regenerating the TypeScript SDK.
- [x] Regeneration should be deterministic so changed contracts produce reviewable diffs.
- [x] Generated TypeScript SDKs must fail TypeScript compilation when downstream code uses removed routes, renamed fields, changed request bodies, or changed response shapes.
- [x] Generated TypeScript SDK tests should compile representative frontend call sites against the generated SDK.

### Rust SDKs For Backend And Rust Clients

- [x] Add initial deterministic Rust request-builder source generator from `SdkGenerationPlan`.
- [x] Generate a Rust SDK crate per service.
- [x] Use `reqwest`, `serde`, and generated typed models.
- [x] Generate typed async methods from operation IDs.
- [x] Generate request builder structs for each operation.
- [x] Generate typed response model structs from contract JSON Schema.
- [x] Generate response enums by status code.
- [x] Support auth hooks for JWT, HMAC, Basic, and custom headers.
- [x] Support Rust-native error enums.
- [ ] Add SDK tests against a Rust test server and a TypeScript test server.

### CLI Integration

- [x] Add commands for `forklaunch sdk generate ts`.
- [x] Add commands for `forklaunch sdk generate rust`.
- [x] Add commands for `forklaunch sdk generate all`.
- [x] Add manifest entries for SDK output directories and target languages.
- [x] Add support for Rust service contract discovery.

## Phase 7 - Auth, Errors, And Telemetry

### Auth

- [x] Implement JWT signature key validation. (`forklaunch-auth::verify_jwt_hs256`)
- [x] Implement direct JWK validation. (`jwt::verify_jwk`, algorithm/key-usage/claim policy tests)
- [x] Implement JWKS URL fetching and caching. (`remote-jwks` feature; rotation, expiry, bounds, and concurrent fetch tests)
- [x] Implement Basic auth callback. (`forklaunch-auth::parse_basic` + app-provided verifier)
- [x] Implement HMAC auth and HMAC header generation. (`forklaunch-auth::hmac_sign`/`hmac_verify`)
- [x] Implement constant-time signature comparison. (`subtle::ConstantTimeEq`)
- [x] Implement session enrichment equivalent to `decodeResource`. (`Principal::from_claims`)
- [x] Implement roles, permissions, scopes, scope hierarchy, feature checks, and subscription checks. (`forklaunch-auth::authorize`)
- [x] Add typed session extraction for handlers. (`Session<T>`, Axum extensions, local-handler verified claims)

### Errors

- [x] Implement standard errors: validation, unauthorized, forbidden, not found, rate limited, internal. (`forklaunch-http::ErrorKind`)
- [x] Include correlation IDs in safe error responses.
- [x] Hide internal error details by default. (`ErrorResponse::redacted`)
- [x] Add structured internal error logging; redact internal handler/status errors from HTTP responses.
- [x] Add tests for error response parity.

### Telemetry

- [x] Implement `OpenTelemetryCollector` around `tracing` and OpenTelemetry. (application-owned providers; generated OTLP bootstrap remains separate)
- [x] Support trace, debug, info, warn, error, fatal logging methods.
- [x] Support counters, gauges, histograms, and up-down counters.
- [x] Add automatic HTTP request spans. (SDK export assertions and parent-context tests)
- [x] Add request/response metrics. (full response lifetime/count/status/active/measured body bytes, including streaming failures and cancellation)
- [x] Add auth failure telemetry.
- [x] Add error correlation telemetry. (correlation in spans/logs, excluded from metric dimensions)

## Phase 8 - WebSockets And AsyncAPI

### `forklaunch-ws`

- [ ] Implement typed WebSocket server wrapper.
- [ ] Implement typed WebSocket client wrapper.
- [x] Validate incoming and outgoing messages. (`WebSocketContract::validate_incoming`/`validate_outgoing`)
- [x] Support client messages, server messages, ping, pong, error messages, and close reason schemas. (`WsMessageKind` + error/close schemas)
- [ ] Support channel and operation metadata.
- [x] Support automatic JSON encoding and decoding. (`WebSocketEnvelope::to_json`/`from_json`)
- [ ] Add integration tests for server/client validation.

### `forklaunch-asyncapi`

- [x] Generate AsyncAPI 3.0 from event contracts. (`generate_asyncapi_json`)
- [x] Support channel grouping.
- [x] Support client-to-server and server-to-client operations.
- [x] Add snapshot tests for AsyncAPI output.
- [ ] Generate TypeScript WebSocket clients.
- [ ] Generate Rust WebSocket clients.

## Phase 9 - Infrastructure Crates

### Redis Cache

- [x] Define `TtlCache` trait.
- [x] Implement Redis-backed TTL records. (`redis-client` adapter; binary/expiry/batch/prefix/FIFO integration checks)
- [x] Implement batch put/read/delete/peek.
- [x] Implement list keys by prefix.
- [x] Implement queue enqueue/dequeue/peek operations. (`InMemoryRedisQueue`)
- [x] Implement batch queue operations.
- [x] Add compliance-aware encryption/decryption hooks. (`CacheCodec` + `put_encrypted`/`get_decrypted`)
- [x] Add Redis container integration tests. (isolated local container and Redis service in CI)

### S3 Object Store

- [x] Define `ObjectStore` trait.
- [ ] Implement S3-backed put/read/delete. (in-memory reference impl done; `aws-sdk` feature wiring pending)
- [x] Implement batch operations.
- [x] Implement stream upload/download. (`StreamingUpload` + `download_chunks`)
- [x] Implement bucket initialization.
- [x] Add compliance-aware encryption/decryption hooks. (`ObjectCodec` + `put_encrypted`/`get_decrypted`)
- [ ] Add MinIO/S3-compatible testcontainer tests.

### Persistence

- [x] Define persistence extension points for `sqlx`. (optional PostgreSQL feature; tenant-bound transactions)
- [x] Implement field encryption helpers. (TypeScript-compatible v1–v4 envelopes, tenant derivation, key rotation, blind indexes; actual TypeScript interoperability tests)
- [x] Implement tenant context helpers. (validated identity and transaction-local database context)
- [x] Implement RLS setup helpers for Postgres. (safe DDL identifiers, FORCE RLS, read/write policies)
- [ ] Implement retention policy enforcement hooks.
- [ ] Add compliance metadata derives where appropriate.

## Phase 10 - `blueprint-rs` Workspace Scaffold

- [x] Create `blueprint-rs/Cargo.toml` workspace.
- [x] Add shared lint, format, clippy, test, and docs commands.
- [x] Add README explaining relationship to TypeScript `blueprint`.
- [x] Add crate naming conventions.
- [x] Add feature flag policy for optional integrations.
- [x] Add sample app/service/worker crates for smoke testing.

## Phase 11 - `blueprint-rs` Core And Interfaces

### `forklaunch-blueprint-core`

- [x] Re-export common framework traits used by blueprints. (`pub use forklaunch_http as http`, `contract`)
- [x] Define initial blueprint error/result, module metadata, audit context, and pagination primitives.
- [x] Define shared entity, DTO, pagination, and API result primitives. (`Entity`, `Identifier`, `Timestamps`, `ApiResult`)
- [x] Define common auth/session DTOs. (`SessionContext`)
- [x] Define common billing DTOs. (`MoneyAmount`)
- [x] Define common worker/job DTOs. (`JobDescriptor`)
- [x] Define blueprint testing helpers. (`forklaunch_blueprint_core::testing`)

### IAM Interface

- [x] Define initial user, session, credentials, permission models, and IAM service trait.
- [x] Define user, organization, tenant, role, permission, session, and token models. (tenant modeled as `Organization`)
- [x] Define IAM service traits.
- [x] Define IAM route contracts. (`forklaunch_iam_interface::contracts::registry`)
- [x] Define IAM SDK generation fixtures. (`client-sdk` tests)
- [x] Generate TypeScript and Rust SDKs from IAM contracts. (`generate::iam_typescript_sdk`/`iam_rust_sdk`)

### Billing Interface

- [x] Define initial customer, money, plan, subscription, checkout session models, and billing service trait.
- [x] Define customer, subscription, plan, price, invoice, entitlement, usage, and webhook models.
- [x] Define billing service traits.
- [x] Define billing route contracts. (`forklaunch_billing_interface::contracts::registry`)
- [x] Define billing SDK generation fixtures. (`client-sdk` tests)
- [x] Generate TypeScript and Rust SDKs from billing contracts. (`generate::billing_typescript_sdk`/`billing_rust_sdk`)

### Worker Interface

- [x] Define initial job, receipt, stats, status models, and worker queue trait.
- [x] Define job, queue, retry policy, schedule, worker result, and distributed signal models.
- [x] Define worker traits.
- [x] Define worker event contracts. (`forklaunch_worker_interface::events::channel`)
- [x] Generate AsyncAPI where applicable. (`forklaunch_worker_interface::events::asyncapi_json`)

### Monitoring Interface

- [x] Define initial health status, metric, and monitoring sink models.
- [x] Define health, readiness, metrics, audit event, and telemetry DTOs.
- [x] Define monitoring route contracts. (`forklaunch_monitoring_interface::contracts::registry`)
- [x] Add reusable health/readiness handlers. (`liveness`, `aggregate_readiness`)

## Phase 12 - `blueprint-rs` Implementations

### IAM Base

- [x] Implement initial in-memory IAM service for interface and SDK smoke tests.
- [x] Implement base IAM service with `sqlx`. (optional asynchronous `PostgresIamService`)
- [x] Implement user and organization CRUD. (tenant-bound PostgreSQL storage and keyset pagination)
- [x] Implement role and permission resolution. (`roles_for_user` + `permissions_for_user`)
- [x] Implement persistent opaque session issuance, hashed token storage, expiry, revocation, password-change invalidation, and disabled-user checks.
- [ ] Implement JWT issuance, cookie/CSRF lifecycle, and OAuth/OIDC provider flows.
- [x] Expose async IAM read/session/repository traits and a reusable Axum adapter; wire PostgreSQL IAM into `init module --language rust -m iam-base` and application `--modules iam-base`.
- [x] Validate generated IAM through its typed TypeScript SDK: restricted runtime role, tenant isolation, restart persistence, logout revocation, graceful shutdown, source preservation, rename, and delete.
- [x] Update generated SDK artifacts and crate names when a Rust service is renamed (including IAM); reject occupied project/artifact destinations before mutation.
- [x] Implement tenant-aware queries. (explicit predicates plus transaction-local RLS)
- [x] Add embedded migrations and isolated PostgreSQL container tests with a non-superuser role, including pooled-connection context cleanup.

### Billing Base

- [x] Implement initial in-memory billing service for interface and SDK smoke tests.
- [ ] Implement base billing traits and storage.
- [ ] Implement customer/subscription/plan persistence.
- [x] Implement entitlement checks. (metered `has_entitlement`)
- [x] Implement usage recording. (`record_usage` + `recorded_usage`)
- [ ] Add migrations and testcontainers tests.

### Billing Stripe

- [x] Implement initial Stripe config, webhook event, and checkout adapter shape.
- [ ] Implement Stripe client integration.
- [x] Implement webhook verification. (`webhook::verify` — `t=...,v1=<hex>` HMAC-SHA256, constant-time)
- [ ] Implement subscription sync.
- [ ] Implement customer and checkout helpers.
- [ ] Add wiremock tests for Stripe interactions.

### Worker Implementations

- [x] Implement initial in-memory database, Redis, and Kafka queue adapters behind the worker trait.
- [x] Implement PostgreSQL-backed worker queue with tenant RLS, atomic batch enqueue, concurrent claims, renewable fenced leases, acknowledgments, fixed retry backoff, crash recovery, and failed-job retention.
- [x] Define an async leased-queue interface and validate the PostgreSQL adapter against a restricted-role database, including migration coexistence with IAM.
- [x] Wire PostgreSQL queues into new generated workers with polling, lease renewal, bounded operations, and graceful drain; validate actual process crashes, restart recovery, tenant isolation, and source-preserving lifecycle operations.
- [x] Convert existing example workers to PostgreSQL on explicit backend changes, preserving their handler and unrelated Cargo metadata.
- [x] Add bounded per-process concurrency settings to the shared runner and generated PostgreSQL/Redis workers; renew every active lease and drain all in-flight handlers on shutdown.
- [x] Add shared process-local pause/resume/drain controls with lease renewal, irreversible drain, retention coordination, and real Redis/PostgreSQL coverage.
- [x] Add atomic ordered batch claims for PostgreSQL and Redis with independent delivery capabilities, tenant fencing, concurrent-consumer coverage, and Redis selection-validation regression tests.
- [x] Add batch-handler settings and per-job batch failure results; preserve existing single handlers, renew all leases, cancel on lease loss, validate complete failure sets before acknowledgments, and exercise concurrency/drain/recovery against both durable backends.
- [x] Add optional worker HTTP health/readiness and bearer-authenticated pause/resume/drain endpoints; validate lifecycle, authorization, retention coordination, and graceful shutdown in generated Redis/PostgreSQL processes.
- [x] Wire durable Redis queues into generated worker binaries; preserve application handlers through sync, rename, and Redis/PostgreSQL conversion, and validate real Redis process lifecycle behavior.
- [ ] Wire durable Kafka queues into generated worker binaries.
- [x] Add tenant-bound dead-letter listing/requeue and explicit age-based, bounded completed/failed retention APIs; expose generated administration commands with payload/token-free summaries. Validate races, tenant isolation, retry fencing, and legacy Redis terminal-index upgrades.
- [x] Add opt-in automatic retention scheduling and runtime policy settings for both durable backends; bound per-status purges and storage waits, stop maintenance during shutdown, and drain handlers on maintenance failure.
- [x] Implement Redis-backed worker queue. (`redis-client` async leased adapter: atomic mixed-queue batches/claims, persisted retry budgets, fenced ack/renew/fail, bounded expiry recovery, retained terminal jobs; real Redis concurrent-consumer and shared runner integration coverage. Generated Redis workers use the shared runner.)
- [ ] Implement Kafka-backed worker queue.
- [x] Define worker signaling interfaces and an in-memory reference bus. (`SignalBus` + `InMemorySignalBus`)
- [ ] Implement cross-process distributed worker signaling.
- [x] Add AsyncAPI generation for worker events where applicable. (`events::asyncapi_json`)

## Phase 13 - CLI And Template Integration

The existing CLI is already written in Rust. The Rust framework work should extend the current CLI rather than introduce a second CLI.

Current CLI structure to build on:

- Commands live under `cli/src/init`, `cli/src/openapi`, `cli/src/sdk`, `cli/src/sync`, `cli/src/release`, and `cli/src/deploy`.
- Templates live under `cli/src/templates`.
- Manifest structs live under `cli/src/core/manifest`.
- SDK support currently exists as `forklaunch sdk mode`; generation should become a first-class SDK subcommand family.
- OpenAPI export already exists for TypeScript services and should become language-aware.

### CLI Product Shape

- [x] Wire Rust and mixed project generation into the existing `forklaunch init` command family.
- [x] Add `cli/tests` coverage for Rust and mixed project generation plus Rust init, change, sync, delete, OpenAPI export, and SDK generation command paths.
- [x] Add dedicated Rust CLI parity scripts mirroring TypeScript command coverage for init, change, sync, delete, OpenAPI export, SDK generation, dry-run, custom-path, and Cargo workspace checks.
- [x] Add `ProjectLanguage` to the manifest model: `typescript`, `rust`.
- [x] Add application-level language mode: `typescript`, `rust`, `mixed`.
- [x] Add project metadata fields: `language`, `crateName`, `cratePath`, `contractArtifactPath`, `openapiPath`, `asyncapiPath`, `sdkTargets`.
- [x] Preserve existing TypeScript defaults so existing manifests parse without migration.
- [x] Add Rust-aware project dependency topology to the manifest.
- [x] Add Cargo workspace metadata for Rust projects.
- [x] Add PNPM workspace metadata only when TypeScript packages or TypeScript SDK output are present.

### Init Commands

- [x] Add `forklaunch init app <name> --language rust`.
- [x] Add `forklaunch init app <name> --language mixed`.
- [x] Add `forklaunch init service <name> --language rust`.
- [x] Add `forklaunch init worker <name> --language rust`.
- [x] Add `forklaunch init library <name> --language rust`.
- [x] Add `forklaunch init router <name> --language rust` if Rust route module generation remains separate from service generation.
- [ ] Add prompts for Rust package/crate name when the generated default is not enough.
- [x] Add prompts for SDK targets: TypeScript frontend SDK, Rust backend SDK, or both.

### Rust Templates

- [x] Add `cli/src/templates/rust/application`.
- [x] Add `cli/src/templates/rust/project/service`.
- [x] Add `cli/src/templates/rust/project/worker`.
- [x] Add `cli/src/templates/rust/project/library`.
- [x] Add `cli/src/templates/rust/router`.
- [x] Add `cli/src/templates/rust/blueprint/iam-base` as a symlink to `blueprint-rs`.
- [x] Add `cli/src/templates/rust/blueprint/billing-base` as a symlink to `blueprint-rs`.
- [x] Add `cli/src/templates/rust/blueprint/billing-stripe` as a symlink to `blueprint-rs`.
- [x] Add generated `Cargo.toml`, `src/lib.rs`, `src/main.rs`, `src/routes`, `src/domain`, `src/persistence`, `src/config`, and `tests` files.
- [x] Replace placeholder Rust crate output with runnable generated service, worker, library, router, and smoke-test implementations.
- [x] Add Rust Dockerfile templates.
- [x] Add Rust env example and config files.
- [x] Add Rust `.gitignore`, `rustfmt.toml`, and clippy config files.

### Template Rendering

- [x] Add Rust template namespaces under `cli/src/templates/rust` and SDK template namespaces under `cli/src/templates/sdk`.
- [ ] Reuse `generate_with_template` where practical.
- [x] Add Rust-specific generation variables: crate name, module path, lib name, binary name, Rust edition, SDK targets.
- [x] Avoid TypeScript AST transforms for Rust templates.
- [x] Render Rust app, service, worker, library, router, TypeScript SDK, and Rust SDK scaffolds through Ramhorn templates instead of inline stubs.
- [x] Add Rust-aware file preservation rules for user-edited code.

### Workspace Updates

- [x] Generate and update root `Cargo.toml` workspaces for Rust apps.
- [x] Generate service crate `Cargo.toml` files.
- [x] Add local path dependencies between Rust crates.
- [x] Add generated Rust SDK crates to the workspace when selected.
- [x] Keep PNPM/Bun generation for TypeScript SDKs and TypeScript projects.
- [x] Support mixed apps with both `Cargo.toml` and `package.json`/`pnpm-workspace.yaml`.

### OpenAPI And Contract Export

- [x] Make `forklaunch openapi export` detect project language from manifest.
- [x] For TypeScript projects, keep the existing TSX/openapi export path.
- [x] For Rust projects, run the Rust service binary in contract export mode.
- [x] Add Rust manifest/OpenAPI artifact fallback export for generated Rust projects.
- [x] Standardize output paths under `.forklaunch/openapi/<service>/openapi.json`.
- [x] Add fallback `.forklaunch/contracts/<service>/contract.json` artifacts for generated Rust projects.
- [x] Replace fallback Rust contract artifacts with binary-exported `framework-rs` contract registry output for generated Rust service and worker crates.
- [ ] Add `.forklaunch/asyncapi/<service>/asyncapi.json` for WebSocket/event projects.
- [x] Ensure release/deploy manifest generation reads TypeScript and Rust outputs uniformly.

### SDK Commands

- [x] Add `forklaunch sdk generate --target typescript`.
- [x] Add `forklaunch sdk generate --target rust`.
- [x] Add `forklaunch sdk generate --target all`.
- [x] Add `forklaunch sdk generate --service <name>`.
- [x] Add `forklaunch sdk generate --from contract`.
- [x] Add `forklaunch sdk generate --from openapi`.
- [x] Add `forklaunch sdk watch --target typescript` for regenerating TS SDKs when Rust contract artifacts change.
- [x] Add `forklaunch sync` integration that regenerates TypeScript SDKs after Rust route or model changes.
- [x] Add a CI check mode that runs SDK generation and fails when generated TypeScript/Rust SDKs are stale.
- [x] Generate TypeScript SDKs into a stable `sdks/typescript` package path.
- [x] Generate Rust SDK crates into the Cargo workspace.
- [x] Generate TypeScript and Rust SDK contents from Ramhorn templates.
- [x] Generate SDK operations from Rust `framework-rs` contract artifacts when `--from contract` is selected.
- [x] Generate SDK response models and status-code response maps from Rust `framework-rs` contract schemas.
- [x] Add `forklaunch sdk generate --check` for stale generated SDK detection.
- [x] Add SDK generation to app init when SDK targets are selected.
- [x] Keep `forklaunch sdk mode --type live` as the TypeScript-only local live import mode.
- [x] Treat Rust services as generated-SDK services.
- [x] Ensure mixed apps expose one stable generated SDK import surface.

### Sync, Change, Delete

- [x] Extend `sync service`, `sync worker`, and `sync library` for Rust projects.
- [x] Extend `sync all` for Rust Cargo workspace projects.
- [x] Extend `change application` for Rust application metadata.
- [x] Extend `change service`, `change worker`, and `change library` for Rust project metadata.
- [x] Extend `change router` for Rust route module renames.
- [x] Extend delete commands to remove Rust manifest entries, route modules, crate directories, and Cargo workspace members.
- [x] Remove generated SDK references during Rust deletes when SDK generation lands.
- [x] Extend dependency detection to include Rust SDK imports and Cargo path dependencies.
- [x] Keep TypeScript AST transforms isolated from Rust project updates.

### Local Development

- [x] Generate `cargo run -p <service>` scripts or `just`/`make` tasks for Rust services.
- [ ] Add mixed dev orchestration for TypeScript and Rust services.
- [x] Add Docker Compose service entries for Rust services.
- [x] Add health check defaults for Rust services.
- [x] Add Rust test commands to app-level scripts.

### Release And Deploy

- [x] Add Rust build artifacts to release packaging.
- [x] Add Rust Docker build path.
- [x] Add Rust service metadata to release manifest generation.
- [x] Add Rust OpenAPI artifacts to platform upload.
- [x] Add Rust SDK artifacts to release outputs when selected.
- [ ] Verify ECS/container defaults work for Rust services.

## Phase 14 - Cross-Language Conformance

- [ ] Build a canonical contract fixture service in TypeScript.
- [x] Build the same canonical contract fixture service in Rust. (`forklaunch-blueprint-sample-service::iam_router` — runnable, contract-driven, serves OpenAPI)
- [ ] Snapshot OpenAPI outputs and compare normalized output.
- [ ] Snapshot AsyncAPI outputs and compare normalized output.
- [x] Run generated TypeScript SDK against the Rust service. (`cli/tests/rust_http_runtime.sh`; relocated app, live HTTP, graceful shutdown verified)
- [ ] Run generated Rust SDK against the TypeScript service.
- [x] Add CLI tests that generate Rust TypeScript/Rust SDKs and typecheck the generated TypeScript SDK entrypoint.
- [x] Add CLI tests that assert generated Rust and TypeScript SDK model shapes come from Rust contract schemas.
- [ ] Test validation parity for params, query, headers, body, multipart, binary, SSE, and WebSockets.
- [ ] Test auth parity for JWT, JWKS, Basic, HMAC, roles, permissions, and scopes.
- [ ] Test error response parity.
- [ ] Test telemetry presence where feasible.

## Phase 15 - Release Readiness

- [ ] Add full docs for `framework-rs`.
- [ ] Add full docs for `blueprint-rs`.
- [ ] Add examples for Rust services, Rust workers, and mixed TypeScript/Rust apps.
- [ ] Add migration guide for TypeScript users adopting Rust services.
- [ ] Add crate publishing pipeline.
- [ ] Add version compatibility matrix between TypeScript and Rust packages.
- [ ] Add performance benchmarks for HTTP, validation, SDK calls, Redis, S3, and WebSockets.
- [ ] Add security review for auth, HMAC, JWT, encryption, secrets, and generated SDKs.

## Initial Milestone Definition

The first useful milestone is not full parity. It is:

- [x] A Rust HTTP service using `framework-rs`.
- [x] Typed request/response structs with validation-mode metadata and OpenAPI output.
- [x] A generated TypeScript SDK package derived from the Rust service contract.
- [x] A generated Rust SDK crate derived from the same Rust service contract.
- [x] A minimal `blueprint-rs` IAM interface crate proving the blueprint style.
- [ ] CI snapshot tests for the generated OpenAPI and SDK output.

## Open Decisions

- [ ] Choose `aide` versus `utoipa` as the preferred OpenAPI helper, or keep both as optional emitters.
- [ ] Decide how much TypeScript-style coercion Rust should preserve.
- [ ] Decide whether Rust SDKs are generated from OpenAPI only or from the richer ForkLaunch contract artifact.
- [ ] Decide whether TypeScript SDKs are generated in the existing CLI package or inside `framework-rs` codegen.
- [ ] Decide whether `blueprint-rs` should support Better Auth parity directly or provide generic OAuth/JWT/session integration points.
- [ ] Decide whether persistence helpers should standardize on `sqlx` only or expose optional SeaORM adapters.
