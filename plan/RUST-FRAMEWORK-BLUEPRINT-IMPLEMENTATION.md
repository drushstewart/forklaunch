# Rust Framework And Blueprint Implementation Plan

## Purpose

Build a Rust-native ForkLaunch stack with two new workspaces:

- `framework-rs`: the Rust framework runtime, contracts, adapters, SDK generation, infrastructure traits, and testing utilities.
- `blueprint-rs`: Rust blueprint libraries that mirror the style of the existing TypeScript `blueprint` workspace: shared core primitives, interface crates, and implementation crates for IAM, billing, workers, monitoring, and generated service scaffolds.

The Rust stack should interoperate with the TypeScript stack through shared contract artifacts. A Rust service should be able to generate TypeScript SDKs for frontend consumers and Rust SDKs for service-to-service consumers.

## Active Task Queue (2026-10-10)

- [x] Merge `origin/main` (`fae3d3e06`) into `rohin/rust-framework` and resolve CLI API/manifest conflicts; combined CLI compiles. Fetched and rechecked on 2026-10-09: no newer `main` commits remain to merge.
- [x] Replace generated services' TCP stub with Axum and graceful shutdown.
- [x] Remove build-machine Cargo paths by bundling framework source in generated applications.
- [x] Fix SDK mode's missing path argument and sync discovery's optional-argument panic.
- [x] Revalidate the combined workspaces: latest CLI 955 tests pass; latest all-feature framework 114 and blueprint 63 tests pass. Formatting and all-feature Clippy checks pass in both Rust workspaces. Real Redis/PostgreSQL integration and six TypeScript encryption interoperability fixtures pass separately.
- [x] Complete the post-merge generation rerun: init, change, sync, delete, OpenAPI/SDK, and relocated-service HTTP/TypeScript SDK suites pass.
- [x] Validate a real optimized generated Rust service release build through the shared exporter, including framework contracts and OpenAPI artifacts.
- [x] Validate Rust/mixed release build order, shared export dispatch, SDK package builds, failure stops, mixed crate locations, and prebuilt isolation using the actual CLI with owned platform/command fixtures.
- [x] Validate real Cargo/PNPM mixed release builds with a generated Rust service/TypeScript SDK and a minimal tsx service export hook; reuse those artifacts with intentionally unbuildable source, preserving prebuilt isolation and bounded mixed crate paths.
- [x] Complete relocated generated-service container build, health/OpenAPI, and SIGTERM shutdown checks. The Docker capacity blocker is resolved; `rust_container_runtime.sh` passes locally and runs in CI. Builder/runtime both use Debian bookworm.
- [x] Add broker-backed Kafka and generated Kafka workers, real transaction/process-crash/group-fanout tests, CI broker coverage, all three durable-worker source-archive installation, and complete embedded source packaging (currently 145 files including OTLP/CAC/request-HMAC sources).
- [x] Wire generated Redis/PostgreSQL distributed controls with separate-process command/HTTP fanout, scoped isolation, graceful disconnect failure, explicit restart semantics, and packaged CLI generation checks. HTTP commands apply through the ordered subscription to preserve command ordering.
- [ ] Validate generated ECS defaults, health checks, and service startup separately from the local container checks. Read-only audit of local `forklaunch-platform` at `3fb59b3cc` found a concrete integration gap: deployment/image planning maps non-Bun runtimes (including Tokio) to PNPM, and generated ECS commands invoke JavaScript start scripts. Rust needs native binary startup and image/build support in the platform before this gate can pass; local container success does not establish ECS parity.
- [x] Keep generated Rust SDKs for empty worker HTTP contracts free of unused Serde imports, path helpers and client-field warnings; compile packaged PostgreSQL/Redis worker SDKs with warnings denied.
- [x] Verify the CLI Cargo package file list includes all 129 embedded framework and blueprint source/template files; use `.template` manifest names to avoid Cargo excluding nested packages, and add a CI regression check.
- [ ] Finish mixed-app orchestration and one shared SDK surface (phases 13–15).
- [ ] Finish AsyncAPI export and cross-language contract conformance checks (phases 13–15).
- [x] Install the CLI from its Cargo source archive outside the checkout, generate and relocate service/PostgreSQL/Redis workers, compile them, and export real framework contracts/OpenAPI plus both SDK targets; run the regression in the CLI build CI job.
- [ ] Validate registry publication and installation of published crate versions (phases 13–15). Source-archive installation is checked; no crate has been published during this port.

PostgreSQL IAM is wired into generated modules. General service storage, auth/provider lifecycle and billing remain explicit tasks in phase 12. Kafka has a broker-backed implementation and generated runtime coverage; additional TypeScript module families are tracked below.

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
- [x] Add generated OTLP provider/exporter bootstrap. Generated services/IAM/all workers own JSON logging, optional HTTP/protobuf or HTTP/JSON trace/metric/log providers, W3C propagation and blocking-pool shutdown flushing; actual receiver and generated HTTP SIGTERM tests pass. Offline exports run before initialization.
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
- [x] Export accurate durable-worker contracts without unmounted example HTTP routes or internal management endpoints; validate empty application HTTP SDKs against both generated backends. Empty contracts and durable-worker manifest fallbacks also stay empty during SDK generation instead of inventing health methods.
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
- [x] Preserve declared binary/file inputs as bytes even for ASCII, Unicode and zero-length sources in both Axum adapters; compare actual and declared media types consistently when charset/boundary parameters are present. Empty declared text and URL-encoded inputs also retain their body kind; missing required media headers still fail.
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
- [x] Use `rmcp` for a Streamable HTTP server behind `rmcp-server`, with bounded HTTP-router dispatch, per-request transport authentication, Host/Origin checks and generated-service wiring.
- [x] Support per-route MCP opt-in and opt-out. (`route.options.mcp`)
- [x] Support transport authentication callback. (`McpAuthCallback`)
- [x] Support additional manually registered tools. (`McpToolRegistry::with_tool`)
- [x] Map input/output schemas from `schemars` metadata. (`inputSchema`/`outputSchema` from contract `SchemaRef`)

## Phase 6 - SDK Generation

### Shared SDK Codegen

- [x] Create `forklaunch-sdk-codegen` around the contract artifact and OpenAPI.
- [x] Resolve operation names consistently across TypeScript and Rust.
- [x] Keep generated service fallback operation names stable across export; isolate lifecycle fallback per service and refresh existing Rust as well as TypeScript SDKs on deletion. Preserve unaffected custom contracts and reject malformed artifacts explicitly.
- [x] Generate response models from JSON Schema for TypeScript and Rust SDKs.
- [x] Preserve nullable schemas across OpenAPI 3.0 and 3.1 forms, typed dictionary values, unconstrained arrays, and additional object fields in CLI SDKs; validate null/populated payloads through both artifact sources and live language fixtures.
- [x] Generate typed untagged Rust enums for oneOf/anyOf and multiple-type schemas, including nested arrays/objects and nullable roots; validate each object/scalar branch through both artifact sources and live language servers. Closed structured objects reject unknown fields; full schema-constraint validation remains server-side.
- [x] Generate TypeScript request field/body types from contract request schemas.
- [x] Preserve status-code-specific response typing.
- [x] Carry params, query, headers, body content type, response headers, and response bodies in `SdkGenerationPlan`.
- [x] Support params, query, headers, and JSON body transport in deterministic TypeScript and Rust SDK generators. Path substitution matches whole segments and percent-encodes UTF-8/reserved bytes consistently across both SDKs; conformance fixtures cover prefix collisions, recursive-looking values, and repeated parameters.
- [x] Decode text and binary download responses in CLI-generated Rust and browser TypeScript SDKs, including UTF-8 and arbitrary bytes; validate both artifact sources against live Rust and compiled TypeScript servers.
- [x] Transport text and raw binary request bodies through CLI-generated TypeScript and Rust clients using the contract media type; validate UTF-8, arbitrary bytes, nonzero binary view offsets, and JSON regressions against both artifact sources and live language servers.
- [x] Transport URL-encoded requests through both CLI SDKs and artifact sources: TypeScript form records retain Unicode and encode composite values as one JSON string per field; explicit URLSearchParams can carry repeated values. Rust sends caller-encoded strings with the form MIME. Live signed Twilio callbacks validate both clients. OpenAPI named primitive schemas now retain string/boolean/integer/number types instead of defaulting to objects.
- [x] Preserve request body kinds/media in framework SDK plans; generate raw text, byte/file and JSON requests in native Rust builders and TypeScript fetch clients. TypeScript also sends FormData with actual browser boundaries and URLSearchParams; validate bytes and MIME against a live HTTP server in CI, and compile/run Rust builders with warnings denied.
- [x] Support multipart/file-form uploads in generated CLI TypeScript/Rust SDKs through contract and OpenAPI artifacts, with standard parsers, repeated/binary/empty files and encoder-owned boundaries.
- [ ] Finish multipart response decoding and automatic typed runtime multipart extraction beyond explicit Axum extractors.
- [x] Generate bounded, pull-driven SSE response clients from both contract and OpenAPI artifacts, with typed Rust envelopes, iterator/abort/drop cancellation, auth before work, standard framing, terminal errors and live HTTP/MCP startup/shutdown verification.
- [x] Generate framework-native Rust HTTP clients with typed status decoding, guarded recursive models, Serde presence rules and bounded SSE through the shared CLI compiler/template; validate live presence, recursion and SSE with HTTP/MCP startup/shutdown.
- [x] Generate framework-native TypeScript typed HTTP/status/business-error responses, request schemas, omission/null models and bounded SSE using the shared CLI compiler/runtime; validate actual media/status handling and live presence/recursive/SSE clients with HTTP/MCP startup/shutdown.
- [ ] Complete remaining SSE emitter/helper/schema-validation conformance against TypeScript.
- [x] Support empty success bodies, including 204/205 and HEAD, with TypeScript `undefined` and Rust `()`; preserve explicit JSON null and validate both artifact sources against live Rust and compiled TypeScript servers.
- [x] Preserve every explicit 2xx response in TypeScript discriminated unions and Rust status enums, including distinct typed JSON models and 204/205 variants. Validate both SDKs and artifact sources against live Rust and compiled TypeScript servers; reject duplicate status/model names before generation, and avoid inventing success responses for error-only contracts.
- [x] Preserve declared business-error payloads and media types in CLI SDK imports and clients, overriding default error status types with contract-specific JSON/text/binary shapes. Schema-free JSON uses unknown/serde_json::Value.
- [x] Support guarded recursive schemas in CLI SDK imports/clients through contract and OpenAPI artifacts, preserving bounded reference processing and explicit unsupported-reference errors.
- [x] Share the bounded local schema graph/model compiler between CLI and both framework-native HTTP generators.
- [x] Bring the framework-native TypeScript generator to guarded recursive model parity through the shared bounded compiler and strict/live recursive fixtures.
- [ ] Complete remaining schema-constraint/emitter conformance.
- [ ] Resolve schema resource IDs, anchors and dynamic references in SDK imports.
- [x] Preserve Rust model omission versus explicit null and reject missing required nullable fields, with 95 actual TypeScript validator cases through both SDK artifact formats and live Rust/TypeScript HTTP and MCP.
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
- [x] Support binary downloads as Uint8Array in browser TypeScript clients.
- [x] Support browser file uploads through FormData, Blob and ArrayBuffer, preserving byte-view bounds and encoder-owned multipart boundaries.
- [ ] Add optional TanStack Query helpers as a later feature.
- [x] Add TypeScript SDK tests against a relocated generated Rust HTTP server (`rust_http_runtime.sh`).

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
- [x] Generate typed response model structs from contract JSON Schema, preserving exact JSON property names through Serde rename attributes (including camelCase, keywords, punctuation, and escaped names). Allocate distinct deterministic field and nested-model names after normalization; validate collision payloads through both SDKs, artifact sources, and live language fixtures.
- [x] Generate response enums by status code.
- [x] Support auth hooks for JWT, HMAC, Basic, and custom headers.
- [x] Support Rust-native error enums.
- [x] Run generated Rust SDKs against a relocated generated Rust HTTP server, including typed models, concurrent requests, HTTP errors, and transport failures (`rust_http_runtime.sh`).
- [x] Run generated Rust SDKs against a compiled TypeScript HTTP wire fixture, checking typed payloads, query/header transport, concurrent calls, HTTP errors, and transport failures. Full TypeScript framework conformance remains in phase 14.

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

- [x] Implement typed WebSocket server wrapper. (`TypedWebSocketServer`, authenticated upgrade callback, application-owned listener; optional `transport` feature)
- [x] Implement typed WebSocket client wrapper. (`transport::connect`, caller headers/subprotocols and TLS roots)
- [x] Compose typed Axum WebSocket upgrades with HTTP/MCP on one listener; preserve subprotocol and failed-upgrade configuration, validate frame/message bounds, and verify application-owned shutdown.
- [x] Validate incoming and outgoing messages. (`WebSocketContract::validate_incoming`/`validate_outgoing`)
- [x] Support client messages, server messages, ping, pong, error messages, and close reason schemas. (`WsMessageKind` + error/close schemas)
- [x] Support channel and operation metadata. (`SocketContract`/`SocketSchemaEntry` preserve TypeScript singular/plural precedence, channel fan-out, error/control/context/user-data metadata and derived framework schemas)
- [x] Compile bounded raw JSON Schema codecs for generated-client validation before serde field removal, with recursive/scoped references, explicit AJV email/UUID formats, exact decimals and disabled external retrieval.
- [x] Support automatic JSON encoding and decoding. (`WebSocketEnvelope::to_json`/`from_json`)
- [x] Add integration tests for server/client validation. (real native peers, bidirectional actual TypeScript peers, 168 Zod payload comparisons and 4,104 raw JSON Schema codec comparisons, frame/control byte limits, stalled handshake/write deadlines, cancellation and close handshakes)

Native typed runtime uses raw UTF-8 string / JSON payloads matching the TypeScript wrappers; the legacy tagged `WebSocketEnvelope` contract remains a separate protocol. `PayloadCodec::framework()` optionally applies serde/garde constraints. Invalid incoming native payloads close with 1008 before delivery; outgoing failures remain local. Control replies preserve exact bytes and validate the outgoing pong schema. Actual TypeScript tests also fixed the wrapper's automatic pong/close paths, which previously revalidated encoded Buffers as objects. Schema coercion follows Rust's documented stricter policy. Optional Axum `websocket::upgrade` now shares the native typed engine on existing HTTP/MCP listeners, with pre-upgrade authorization, subprotocol negotiation, bounded invalid/oversized input, and explicitly propagated connection shutdown tested together. Partial-write cancellation releases the socket. Raw socket schema metadata and checked AsyncAPI export are now implemented; generated Rust clients now share these codecs; generated TypeScript clients now use the checked raw-schema profile too; CLI `--from socket-contract` now generates both clients and checked AsyncAPI from versioned multi-socket artifacts, with service selection, drift checks, bounded native compilation and custom-path watch regeneration. Service-registry socket exports now run through Rust contract export, with checked names, custom paths and ownership-aware move/removal cleanup. External AsyncAPI imports remain open.

### `forklaunch-asyncapi`

- [x] Generate AsyncAPI 3.0 from event contracts. General `AsyncApiDocument::to_json` now returns a checked result with a channel map, grouped messages, separate send/receive operations, actual supplied payload/header schemas, and conflict errors.
- [x] Add checked typed WebSocket AsyncAPI generation with actual envelope discriminators, supplied schema components, declared error/close controls, local reference checks, and official AsyncAPI parser conformance in CI. Legacy `generate_asyncapi_json` retains placeholder payload schemas.
- [x] Export raw typed socket AsyncAPI without event envelopes, with document options, schema relocation, literal-safe reference handling, bounded schema traversal and explicit conflict/scope rejection. Compare 34 actual TypeScript/native documents against the official JSON Schema; 63 parser cases also pass, with eleven documented parser limitations for percent/prototype map keys and literal `$ref` data. Fix actual TypeScript reference escaping, special map keys, invalid channel addresses and boolean-schema object form.
- [x] Support channel grouping.
- [x] Support client-to-server and server-to-client operations.
- [x] Add snapshot tests for AsyncAPI output.
- [x] Integrate versioned multi-socket artifacts with CLI SDK generation/check/watch, portable bundled generators, both workspace/package formats, checked AsyncAPI aggregation and all-service validation before writes. (relocated strict consumers/live peers, parser, custom paths, containment and generated HTTP/MCP startup)
- [x] Generate TypeScript WebSocket clients. (`generate_typescript_websocket_sdk_source`: Node ws/AJV raw JSON profile, strict ESM/CommonJS declarations and recursive/empty consumers, actual TS/native peers, bounded queues, write/close deadlines, cancellation and owned iterators)
- [x] Add declared Draft 6 support to generated TypeScript socket codecs, with an embedded official meta-schema, typed ESM/CommonJS consumers and native conformance comparisons.
- [x] Add declared Draft 4 support to generated TypeScript socket codecs, preserving older keyword/model semantics: ignore newer AJV adapter keywords and model literals, validate boolean exclusive bounds, reject unsupported legacy `id` scopes, and compile both module formats plus portable CLI/archive consumers.
- [x] Align safe integral decimal/exponent spellings with JavaScript JSON numbers in compiled socket codecs before validation/typed integer deserialization, preserving negative zero for floating-point/JSON-value targets and rejecting string/fraction coercion. Generic serde codecs retain their stricter policy.
- [x] Preserve declared non-Draft-7 JSON Schema dialects in both raw AsyncAPI exporters through Multi Format Schema Objects, including reference relocation inside the wrapper, official-schema/parser conformance and rejection of legacy Draft 4 resource scopes. Draft 6 codecs ignore newer conditional keywords.
- [x] Preserve ignored Draft 4/6 keyword annotations through native/TypeScript AsyncAPI normalization, schema relocation and SDK model graphs, including scalar annotations, untouched reference data, 34 metadata documents and 4,104 codec comparisons.
- [x] Prevent AJV resource discovery from treating ignored legacy annotation IDs as active resources. On a private compilation copy, retain explicitly referenced schemas in annotation storage, recursive/local targets, property names and literal IDs in active const data; strict ESM/CommonJS fixtures, all codec cases and portable generation/rename/HTTP/MCP startup pass.
- [x] Follow explicit local schema references into legacy annotation storage during relocation and checked AsyncAPI export, including nested/recursive edges and once-decoded percent-encoded pointers. Preserve unreferenced literals and embedded resource bases, bound traversal to 64 steps/100,000 locations, and independently validate exported payload constraints with AJV and the official schema/parser.
- [x] Decode URI fragments once in shared SDK model graphs and CLI OpenAPI reference objects; canonicalize equivalent recursive edges and URI-encode relocated pointers after JSON-pointer token escaping. Exercise Unicode/slash/tilde/percent definition names in both native generators and portable CLI socket artifacts.
- [x] Extract standalone schemas from containing documents with transitive/recursive local reference copies, original literal data, UTF-8/percent pointer handling, collision-checked definitions and shared copy/output limits. Reject external/missing targets, resource IDs/dynamic scope and dictionary aliases; this is the schema foundation for external AsyncAPI input, whose transport/direction integration remains open.
- [ ] Bundle schema-dictionary aliases with separate interpretation contexts when the same value acts as both a schema and literal assertion data. Checked socket exports currently reject these references to preserve constraints.
- [x] Align checked schema reference profiles by rejecting encoded pointer separators before model/artifact generation. Preserve literal assertion/annotation data, percent-encoded names and general OpenAPI reference-object URI decoding. Actual/native/generated codec comparisons include rejected leading separators; imported SDK validation checks no-write behavior.
- [x] Align raw native JSON Schema codec resolution of encoded interior separators with actual AJV token decoding outside the checked SDK/export profile. An isolated, provenance-pinned resolver decodes each URI token once at lookup and registry-discovery boundaries, preserving internal pointers, literals, recursion and embedded resource IDs. Default upstream RFC behavior remains independently tested. Checked SDK/export/import profiles continue rejecting this ambiguous extension.
- [x] Match actual AJV validation of constraints beside `$ref` in Drafts 4–7 through a separate isolated opt-in validator feature, preserving dialect-specific keywords and original schema documents. All five supported dialects pass 12,960 actual helper/AJV codec comparisons, including interior separators, invalid escapes, annotation targets, recursive and scoped resources, sibling constraints and unchanged literal assertions.
- [x] Bundle explicitly referenced schemas stored inside const/enum/default/example data without mutating the original literal payload constraints. Share collision-checked copies and recursive edges with bounded common relocation, preserve embedded resource bases, retain idempotence, and validate full original output with independent AJV plus structural parser checks. Portable multi-socket fixtures compile both language models.
- [ ] Carry explicit raw binary payload metadata through checked socket contracts and both generated clients.
- [x] Exercise and harden actual TypeScript automatic control-reply failures and abnormal closes with required pong/close schemas. (one error and 1008 before typed delivery, five-second unresponsive-peer close bound, preserved manual validation and explicit autoPong=false; live CI fixture and 110 unit/integration tests)
- [x] Reject boolean `false` message schemas before typed delivery on actual TypeScript client and server sockets; preserve `true` decoding and absent-schema raw frames, with a reproduced live regression and outgoing validation checks.
- [x] Generate Rust WebSocket clients. (`SocketSdkPlan`, `generate_rust_websocket_sdk_source`: typed directions/controls, recursive unions, retained upgrade responses, actual TypeScript server conformance, invalid peer rejection and warning-free recursive/empty consumer compilation)

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
- [x] Implement async S3-backed put/read/delete through the official AWS SDK, with namespace fences, byte/count/deadline limits, actual tags, ordered batches, and bounded paginated listings. Validate against an isolated MinIO container.
- [x] Implement batch operations.
- [x] Implement stream upload/download. (`StreamingUpload` + `download_chunks`)
- [x] Implement bucket initialization.
- [x] Add compliance-aware encryption/decryption hooks. (`ObjectCodec` + `put_encrypted`/`get_decrypted`)
- [x] Add MinIO/S3-compatible container integration tests and run them in framework CI; test permission-denied bucket initialization and deadlines separately.
- [x] Add SDK-signed S3 download links with bounded whole-second lifetimes, namespace/key fences, credential-resolution deadlines, and UTF-8 attachment filenames; verify actual download, expiration, and tampering rejection against MinIO.
- [x] Add native bounded S3 chunked downloads and exact-length byte-stream/file uploads without whole-object buffering; test actual file transfers, metadata, drop behavior, overflow, truncation, idle deadlines, and source errors. Legacy streaming helpers still buffer.
- [x] Add native S3 multipart transfers for unknown-length sources with bounded part buffering, CRC32 checksums, hidden-until-complete publication, source/byte/deadline bounds, and cancellation abort cleanup; validate real multiple-part, empty/exact-boundary, overflow, failure, timeout and cancellation behavior against MinIO.
- [x] Add presigned S3 POST upload policies, with exact key/type/byte bounds, bounded SDK credential resolution, temporary-token/rotation coverage, AWS signature vectors, actual TypeScript policy fixtures and live MinIO rejection tests.

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
- [x] Port configured IAM directory CRUD and grant surfacing: typed single/batch interfaces, transactional memory/PostgreSQL stores, relationship hydration and cleanup, encrypted sensitive fields, retained-key uniqueness claims and atomic actor audits. Real PostgreSQL fixtures cover rollback, cross-tenant reads, runtime-role checks, concurrent uniqueness, key rotation and ciphertext swapping. Direct actual TypeScript conformance covers 44 service operations, 30 routes and 20 request/response schema layouts. Correct upstream permission batch awaiting/transaction propagation and user batch ID filters.
- [x] Generate the thirty configured IAM directory HTTP/MCP operations alongside opaque sessions. Validate live typed Rust and TypeScript SDK calls, nested models and status unions. Verify request HMAC and JWKS JWT policy, role/permission checks, storage-resolved organization scope, atomic mixed-organization rejection and SDK generation. Preserve the legacy `/users/:id` route as `getSessionUser` so directory `/user/:id` retains `getUser` in SDK/MCP contracts.
- [x] Add configured IAM base discovery/compliance routes and actual compliance-policy conformance. Public discovery returns 204; system-role JWT export/erase return 404 because all configured entity scalars (including inherited base properties) are classified `none`. Execute the actual TypeScript compliance service to verify empty results without entity reads. All 33 configured routes and 22 request/response schema layouts match the actual sources; generated HTTP/MCP, both SDKs and lifecycle checks pass.
- [ ] Support application-specific directory compliance classifications and generic entity metadata. Encryption at rest must not implicitly select fields for compliance export/erase.
- [ ] Integrate directory identities with enabled authentication providers and complete generic mapper/filter extensions; directory CRUD passwords are deliberately ignored by the configured TypeScript mapper, and opaque credential identities remain separate.
- [ ] Implement JWT issuance, cookie/CSRF lifecycle, and OAuth/OIDC provider flows.
- [x] Expose async IAM read/session/repository traits and a reusable Axum adapter; wire PostgreSQL IAM into `init module --language rust -m iam-base` and application `--modules iam-base`.
- [x] Validate generated IAM through its typed TypeScript SDK: restricted runtime role, tenant isolation, restart persistence, logout revocation, graceful shutdown, source preservation, rename, and delete.
- [x] Update generated SDK artifacts and crate names when a Rust service is renamed (including IAM); reject occupied project/artifact destinations before mutation.
- [x] Implement tenant-aware queries. (explicit predicates plus transaction-local RLS)
- [x] Add embedded migrations and isolated PostgreSQL container tests with a non-superuser role, including pooled-connection context cleanup.

### Billing Base

- [x] Implement initial in-memory billing service for interface and SDK smoke tests.
- [x] Implement the five asynchronous base billing service traits and typed DTOs with owned memory and tenant-RLS PostgreSQL storage. Plans/subscriptions are durable; checkout/payment-link/portal sessions use TTL cache primary storage with optional database backup. Actual TS base-class conformance covers 168 operations over explicit generic mapper/cache/ORM boundaries. Native fixtures cover concurrent disjoint updates, unique provider IDs, real cache expiry independent of provider expiry, encrypted provider data/session URIs, audit rollback, cross-tenant isolation, key rotation and reopening. Configured Stripe mappers/provider integration and generated modules remain separate gates.
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
- [x] Wire broker-backed transactional Kafka into generated worker binaries; validate explicit topic provisioning, non-mutating peek/dead-letter inspection, retries, source offsets, actual SIGKILL recovery, authenticated HTTP/Redis distributed controls, source preservation, rename, backend conversion and delete against an isolated broker.
- [x] Add tenant-bound dead-letter listing/requeue and explicit age-based, bounded completed/failed retention APIs; expose generated administration commands with payload/token-free summaries. Validate races, tenant isolation, retry fencing, and legacy Redis terminal-index upgrades.
- [x] Add opt-in automatic retention scheduling and runtime policy settings for both durable backends; bound per-status purges and storage waits, stop maintenance during shutdown, and drain handlers on maintenance failure.
- [x] Implement Redis-backed worker queue. (`redis-client` async leased adapter: atomic mixed-queue batches/claims, persisted retry budgets, fenced ack/renew/fail, bounded expiry recovery, retained terminal jobs; real Redis concurrent-consumer and shared runner integration coverage. Generated Redis workers use the shared runner.)
- [x] Implement a broker-backed Kafka producer/consumer with atomic batch publication, offset/retry/dead-letter transactions, namespace/tenant/queue/group scoping, bounded payloads and operation deadlines, explicit consumer-session/handler/drain deadlines, TLS/SASL configuration, stable replica fencing and fail-closed assignment handling. Validate group fanout, offset persistence, pause/drain abandonment recovery, tenant isolation and producer fencing against a pinned ARM64/AMD64 Redpanda broker; run in CI. Kafka uses log/partition semantics rather than the database lease API.
- [x] Validate a generated Kafka worker's Linux container build/startup against the isolated broker, including bundled librdkafka/OpenSSL. Relocated Linux image passes offline export, migration, enqueue/processing/inspection, health/controls and SIGTERM; builder includes the libcurl headers required by upstream native compilation.
- [x] Define worker signaling interfaces and an in-memory reference bus. (`SignalBus` + `InMemorySignalBus`)
- [x] Implement namespace/tenant/queue-bound Redis Pub/Sub worker signaling with ordered bounded subscriptions, sanitized publication deadlines, independent publishers and sticky drain forwarding. Verify actual child-process delivery, isolation, overflow/malformed-message errors and owned TCP disconnect behavior in the Redis CI suite.
- [x] Add subscriber-free administrative Redis signal publishers and `http::run_with_control` so distributed forwarding, HTTP readiness and durable consumption share one control; test paused jobs, resume completion, readiness changes and graceful drain against real Redis.
- [x] Wire opt-in distributed signaling into generated Redis/PostgreSQL worker commands and authenticated control endpoints. Validate separate-process fanout, namespace/tenant/queue isolation, queue-storage-independent administration, paused jobs, resume and drain against both backends. Subscription failure drains active handlers and exits with an error; explicit restart begins running without replay or automatic desired-state restoration.
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
- [x] Add `.forklaunch/asyncapi/<service>/asyncapi.json` for raw WebSocket contract artifacts through `sdk generate --from socket-contract`, with custom project output paths.
- [x] Export socket artifacts automatically from generated service registries, with checked module names, service title/version, custom contained output paths, unchanged-owned move/removal cleanup and preserved manual/edited artifacts. Relocated CLI and archive fixtures generate both SDKs from registered metadata and validate HTTP/MCP startup.
- [x] Import explicit AsyncAPI 3.0.0 JSON WebSocket server documents through native/CLI generation/check/watch, offline official schema validation, bounded local-reference extraction, server/client direction mapping, disjoint message checks, input ownership and SDK rename/pruning. Relocated strict consumers/live peers and HTTP/MCP startup validate the supported profile.
- [ ] Extend external AsyncAPI imports to event projects, other payload dialects/protocols, JSON string wire encoding, upgrade authentication/parameters, traits/replies and richer message matching.
- [x] Prune unchanged obsolete socket modules during regeneration, with per-package hashes, read-only drift checks, all-service staging, preserved edits/untracked files/compiler options and symlink containment.
- [x] Retire owned socket SDK files/AsyncAPI after an exported last-socket removal or explicit service deletion, removing Cargo members and empty directories while preserving edited/untracked files and compiler options. Live watch requires prior registry provenance plus a fresh empty framework contract, survives transient missing artifacts and restores both packages when sockets return. Shared pnpm package globs remain available for retained user packages.
- [x] Rename socket artifacts and owned SDK packages with their service, preserving compiler options and retaining edited/untracked files at old SDK locations. Default/explicit default paths follow the new name, custom paths retain their location, occupied destinations reject before source mutation, strict consumers compile and renamed HTTP/MCP startup passes. Deletion also removes unchanged owned custom registry artifacts.
- [x] Ensure release/deploy manifest generation reads TypeScript and Rust outputs uniformly.

### SDK Commands

- [x] Add `forklaunch sdk generate --target typescript`.
- [x] Add `forklaunch sdk generate --target rust`.
- [x] Add `forklaunch sdk generate --target all`.
- [x] Add `forklaunch sdk generate --service <name>`.
- [x] Add `forklaunch sdk generate --from contract`.
- [x] Add `forklaunch sdk generate --from openapi` reading actual OpenAPI 3.0/3.1 JSON artifacts, operation IDs, parameters, JSON body models, and bounded document-local references; artifact edits invalidate check mode, and missing/unsupported artifacts fail before output writes.
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
- [x] Run generated Rust SDK against the compiled TypeScript HTTP wire fixture, including typed JSON, statuses, text/binary uploads and multipart files through both contract and OpenAPI generation. Full TypeScript framework service conformance remains separate.
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
- [x] Generate Rust SDKs from both OpenAPI and the richer ForkLaunch contract artifact; verify equivalent live HTTP behavior from both formats.
- [x] Generate TypeScript SDKs through both the CLI and framework-native API using shared model compilation, templates and fetch/SSE runtimes.
- [x] Support configured Better Auth behavior directly in the isolated native `forklaunch-iam-better-auth` crate, with encrypted tenant storage and a raw HTTP namespace alongside generic IAM/session integration. Remaining behavior and CLI-generation gates are tracked below.
- [ ] Decide whether persistence helpers should standardize on `sqlx` only or expose optional SeaORM adapters.

## Additional TypeScript Module Coverage Required For Full Parity

The initial phases cover only part of the repository's published TypeScript surface.
These packages are part of the full-parity target as well; their absence must not
be hidden by completed framework/blueprint scaffold checks. Audit concrete exports,
contracts and TypeScript behavior, then implement idiomatic Rust equivalents and
validate generated modules and SDKs. Runtime-specific JavaScript adapters do not
require literal Rust clones, but their public request/response behavior does.

- [ ] Inventory every published framework/blueprint package and map its public runtime, provider, service, DTO, contract and generated-module capabilities to Rust implementations and conformance fixtures. The 44-package/module inventory is recorded in [RUST-TYPESCRIPT-PARITY-INVENTORY.md](RUST-TYPESCRIPT-PARITY-INVENTORY.md); detailed symbol/DTO/fixture mapping remains.
- [x] Complete messaging interface/base persistence, generated SMS module and Twilio delivery/status-callback integration with provider fixtures and tenant isolation. (`blueprint/interfaces/messaging`, `blueprint/implementations/messaging/{base,twilio}`, generated messaging modules) Actual TypeScript base/Twilio fixtures cover DTOs, all 16 status cases, dispatch forms, callbacks and error clearing. Generated Rust/mixed modules pass both SDKs through contract and OpenAPI imports, JWT/HMAC failures, signed-form tampering, restricted-role RLS/encryption/auditing, restart persistence, compliance and init/rename/sync/delete. SMS storage uses native Rust migrations; shared MikroORM schema and generic compliance/retention wrappers remain separate tasks.
- [x] Port all five billing interface service families and base persistence/cache lifecycle. The 168-operation actual TS base-class comparison and native memory/PostgreSQL tests pass; provider ID/party lookups use indexed tenant-scoped queries, and metadata-only ID lookup avoids decrypting provider data. Native responses retain complete typed DTOs when a generic mapper would otherwise return partial portal data; configured Stripe mappers and Redis cache interoperability remain separate.
- [x] Port Stripe billing authenticated transport and five asynchronous provider services over tenant-bound base persistence. Twenty-one official Stripe 22.6.2 transport comparisons and 144 actual TS wrapper operations pass, including plan replacement, subscription item updates, nonresumable cancellation errors, session IDs/expiry, payment-link metadata, provider failures and cache/backup lifecycle. Native checks enforce timeout/response/form bounds, redaction and redirect refusal. Shared raw-body verification retains 20 SDK signature comparisons and adds stale billing callback rejection; the synchronous reference adapter now fails closed instead of fabricating provider results. Fixtures use owned generic mapper/storage boundaries, so configured application mappers remain a separate gate.
- [x] Define the thirty shared billing application contracts for both variants. Sixty-six actual TS route comparisons (including compliance and Stripe callbacks) and 28 actual configured DTO schema comparisons pass, including HMAC versus admin/JWKS portal auth, canonical paths, params/query/body, response statuses, required fields, UUID base IDs and provider-specific enums/`stripeFields`. Legacy customer/invoice reference contracts remain compatible under their existing module; generated billing uses the new module registry. Callback/platform contracts are covered by the webhook task below.
- [x] Generate billing-base and billing-stripe modules with 33 HTTP/MCP routes each (five service families, compliance and health), configured enums and top-level `stripeFields` translation. Live generated TypeScript/Rust SDKs, HMAC and admin/system JWT roles, query lists, tenant isolation, encrypted/audited PostgreSQL data, restart persistence, disabled/custom-path MCP, graceful shutdown and init/sync/rename/delete fixtures pass. Fresh archive installation compiles both variants and validates service HTTP/MCP startup and contract/SDK exports. Current TS billing entities classify all fields as non-PII; compliance preserves system-role authorization and empty 404 results.
- [ ] Complete configured billing mapper semantic review and provider reconciliation. Generated party lookups explicitly configure lowercase application enum values; generic services retain uppercase defaults matching the actual TS base fixture. Native typed responses preserve complete IDs/fields where TS configured cache mappers drop them. Managed Stripe transport is covered below; mixed deployment remains a separate gate. Portal webhook expiry preserves the current TS seconds/milliseconds quirk, and payment-link callbacks preserve CREATED/UPDATED statuses outside the configured HTTP enum; resolve these explicitly during mapper review.
- [x] Implement Stripe billing webhook business synchronization for portal, checkout, payment-link, legacy plan, price, paginated product and subscription events. Twenty-six actual TS webhook state/provider-effect comparisons pass, including partial missing-plan updates, subscription party reassignment protection and missed-created-event recovery. Direct callbacks verify exact Stripe-signed bytes; platform payment callbacks bind instance HMAC credentials, platform key ID, path/body/time and atomic PostgreSQL nonces. Tenant-RLS metadata-only success receipts persist across restarts, reject event type reuse and serialize duplicate delivery; failed events retry. Admission limits preserve pool capacity for callback business writes. Native PostgreSQL deduplication/concurrency/retry/isolation/nonce/admission tests pass. Generated base/Stripe runtime gates now cover 33/35 HTTP/MCP routes, signed direct callbacks, platform key-ID/time/nonce failures, subscription updates without party reassignment, duplicate callbacks across restart, disabled callback credentials, both SDKs and CLI lifecycle. The generated signed MCP callback fixture exposed a framework transport bug: text bodies with application/json content type were JSON-encoded. MCP now dispatches by contract body kind and preserves signed bytes, with a dedicated regression test.
- [x] Port managed outbound Stripe transport through a reusable instance gateway client. Twenty-three actual TS createStripeClient/gateway-fetch comparisons verify gateway URL prefixes, instance-key HMAC over full query filters and exact form bytes, fresh nonces, products/prices and existing resources, provider failures and native redirect refusal. Direct transport retains 21 official SDK comparisons and all 144 service operations. Generic JSON capability requests have native signature/path/body/nonce/configuration checks. Generated services select complete trusted managed settings before vendor keys, accept the TS STRIPE_API_KEY name plus the existing STRIPE_SECRET_KEY alias, and carry optional gateway/callback settings through Compose. Generated restricted-role PostgreSQL runtime validates keyless managed HTTP/MCP startup and signed plan CRUD/list, local keys ignored in managed mode, incomplete managed settings rejected, both SDKs, callbacks, restart and CLI lifecycle. The full fixture passes.
- [ ] Finish interrupted billing provider/local-write and webhook receipt reconciliation and remaining configured mapper semantics. Plan replacement deletes the old provider plan before creating its successor, and provider calls precede local writes, matching TS; external effects are not transactionally atomic. Cancelled Stripe subscriptions cannot be resumed and provider errors leave local state inactive.
- [x] Port all ecommerce interface DTOs/traits and the six base services with PostgreSQL persistence. Actual TypeScript service conformance covers 208 operations and all 36 order-state pairs. Native memory/PostgreSQL fixtures cover stock/cart/order/payment concurrency, uniqueness, tenant RLS, encrypted/audited addresses, key rotation and audit rollback. Cache failures/timeouts fall back to storage; native integer/overflow checks preserve database limits.
- [x] Port ecommerce product-controller search and catalog reads: variant price/options/stock filters, explicit empty results, optional inventory, stable createdAt/id pages, SQL limit/offset and tenant isolation. Thirty-five actual TypeScript controller comparisons and memory/PostgreSQL fixtures pass. Native pagination uses integral limits/offsets; SQL/storage failures propagate instead of being treated as absent stock.
- [x] Port ecommerce order inventory event consumption and native leased-worker handler. Thirteen actual TypeScript event comparisons cover legal transitions, paid decrement, paid/fulfilled cancellation restock, pending cancellation, multi-item rollback and retry. Native memory/PostgreSQL fixtures additionally enforce positive quantities/legal transitions, atomic durable receipts, payload identity conflicts, tenant isolation and concurrent redelivery. A real PostgreSQL queue fixture verifies stock is unchanged on redelivery after an expired unacknowledged lease and reopening the store. Generated transition/event producer wiring remains below.
- [x] Define typed core ecommerce module contracts and provider-selecting creation DTOs. Twenty-seven actual TS controller/router metadata fixtures compare canonical methods/paths/names, HMAC, params/query/body and response statuses. Response-only Stripe secrets are explicit on creation and excluded from reads; checkout/import/webhook/compliance registries and live module wiring remain separate.
- [x] Add tenant-checked PostgreSQL queue enqueue inside an application transaction and atomic ecommerce order transition/event production. Restricted-role live tests cover queue-write and tenant failures rolling back order/audit changes, concurrent transition winners, real paid/fulfilled/cancelled worker delivery and stock restoration. Existing PostgreSQL worker lease/retry/isolation/runner fixtures pass after sharing the insert path.
- [x] Mount core ecommerce HTTP/MCP routes through the tenant-bound PostgreSQL adapter, audited request actors, Stripe/PayPal selection and atomic order/event production. Live TCP fixtures cover all 27 application routes plus health, provider wire requests, JSON query filters/pages, typed-body validation, response-only secrets, MCP discovery/signed tools/Origin rejection, RLS and real inventory delivery. Strict workspace tests/clippy and archive-installed adapter compilation pass; generated CLI lifecycle and remaining ecommerce controllers stay open below.
- [x] Generate the core ecommerce-stripe CLI module in Rust/mixed applications with provider configuration, offline contracts, HTTP/MCP startup, both SDKs and a separate inventory worker. Live generated fixtures verify Stripe/PayPal wire requests, HMAC/MCP authorization, RLS, encrypted/audited orders, paid/cancelled stock updates, worker controls, restart persistence, relocation and source-preserving init/sync/rename/delete.
- [x] Port Stripe Tax and flat-rate shipping calculators. Thirty-two actual TypeScript comparisons cover USD form fields, jurisdiction breakdown, omitted line2, free/domestic/international rates, seven-percent estimated fallback and integer rounding. Native checks reject negative/overflow amounts and flag malformed, oversized, negative and redirected provider responses as estimates without exposing provider bodies.
- [x] Port bulk catalog import with typed HTTP/MCP contracts and both SDKs. Nine actual TS controller operations cover external-ID upserts, optional-field preservation, variant reparenting, default shipping/stock and live-stock preservation. Generated PostgreSQL HTTP/SDK/MCP re-imports retain adjusted inventory and reject null stock; full CLI lifecycle and native restricted-role regression fixtures pass. Partial import failures remain resumable, matching TS sequential semantics.
- [x] Wire checkout through generated HTTP/MCP and both SDK schemas: stock/current variant prices, checked integer totals, Stripe Tax/explicit estimate, flat shipping, order/cart association, pending order/payment resume, provider switching, stale basket replacement and provider failure. Eight actual TS controller comparisons run against owned PostgreSQL/provider service boundaries. Cross-process races produce one order/payment/provider call; ten parallel carts finish without pool starvation. Tenant/namespace/drop-release lock tests, verified actor audit checks and complete generated lifecycle fixtures pass. Cart retention and cancelled-order provider-reference reconciliation preserve current TS behavior; webhook/capture guarantees remain below.
- [x] Generate ecommerce inventory worker Compose orchestration with the HTTP image, isolated runtime credentials, container readiness and module ownership. Sync preserves worker overrides; rename/delete follow the owning module and preserve sibling workers. Native generated lifecycle and actual Linux Compose fixtures pass with restricted-role PostgreSQL, HTTP/MCP discovery/authentication, paid/cancelled inventory delivery and SIGTERM for both processes.
- [ ] Complete ecommerce persistence compliance (encrypted address retention/export/erasure). The current TS server has no ecommerce-specific compliance controller; generic persistence hooks remain in scope. (`blueprint/ecommerce-stripe`)
- [x] Port ecommerce Stripe/PayPal native clients and payment services: PaymentIntent creation/resume, direct Connect fees, OAuth/Orders/signature verification, confirmation/failure and capture-claim rollback. Actual TS conformance covers 15 Stripe scenarios, 22 PayPal client cases and 19 wrapper operations; live provider fixtures validate wire requests, single capture, retry rollback, response bounds, credential redaction and response-only client secrets. Embedded source archives compile both providers.
- [x] Add durable provider webhook bookkeeping with tenant-RLS PostgreSQL storage and owned memory fixtures. Ten actual TS ledger operations verify new/unfinished/processed replay, provider-scoped identity and repeated completion. Native concurrent receipts have one fresh winner; persistence survives reopening, tenant/namespace isolation and changed-type identity rejection pass. Raw provider payloads are not persisted. This gate records processing state; provider authentication and serialized business handling remain below.
- [x] Add raw-body Stripe webhook authentication with constant-time HMAC verification, multiple signatures, secret redaction and the SDK replay window. Twenty official Stripe 22.6.2 SDK comparisons verify exact bytes, timestamp boundaries (including its future-time behavior), rotated signatures and malformed headers. Route/business processing remains below.
- [x] Mount and generate Stripe/PayPal callbacks with raw-byte/provider API verification, typed metadata and both SDKs. Thirty-one application contracts match actual TS metadata. Eleven actual TS controller comparisons cover paid/failed/mismatch, verification, replay and capture rollback/retry over restricted PostgreSQL/native provider boundaries; native checks additionally cover original PayPal event preservation, tenant isolation, cross-process simultaneous/same-payment events with one capture, public-route session independence, SDK/MCP guards and real inventory delivery. Full generated HTTP/MCP startup and source-preserving lifecycle checks pass. PayPal amount reconciliation requires exact nonnegative cents; malformed/fractional-cent amounts do not authorize paid transitions.
- [ ] Complete interrupted external capture and cancelled-order provider-reference reconciliation. Preserve TS behavior while documenting its provisional PayPal succeeded claim and failed-payment confirmation semantics; interrupted external capture needs explicit reconciliation before stronger financial guarantees can be claimed. (`blueprint/implementations/ecommerce/{stripe,paypal}`, `blueprint/ecommerce-stripe`)
- [x] Support JWT allowlists spanning RSA/EC/EdDSA key families: verify the configured policy and JWK metadata first, then narrow only the decode operation to its selected allowed algorithm. Regression and generated CAC JWKS tests pass.
- [x] Port TypeScript request-HMAC canonicalization and authorization headers with constant-time verification, normalized timestamps and optional clock-skew checks. Sixteen actual TypeScript signer/header-generator fixtures cover absent/text/buffer/raw-JSON/Unicode/timezone cases; atomic distributed nonce replay policy remains application-owned.
- [x] Add CAC provider DTOs/traits, internal HMAC-declared lookup contracts and the free mock provider. Actual TypeScript provider conformance passes for descriptor/licensing, all existing mock codes, exact matching and unknown codes. The authenticated runtime and generated CAC module are now validated separately below.
- [x] Port CAC code-set provider interfaces and the existing mock procedure-code provider, preserving descriptor/licensing metadata and lookup behavior; do not invent or bundle licensed clinical data. (`blueprint/interfaces/cac`, `blueprint/implementations/cac/base`, generated CAC module) Generated Rust/mixed modules now mount request-HMAC lookup and JWKS/system-role-protected empty-entity compliance routes, health and docs. Both SDKs, claim-policy failures, shared key caching, embedded-workspace upgrades, coexistence, source preservation, rename/sync/delete and source-archive compiler checks pass.
- [ ] Audit BullMQ producer/consumer options, retry/retention behavior, waiting/active inspection and tenant Redis-prefix/ACL handling; cover missing Rust worker behavior and cross-language interoperability requirements explicitly. (`blueprint/implementations/worker/bullmq`)
- [x] Audit the configured Better Auth 1.7.6 plugin and HTTP surface against actual source/library execution. Capture 70 HTTP route descriptors, four server-only helpers and 49 protocol checks with an owned UUID/SQL adapter boundary. Cover real discovery/surfacing controllers, signed/revoked session cookies, credentials and rotation, Origin/callback checks, EdDSA/JWKS claims, organization defaults, configured role denials and disabled flows. This reference audit does not imply native protocol or real-ORM parity.
- [x] Add isolated native Better Auth password/cookie primitives and direct bidirectional library comparisons. Scrypt matches N=16384/r=16/p=1, 64-byte keys, NFKC normalization and text salt encoding. Cookies match HMAC-SHA256, standard padded base64, JavaScript URI encoding, duplicate/quoted/malformed cookie parsing, secure names and 24-hour/transient/cleared options. All 322 comparisons pass, strict crate clippy passes, and source/templates are packaged. Endpoint/session persistence remains open.
- [x] Add atomic memory/encrypted PostgreSQL storage for all eleven configured Better Auth models. Independent schema migration ledger, forced tenant RLS, runtime-role checks, model/record-bound encryption, rotating blind unique indexes, shared/exclusive transaction locks and transactional audit records are implemented. Live PostgreSQL verifies rollback/read-only enforcement, tenant isolation, secrets absent at rest, signing-key restoration, key rotation and ciphertext transplant rejection. Auth endpoint behavior, relationship/cascade policy and indexed pagination beyond bounded 4096-row model scans remain open.
- [x] Audit actual configured session lifecycle beyond configuration values. Sixty checks cover exact one-hour refresh, refresh suppression, stable tokens/creation time, creation-based freshness despite extended expiry, scoped/unknown/foreign token revocation, revoke-others/all, replay, transient signed cookies, expiry cleanup and the precise expiry millisecond. Configured POST get-session is rejected with 405 because deferred refresh is disabled. Native session endpoint comparisons remain open.
- [ ] Add indexed encrypted auth lookups and pagination beyond the explicit 4096-row model-scan bound before claiming scalable production storage parity.
- [x] Implement the native persistent session backend: random 32-character tokens, 24-hour lifetime, one-hour refresh, creation-based freshness, refresh suppression, scoped/unknown/foreign revocation, revoke-others/all, sign-out and transient expiry cleanup. Forty-eight native body/storage comparisons execute the actual configured HTTP handlers with shared seeded credentials; the fixture includes 63 checks. Memory and live encrypted PostgreSQL lifecycle tests pass. Native credential operations, production HTTP cookie/CSRF/Origin integration and indexed session lookup remain open.
- [x] Implement native email signup/signin over encrypted auth records. Signup commits user/account/session atomically and rechecks normalized-email uniqueness under the write lock. Bounded blocking scrypt jobs hold capacity through cancellation and run outside tenant write locks; signin rechecks account credentials afterward. Thirty-nine comparisons match actual configured handler bodies/stored records, callbackURL casing, errors, case normalization and UTF-16 password limits. Memory/live PostgreSQL cover concurrent signup; a controlled password-change race rejects stale verification without creating a session. Production HTTP validation/cookies/Origin/CSRF, remaining credential/account operations and indexed lookup remain open.
- [x] Match native authenticated password verification and rotation against actual configured handlers. Eighty-one credential comparisons cover password errors, no-revocation retention, all-own-session replacement and other-user isolation. Memory/live PostgreSQL exercise encrypted credential updates and session replacement. Blocking derivation stays outside write locks and authoritative session/account rechecks reject concurrent changes. Production HTTP cookies/CSRF/Origin, user/account operations and CLI integration remain open.
- [x] Match native trusted-origin/callback and mutation/login CSRF policies against 2,822 actual library/HTTP cases. Cover exact/wildcard/custom origins, UTF-16 wildcard units, relative callback separators/traversal, Cookie header presence, Origin/Referer fallback, null same-origin Fetch Metadata and cross-site login navigation. Production adapter wiring remains open.
- [x] Add native signup/signin/password verification/rotation HTTP routes over atomic encrypted storage. Compare 122 actual HTTP body/storage/cookie/callback outcomes and 63 full parser/schema/guard/method errors. Preserve transient replacement-cookie flags, additional-field JSON presence, ignored private inputs, forwarded-IP normalization and current-request agent/IP metadata. Live memory/restricted-role PostgreSQL exercise signup, signin, rotation and revocation through TCP alongside MCP startup/discovery/invocation/shutdown. Existing 161 session/JWT HTTP comparisons remain green. Remaining raw routes, production rate limiting and CLI preset stay open.
- [x] Port configured Better Auth default production per-IP/per-path rate limits, rolling windows, special endpoint rules, fallback IP buckets, atomic concurrency and retry headers. Compare 153 actual production HTTP outcomes, including guard/parser order and normalized IP/path keys. Bounded process-global memory indexes preserve insertion eviction. A live TCP burst admits exactly three of 32 simultaneous signin requests and shuts down cleanly. Complete preset remains open.
- [x] Add native update-user, list-accounts and unlink-account HTTP routes. Fifty actual response/cookie/storage comparisons cover scoped accounts, token/hash filtering, ECMAScript scope trimming, opaque profile updates, ignored private fields, transient cookies and full errors. Atomic unlinking preserves one remaining login under concurrency; revoked sessions cannot mutate profiles. Live memory/restricted-role encrypted PostgreSQL exercise profile HTTP beside MCP. Remaining user/account/provider operations and real ORM conformance stay open.
- [x] Match the four configured-disabled reset-email, verification-email, change-email and delete-user HTTP routes. Compare 128 actual response/cookie/storage outcomes, including schema/Origin order, authoritative session requirements and empty 404 deletion responses. Live memory/encrypted PostgreSQL verify their HTTP responses alongside MCP. Actual token reset/verification and remaining user/account/provider operations remain open.
- [x] Add native single-use token password reset. Forty-four actual HTTP/storage comparisons cover body/query tokens, validation order, UTF-16 limits, newest duplicate selection/invalidation, expired/missing-user consumption, exact expiry, missing credential creation and retained sessions. Bounded hashing stays outside tenant locks; concurrent consumption permits one winner. Live memory/restricted-role PostgreSQL exercise reset and password verification over HTTP beside MCP. Email verification/callback routes remain open.
- [x] Add GET reset-token and configured-disabled deletion callbacks. Fifty-two actual HTTP/storage comparisons cover query validation aggregation, raw encoded token segments, trusted destinations, configured redirect authority, query replacement/encoding, JSON redirect content type, expired-verification cleanup and non-consuming GET behavior. Live memory/restricted-role PostgreSQL follow GET redirects with POST reset alongside MCP. Email verification and remaining user/account/provider operations remain open.
- [x] Match configured HS256 email-token verification against 96 actual JOSE/HTTP outcomes. Distinguish forged/malformed tokens, expiry and handler schema failures; preserve critical-header behavior, optional iat/nbf/exp ordering, padding/noncanonical bits, BOMs, duplicate keys and numeric overflow using local raw JSON parsing. Native email-verification HTTP/user/session/redirect integration remains open.
- [x] Add signed email-verification HTTP integration. Fifty-two actual response/storage/cookie/header comparisons cover idempotent verification, confirmation/verified/legacy change tokens, authoritative session checks, reuse/creation/transient cookies, error-query appending, raw relative and byte-string success redirects, whitespace normalization and empty malformed-claim 500 responses. Live memory/restricted-role PostgreSQL verify HTTP mutations, forgery rejection, cookies and subsequent session/JWT access beside MCP. Configured email delivery/change requests remain disabled; provider/org/account operations and CLI preset remain open.
- [ ] Complete Better Auth user/account operations over the native credential/session HTTP backend, with real TypeScript ORM conformance and protocol comparisons.
- [x] Add native Ed25519 JWT issuance/public JWK primitives with configured claims and 15-minute lifetime. Twenty-six comparisons execute the actual configured Better Auth payload/signing/JWKS and JOSE verification. Private PKCS#8 restoration preserves IDs/signatures; wrong issuer/audience, exact expiry and forged signatures fail verification. The subsequent keyring checkpoint covers encrypted persistence/selection/rotation; session-authenticated HTTP integration remains open.
- [x] Add durable native keyring selection and rotation over encrypted auth storage. First-key initialization serializes concurrent callers; service reconstruction preserves keys, selection keeps actual insertion ties, and retired public keys obey the exact 30-day grace boundary. All-expired stores publish an empty set until issuance provisions a new live key. Twenty-seven actual Better Auth/JOSE comparisons and memory/live-PostgreSQL lifecycle tests pass, including corrupt key rejection. Session-authenticated token/JWKS HTTP integration remains open.
- [x] Integrate nine native raw session/JWT HTTP endpoints with signed cookies, authoritative storage, refresh/freshness/revocation, credentialed CORS and Origin/callback guards. Include session-authenticated token issuance, public JWKS and the get-session JWT response header. Compare 161 actual configured HTTP body/storage/header/JWT outcomes; live memory/restricted-role encrypted PostgreSQL validate HTTP and MCP startup/discovery/invocation, auth, CORS, CSRF and graceful shutdown. HTTPS extraction/clearing uses secure names/attributes. Raw auth routes stay outside contract/MCP discovery. Credential HTTP, remaining routes and CLI preset remain open.
- [ ] Complete Better Auth custom callback exchange and generated client integration over the native session-authenticated token/JWKS endpoints.
- [x] Correct the configured Better Auth invitation lifetime to 48 hours in seconds. An operator-seeded dynamic invitation grant exercises the actual handler and validates persisted expiry; default-role denials remain intact.
- [ ] Port the configured Better Auth organization/team/member/invitation/dynamic-role endpoints, permission checks, default team creation, membership limits, active session organization/team and actual surfacing behavior. Keep static access-control permissions separate from stored surfaced role-permission JSON strings. Organization CRUD, member lifecycle/listing and team lifecycle/member management are implemented; invitation/dynamic-role APIs, real ORM conformance and complete surfacing remain open.
- [ ] Port enabled Google OAuth authorization-code/PKCE/state, callback/account linking and token lifecycle behavior, using the corrected configured Google given/family-name mapping.
- [x] Correct configured Google sign-in's required-name mapping. A valid cryptographically verified Google ID token previously failed with `400 MISSING_FIELD` because `firstName` was absent. Map Google given/family names into required application fields, using empty strings when optional/malformed external name claims are absent. Execute real Google RS256/JWKS verification and Better Auth handlers with only Google's certificate HTTP boundary owned; verify sign-in/session/account reuse and reject forged signatures, wrong issuer/audience/nonce/key ID, expired and stale/future-issued tokens. Native Google OAuth and real ORM conformance remain open.
- [ ] Port Better Auth account PCI encryption/export/erasure and seven-year anonymization retention; its account password/provider-token classifications differ from the all-none IAM base module.
- [x] Harden the actual Better Auth callback helpers before native porting: enforce configured trusted callback origins, derive token-exchange authority from configured base URL, escape query data embedded in script JSON, and clear the configured secure session cookie. Forty checks execute actual server handlers and browser scripts under HTTP/HTTPS with real session/JWT handlers, forged Host headers and malicious callback/script inputs.
- [x] Add native application-owned HTTP namespaces for the raw Better Auth API. Internal mounts inherit global middleware, preserve raw request bodies, rebase through nested routers, reject contract/documentation/MCP namespace collisions, and remain absent from OpenAPI/SDK/MCP contracts. Live TCP tests verify cookie-authenticated HTTP alongside the unchanged MCP discovery/invocation surface and graceful shutdown. This is routing infrastructure; native Better Auth handlers remain open.
- [ ] Complete Better Auth module CLI generation/lifecycle, the two custom callback helpers, registered HTTP/MCP service routes and SDK contracts. Its raw authentication API is mounted internally in TypeScript and is separate from registered MCP tools.
- [ ] Validate generated-module init/change/sync/delete, contracts, SDKs and mixed-app operation for every added module family; update release/package documentation and compatibility fixtures.

- [ ] Audit and support Rust/mixed relay session-ingest and storefront extension behavior, including lifecycle and SDK interactions; these CLI surfaces have no separate published blueprint package.

- [x] Audit all exported HTTP methods (including WebDAV/link/unlink/subscribe variants) and generated custom-router extension behavior inside built-in Rust modules; preserve actual mounted contracts and fail explicitly for unsupported combinations.

- [x] Preserve the actual HTTP method for authentication on implicit HEAD-to-GET dispatch, while retaining explicit HEAD precedence and local-handler compatibility; include request-HMAC regression coverage.

- [x] Validate actual service HTTP and MCP startup, initialization/discovery/tool invocation and authenticated dispatch for generated generic services, IAM, CAC, base/Twilio messaging and custom routers. Live fixtures cover all 27 HTTP methods, signed JSON/forms, bearer/session/JWT forwarding, tenant rejection, revocation, RBAC, Origin rejection, graceful shutdown and archive-installed enabled/disabled/custom-path startup. Reuse these checks for each additional generated module family.

- [x] Align JSON query arrays/objects/null across native MCP and both generated TypeScript SDK surfaces. Forty actual TypeScript MCP-generator/universal-SDK comparisons cover empty arrays, Unicode/reserved characters and single percent encoding. Remove duplicate URL encoding in both TS transports; live native HTTP/MCP and generated SDK regressions pass. Rust SDK query maps continue to accept explicitly serialized string values.

- [ ] Complete remaining MCP compatibility: multipart calls and expanded actual TypeScript generator conformance fixtures. Additional typed tool execution and configurable separate listener/TypeScript port defaults are implemented and validated below.

- [x] Execute actual configured billing schema resolvers, Zod runtime/OpenAPI and application mappers. Sixty-six route contracts, 28 real validator/OpenAPI DTO schemas and 65 configured mapper cases pass. Correct optional Stripe payload contracts, HTTP unknown-field stripping, Stripe local plan/subscription IDs and portal owner preservation; generated HTTP/MCP, both SDKs and verified-webhook-created portal reads pass. Record intentional stricter native coercion/provider-object checks and corrected cache provider-ID behavior; Redis interop and crash reconciliation remain open.

- [x] Port managed payments onboarding/status helper over the instance gateway. Sixteen actual TypeScript/native comparisons validate instance-key signatures, request bodies, readiness DTOs, single-use onboarding requests, invalid redirects, failure statuses, Retry-After hints and malformed/oversized responses. No automatic onboarding retry or vendor credential is introduced. Native response and redirect validation is stricter; remote error bodies are redacted. Other managed capability helpers remain open.

- [x] Port managed SMS client, typed send/event DTOs and GSM/UCS accounting. Fifty-five actual TypeScript/native cases cover all GSM extensions, UTF-16 surrogate-pair and ten-segment boundaries, E.164, ECMAScript whitespace, default/promotional purpose, deferred refusal, signed JSON wire bytes, fresh nonces, opt-out/refusal statuses and retry hints. Native response DTOs are validated and remote errors redacted. CLI managed-capability registration/event stubs for native modules remain in scope; client coverage alone does not complete infra add/remove parity.

- [x] Port managed WhatsApp template/text/listing client and delivery DTOs. Twenty-one actual TypeScript/native comparisons cover template component pass-through, empty listings, recipient/text validation, Unicode whitespace, platform policy refusals (403/409/422/429), signed exact JSON bodies, nonce freshness and retry hints. Typed native responses reject malformed data. CLI capability registration/event routes remain open.

- [x] Port managed voice start/read/end and event DTOs. Forty-seven actual TypeScript/native comparisons cover catalog flow names, E.164, attribute key/value/count/UTF-16/byte limits, reserved prefixes, call-ID isolation, status, empty 200/204 hang-up responses, exact signed bodies, nonce freshness, refusal statuses and retry hints. Empty-response support is opt-in; existing provider JSON response validation remains strict. CLI capability wiring/event mounting remains open.

- [x] Port managed email send/suppression and delivery DTOs. Fifty-one actual TypeScript/native comparisons cover mailbox rules, scalar/array recipients and replies, 50-recipient bounds, UTF-16 subject length, UTF-8 body size, tag limits/reserved names, suppression lowercase and encodeURIComponent paths, signed body bytes, gateway refusals and retry hints. Test servers collect complete bytes before UTF-8 decoding, including a 512 KiB multibyte body. Native email responses remain typed; CLI capability/event wiring and model gateway remain open.

- [x] Add reusable platform-event verification with an asynchronous replaceable nonce-store interface and bounded atomic memory default. Thirty-four actual TypeScript/native comparisons cover exact raw bytes, method/path/key binding, minimal event shape, skew boundaries, replay, malformed payloads and same-event-ID redelivery. One hundred concurrent deliveries have one nonce winner. Native retains future-dated nonces through their full inclusive validity window, closing an actual TS replay gap, and bounds bodies/storage without evicting valid nonces. Memory is single-process; replica callers must supply shared storage, and event-ID business deduplication remains handler-owned. Generated capability routes/wiring remain open.

- [x] Add contract-preserving native route decoration for platform capability dispatch and other middleware. Standard and extended HTTP methods, dynamic paths, nesting, existing-handler fallback and request validation pass; live MCP and direct HTTP both execute the decorated route. All 29 Axum unit and two live MCP tests pass with strict clippy.

- [x] Add trusted managed-settings resolution and explicit fixed Docker gateway-mock transport. Fourteen actual TypeScript/native configuration cases cover missing/partial/complete settings, loopback, fixed mock opt-in, remote TLS, invalid opt-in and credential redaction; normal transport continues to reject remote plaintext. Generated Stripe billing uses the validated gateway constructor and passes HTTP/MCP, both SDKs, callbacks, restart, CLI lifecycle and invalid-opt-in startup rejection. Actual Docker capability add/remove/startup wiring remains open.

- [x] Add native managed-capability CLI wiring for email, SMS, WhatsApp, voice and payments. `infra add/remove` resolves the registered native crate path, stages typed client factories, handler stubs, callback contracts and shared HTTP/MCP middleware, and records source/dependency ownership. Clients share a gateway connection pool; SMS defers unmanaged refusal as the TypeScript registration does. Fresh relocated mixed applications compile both SDKs and pass HTTP/MCP startup, actual-TypeScript-signed callbacks for every feature, forgery/staleness/replay rejection, authenticated MCP boundaries, dry-run and stacked removal preservation. Untouched files restore byte for byte; human handler/Cargo/library/env edits survive. All 60 CLI infrastructure tests pass, including 31 existing TypeScript capability tests. Linux Compose verifies all five outbound clients over signed Docker-DNS calls and signed gateway-to-service callbacks, plus HTTP/MCP startup. Archive-installed CLI generation, service HTTP/MCP settings, callback validation and contract/SDK exports pass. Ownership snapshots are Git-ignored and private on Unix because restoration records may contain pre-existing local settings. Capability lifecycle rename/delete, safe removal without local ownership records, shared replica nonce storage, and in-place S3/Redis CLI wiring remain open.

- [x] Dispatch managed callbacks using Axum-decoded feature parameters while authenticating the exact request URL path. Generated runtime accepts a correctly signed percent-encoded feature and rejects a signature over a different decoded URL; HTTP/MCP, SDK and source-preservation regressions pass. This exact-URL binding is stronger than the TS controller reconstruction of its decoded feature path.

- [x] Exercise native capability decoration on a generated Stripe billing module with its existing payment-event route. All five capabilities stack without duplicate contracts; email/SMS/WhatsApp/voice callbacks reject replay, and payments retain PostgreSQL receipt processing and subscription business effects. Full billing HTTP/MCP, both SDKs, managed Stripe, restart and CLI lifecycle regressions pass.

- [x] Preserve native capability hooks and custom source through service sync, rename and deletion. Renames move private ownership records, update Cargo restoration snapshots and local callback targets, and update generated config/smoke-test names without rewriting customized files. Registered custom crate locations survive sync and rename. A fresh-clone recovery path removes generated hooks without a local ownership record while retaining dependencies/settings of unknown origin. Deletion removes ownership records and releases or retains the shared mock according to remaining manifest capabilities. Generated relocated mixed-app fixtures pass HTTP/MCP before/after rename, SDKs, six service smoke tests, custom nested crate paths, clone recovery and deletion; 62 infrastructure tests pass. Multi-service mock baseline/target preservation and human edits to managed dependency entries remain under review.

- [x] Retain human edits to managed dependency entries across stacked capability commands, including custom paths, extra features and explicit dependency removal. Private ownership records remember the last human entry before generated edits; restoration no longer mistakes a later generated snapshot for user ownership. Three native ownership/lifecycle tests pass. The preceding lifecycle checkpoint also passes all 958 CLI tests and the full generated billing HTTP/MCP, SDK, persistence, managed gateway and rename/delete fixture. Multi-service mock ownership and callback-target preservation remain open.

- [x] Track the local gateway mock at application scope so a custom pre-existing mock survives both service removal orders and deletion even when the first capability owner leaves early. Removing the active owner promotes another registered service; removing an inactive owner preserves the existing target. Rename handling retains exact host boundaries and URL suffixes. Real CLI/Compose configuration tests pass both orders, callback promotion, deletion and custom baseline restoration. Cargo ownership comparisons now use TOML values, avoiding false human overrides when sync changes inline dependencies into tables; custom features/paths/removals remain preserved. All 960 CLI tests and the expanded generated HTTP/MCP/SDK/lifecycle fixture pass. Edits made to the shared mock while capabilities are active still need ownership-aware merging; local-mock routing supports one target per feature.

- [x] Merge developer changes into shared gateway restoration records while capabilities remain active. Changes to images, environment keys and nested extension metadata survive later additions/removals/deletion; deleted keys stay deleted. Customized generated mocks become developer-owned and remain without stale generated callback targets. Explicit mock removal updates the baseline even if a later add recreates it. Real CLI/Compose ownership fixtures and all 960 CLI tests pass.

- [x] Preserve Docker Compose standard long network/port/volume forms and short dependency lists while editing native and TypeScript infrastructure. Network aliases/IP assignments, named ports and mount options round-trip through CLI edits; dependency required/restart metadata and build arguments/secrets are retained. Application rename retains structured options, and port allocation handles address bindings, protocols and ranges. All 964 CLI tests, real normalized Compose ownership edits and generated HTTP/MCP, SDK and capability lifecycle checks pass.

- [x] Generate a preserved `src/capabilities/nonce_store.rs` factory returning `Arc<dyn PlatformEventNonceStore>`. Callback dispatch creates one verifier with that store for every feature; application code can supply shared atomic storage without editing the owned dispatch section. A customized capacity-two store survives sync/rename, accepts email and SMS, then returns 503 for voice, proving shared capacity rather than a new store per callback. Default stub removal and custom factory retention pass alongside generated HTTP/MCP, SDK, source/dependency ownership, clone recovery and custom crate lifecycle checks. All 960 CLI tests pass; the default remains single-process memory and applications must configure shared storage for replicas.

- [x] Wire native in-place Redis/S3 infrastructure commands and honor init/change infrastructure flags. Editable typed factories, relative Cargo dependencies, local settings, manifest resources and shared Compose backends support registered nested crate paths, sync, rename, removal, deletion and clone recovery. Customized factories/dependencies/settings and adopted backends remain application-owned. Generic service metadata edits preserve source. Fix relative directory paths and update ejection to use the source parent; capability removal no longer restores unrelated removed infrastructure settings or discards adopted/deleted gateway dependencies. All 967 CLI tests, real CLI/Compose ownership fixtures and relocated generated Redis/MinIO binary round-trips pass; HTTP/MCP enabled/disabled/custom-path startup and both SDK compilers pass. Factories currently expose raw adapters with explicit local S3 credentials; default production AWS credential-chain configuration and generated encryption/compliance wiring remain open.

- [x] Complete generated Redis/S3 encryption/compliance factories and actual TypeScript/native encrypted-storage comparisons, as covered by the later JSON/queue checkpoints. Complete helper/worker coverage remains tracked separately.

Packaged CLI validation also passes native Redis/S3 generation alongside all five managed capabilities, relocated compiler checks, service HTTP/MCP settings, signed callbacks and contract/SDK exports. Fresh IAM, CAC and base/Twilio messaging runtime fixtures pass HTTP/MCP and lifecycle checks; the IAM fixture now rejects an actually unsupported module instead of the now-supported billing module.

- [x] Add production S3 environment resolution and an async generated factory using the official AWS default credential chain with profile/SSO/process/container-role support. Complete S3 key pairs override the chain; partial pairs follow the TypeScript fallback. Both factories honor namespace and presign caps. Thirty-one actual TypeScript/native configuration cases cover endpoint/region/key combinations, prefixes, numeric/radix/fractional values and ECMAScript whitespace. An owned ECS credential endpoint supplies session credentials used to sign actual SDK object/tag requests under the configured prefix. Generated relocated factories round-trip data through real Redis/MinIO using standard AWS environment credentials, pass HTTP/MCP settings and both SDK compilers. All 967 CLI tests, 18 S3 unit tests, the actual S3 backend test and strict all-target clippy pass. Preserve the synchronous explicit-local factory for compatibility; encrypted/compliance-aware cache and object-store wiring remains open.

Fresh IAM, CAC, base/Twilio messaging and ecommerce runtime/lifecycle fixtures pass. Together with billing and generic managed/local-infrastructure services, these validate HTTP/MCP initialization, discovery, health/tool invocation, SDK compilation and graceful service shutdown. Ecommerce comparisons use the isolated Stripe SDK through `STRIPE_SDK_PATH`.

The production S3 checkpoint also passes archive-installed CLI generation, portable framework compilation, HTTP/MCP settings, signed capabilities and contract/SDK exports. The package-source inventory now checks 268 embedded source/template assets, including the new S3 example directory and infrastructure/capability factories.

- [x] Align native Redis list queues with the actual TypeScript LPUSH/RPOP wire layout and add non-destructive batch peeks in dequeue order. Keep an explicit LegacyNative RPUSH/LPOP mode for existing native backlogs; producers/consumers must use one consistent layout. Actual TypeScript queue methods and native clients exchange data through the isolated Redis backend in both directions, covering batch/single FIFO, mixed producers, peeks and legacy backlog reads. Strict all-target clippy and all six Redis tests (including the actual backend fixture) pass. This closes list orientation interoperability; JSON/encryption/compliance, complete TTL/cache metadata and BullMQ worker wire conformance remain open.

- [x] Add shared fallible `JsonRecordCodec`, typed `JsonCache` and async `JsonObjectStore` wrappers plus explicit generated encrypted factories. Per-operation validated tenant context selects authenticated v2/v3 envelopes; encrypted reads without context, wrong tenants, malformed payloads and serialization failures fail closed. Plaintext JSON migration and key rotation work, including false/zero/null values. Pre-serialize cache batches before writing. Actual TypeScript Redis methods and native clients exchange encrypted JSON in both directions against Redis, covering rotation, expiry, corruption and tenant rejection. Relocated generated services use the factories against real Redis/MinIO with encrypted at-rest checks, metadata preservation and wrong-tenant rejection, then pass HTTP/MCP enabled/disabled/custom endpoint startup and both SDK compilers. All 967 CLI tests, five core tests and six non-ignored Redis tests pass; strict infrastructure all-target clippy and 273 packaged source assets pass. Direct TypeScript/native S3 storage comparisons, encrypted queue convenience methods and complete TTL/cache metadata remain open.

- [x] Add generated `encrypted_queue(name)` and typed enqueue/dequeue/batch/peek helpers, with TypeScript-compatible FIFO envelopes and explicit decode-after-removal semantics. Add transactional GET/PTTL JSON cache reads retaining subsecond expiry and persistent-key metadata. Actual TypeScript/native encrypted cache/queue exchanges pass v2/v3, both producer directions, batch order, non-destructive wrong-tenant peeks and false/zero/null preservation. Actual TypeScript S3 methods over their injected SDK boundary and signed native AWS SDK requests exchange v2/v3 JSON in both directions, including prefixes, content types, rotation, plaintext migration, misses and tenant/corruption rejection. Real MinIO encrypted factories remain covered by the generated runtime fixture. Fresh relocated encrypted-queue/service runtime passes HTTP/MCP settings, shutdown and both SDK compilers; strict infrastructure all-target clippy, six non-ignored Redis tests and 274 packaged source assets pass. The prior encrypted-factory checkpoint also passed archive-installed CLI generation, HTTP/MCP, managed callbacks and SDK exports. Per-record batch TTLs, complete S3 helper coverage and worker/BullMQ wire parity remain open.

- [x] Complete typed Redis per-record TTL batches with prevalidated keys/expirations and transactional value/expiry reads. `put_records` supports a separate TTL or configured default per entry; `read_records` keeps order, misses and persistent-key metadata; literal-prefix SCAN reads expose decoded records with expiry. Actual TypeScript/native batch writes and reads compare values and relative expirations in both directions, including encrypted envelopes. Invalid TTL/key batches leave the first valid entry unwritten, empty batches work, and glob characters are matched literally. The full Redis fixture, including the actual backend, passes all seven tests; strict all-target clippy and the extended encrypted cache/queue conformance harness pass. Retain the intentional native semantics of precise milliseconds, explicit persistent expiry, misses as Option and falsy-value preservation rather than TypeScript's rounding/filtering accidents. Worker/BullMQ interoperability remains open.

- [x] Add typed S3 JSON batch writes/reads and single/batch deletes, defaulting absent content types to application/json while preserving custom types/tags and ordered misses. Serialize/encrypt all JSON before starting batch writes; backend partial-write behavior remains explicit. Actual TS object-store methods (including JSON stream-upload aliases) and signed native AWS SDK calls pass encrypted single/batch exchange, false/zero/null payloads, deletes, missing objects and empty batches. A serialization-failure regression proves no S3 write occurs and serializer details stay redacted. Generated encrypted factories pass real MinIO single/batch writes, metadata/default-type reads, missing keys, deletes and wrong-tenant rejection, then HTTP/MCP settings/shutdown and both SDK compilers. Strict S3 all-target clippy, 19 unit tests and encryption-only feature tests pass; all 274 embedded source assets package correctly. Complete binary/stream/presign helper parity, worker/BullMQ interoperability and release gates remain tracked separately.

- [x] Wire platform Redis key/channel namespace conformance into native leased workers and Pub/Sub controls. Resolve REDIS_KEY_PREFIX before FORKLAUNCH_REDIS_KEY_PREFIX with actual TypeScript-compatible whitespace/separator behavior; preserve dedicated-cache keys and native cluster hash tags. Add injected RedisKeyScope/connect_scoped APIs and sanitized NOPERM errors. A real restricted ACL fixture compares actual TS prefix helpers, enqueues/claims/acks native jobs, fences tenants, delivers Pub/Sub controls and denies foreign keys/channels and FLUSHDB. The runner fails promptly on NOPERM rather than looping. The fixture exposed redis 0.32 PubSub subscribe silently discarding server-error payloads; add a bounded subscription-ack preflight without publishing control messages, and verify denied subscription startup fails. All seven Redis-worker unit tests and both actual Redis integration tests pass, as do strict all-target clippy, 276 packaged assets and generated worker runtime/control/retention/batches/relocation/rename/backend-change/delete regressions. This closes platform prefix/ACL startup behavior; BullMQ wire compatibility remains open and is being implemented separately.

- [x] Add an optional official BullMQ Rust protocol adapter and actual TypeScript/native interoperability fixture. Both producer/consumer directions pass single/batch jobs, fixed/exponential retries, terminal failures, duplicate event IDs, default retention, and waiting/active inspection against a real restricted Redis ACL. A denied native consumer fails promptly and drains. Strict all-feature blueprint workspace clippy, seven worker unit tests, both actual Redis integration tests, formatting and 278 packaged source assets pass. CI now runs infrastructure JSON/encryption and worker ACL/BullMQ conformance fixtures using pinned BullMQ 6.3.10. Generated BullMQ lifecycle support, native failure observers and complete option/retention coverage remain open.

- [x] Generate native BullMQ workers through init/change/sync/rename/delete with the official protocol adapter, preserved application handlers, retries/backoff/concurrency/timeouts, inspection and scoped optional Pub/Sub controls. Add HTTP health/readiness and authenticated pause/resume/drain with active-job shutdown, plus a watch-based control transition API. Per-attempt native failure observers retain original events through retries, automatic removal and graceful drain; consumer peeks work before start. Actual TS/native fixed/exponential retry and default/count-based retention fixtures pass both directions under restricted Redis ACLs. Fresh relocated generation passes explicit stub failures, real processing/retries, HTTP controls, service HTTP/MCP initialization/discovery/health/enabled/disabled/custom-path checks, contracts/SDKs and source-preserving lifecycle changes. A nested workers/team/jobs registration passes sync/reinit/backend conversion/rename/delete, closing hardcoded worker crate/dependency paths. All 967 CLI tests, strict Redis/database-worker clippy, three control-transition tests and 283 packaged assets pass. E2E CI includes the pinned BullMQ runtime fixture. Age-based retention, stalled-job/custom-option behavior and broader worker conformance remain separate gates.

- [x] Bind additional typed MCP tools to the real Streamable HTTP runtime. ManualTool::typed derives discovery metadata from FrameworkSchema, parses/validates before application work, supplies fresh headers for operation authorization and preserves transport authentication. Reject manual/contract name collisions at startup; normalize explicitly public handler failures, redact serializer details, bound invocation time/output and preserve object/scalar/array results. Eleven MCP tests exercise initialization/discovery/invocation, HTTP/manual coexistence, schema/type/range/unknown-field rejection before side effects, tenant credentials per call, timeout, output limits and serializer failures. Strict MCP/Axum and whole-blueprint all-feature clippy, formatting and 285 embedded assets pass. Fresh generated generic HTTP/MCP and SDK fixtures pass after the new dependency. Fresh IAM, CAC, base/Twilio messaging, billing/Stripe and ecommerce runtime/lifecycle suites also pass HTTP/MCP startup and authenticated dispatch. Separate listeners, multipart and complete actual-generator conformance remain open.

- [x] Add configurable separate MCP listener orchestration to generated generic, IAM, CAC, messaging, billing and ecommerce executables. MCP_LISTENER=separate uses the actual TypeScript HTTP-port-plus-2000 fallback; MCP_PORT/MCP_HOST select explicit/ephemeral ports and hostname/IP binding. Preserve the native shared-listener default explicitly. The second router exposes only MCP while tools retain internal HTTP validation/authentication; both listeners bind before serving, then drain together with a 30-second bound. Live two-listener tests verify route credentials, denied calls, concurrent active HTTP/MCP completion and closed ports. Fresh generated startup covers shared/separate/disabled/custom endpoints, explicit/ephemeral/default-offset ports, hostname binding, invalid settings and occupied-port rejection without a leftover HTTP listener. Generated BullMQ runtime and nested lifecycle regressions also pass. All 967 CLI tests, 31 Axum unit tests, two live MCP integration tests, eleven MCP tests, strict Axum/MCP clippy, formatting and 287 packaged assets pass. Multipart and remaining actual-generator conformance are still open.

- [x] Fix managed-capability wiring for the new serve_with_default_mcp entrypoint while retaining legacy into_router_with_default_mcp mounts. Insert the capability router exactly once and preserve byte-for-byte removal; reject ambiguous/missing mounts. A regression exercises all six current generated entrypoint templates plus the legacy call. All 968 CLI tests pass, and fresh billing/Stripe and ecommerce HTTP/MCP/SDK/capability/lifecycle fixtures pass after the correction. IAM, CAC and base/Twilio messaging fixtures also pass with coordinated listener startup.

- [x] Add MCP multipart text-field transport, File/Binary JSON-argument encoding and URL-encoded composite values matching the actual TypeScript generator. Generate collision-checked MIME boundaries and escape field names/line breaks; reject non-string multipart parts and SSE request bodies. Thirty-two actual-generator/body-discriminator comparisons and fifteen MCP unit/transport tests pass, including real Axum Multipart/Form/Bytes extractors behind route authorization. Strict full-framework all-feature/all-target clippy, formatting and 289 embedded assets pass. Expand the archive-installed CLI fixture with official BullMQ generation and assertions for typed, separate-listener and multipart MCP sources. Streaming, arbitrary multipart upload parts and broader MCP option conformance remain open.

- [x] Complete multipart uploads in both CLI SDK targets through contract and OpenAPI generation. TypeScript accepts FormData or typed field records and uses the actual shared safeStringify source for multipart/URL-encoded values; files, repeated names, JSON composites and special JavaScript values match thirteen actual universal SDK comparisons. Normalize header casing and let encoders own boundaries. Rust SDKs consume native reqwest forms, preserve authentication/hooks and repair reqwest zero-byte empty forms with a closing MIME delimiter. Preserve multipart bytes in both HTTP adapters. The framework Rust request builder now encodes owned multipart text/file parts and exposes dynamic content types; the framework TypeScript generator clears conflicting multipart headers. Strict standalone Rust/TypeScript compilation and standard MIME parsing pass; fresh relocated SDKs call Rust and TypeScript services successfully from both artifact formats. All 969 CLI tests, 31 Axum unit/two live MCP tests, fifteen MCP tests, both SDK unit suites, strict framework workspace clippy, formatting, 290 embedded assets and a fresh archive-installed CLI fixture pass. The archive fixture also typechecks its generated TypeScript SDK and verifies the bundled shared stringifier, all four workers, modules, capability callbacks and HTTP/MCP startup. Multipart responses, automatic typed extraction, SSE and remaining recursive/realtime SDK tasks remain open.

- [x] Add transport-independent bounded SSE encoding/decoding with arbitrary chunk boundaries, UTF-8/BOM, CR/LF/CRLF, multiline data, comments, IDs/resets, event names and retry metadata. Reject metadata injection, fail closed on event overflow and discard unterminated EOF frames according to the HTML standard. Eighteen HTTP tests, strict HTTP clippy, formatting and 292 embedded assets pass. Thirty-five actual TypeScript universal SDK comparisons pass for common terminated frames; CI now runs them. Document existing TS multiline/whitespace/EOF differences explicitly. Generated streaming clients, cancellation/reconnection and full typed runtime behavior remain open.

- [x] Generate bounded pull-driven SSE responses in CLI TypeScript/Rust SDKs from both contract and OpenAPI artifacts. Return typed async iterables or boxed owned Rust streams, preserve authentication/hooks, enforce configurable 1 MiB default event bounds and response MIME, deserialize Rust envelopes and release responses on iterator return/throw, abort, close/drop and terminal failures. Preserve complete events before later oversized events even when coalesced in one chunk. Live relocated service tests verify delivery before EOF, split UTF-8, BOM/multiline/CRLF/bare-CR framing, ID resets, auth before handler work, finite/truncated streams, wrong media, type/transport/size failures, unstarted cancellation, server stream cleanup and graceful HTTP/MCP shutdown. Both artifact formats pass, including strict generated-SDK clippy; the streams are boxed to keep response enums small, and request-hook type aliases close an existing clippy warning. All 970 CLI tests, nineteen HTTP codec tests, 31 Axum/two live MCP tests, fifteen MCP tests, both framework SDK suites, strict full-framework clippy, formatting and 298 embedded source/template checks pass. Thirty-five actual TS universal SDK/native codec/generated SDK SSE comparisons and thirteen form comparisons pass; ordinary HTTP/multipart/MCP SDK regressions pass against Rust/TypeScript servers from both artifacts. A fresh archive-installed CLI also passes portable generation, all four workers/modules, TypeScript SDK compilation, capability callbacks and HTTP/MCP startup; E2E CI now includes the dedicated streaming fixture. Framework-native typed response decoding, remaining emitter/schema conformance, recursive models and realtime/mixed SDK work stay open.

- [x] Support guarded recursive CLI SDK models through contract and OpenAPI artifacts: self/mutual records, optional/null fields, recursive arrays/dictionaries and object/array unions. Preserve named graph edges with bounded local reference processing; inline acyclic definitions to retain existing types. Reject external/missing references, validation siblings and unguarded aliases before writes. Bound cached wide-graph traversal as well as initial expansion, and omit Rust Default implementations across required recursive edges. Rust vectors avoid redundant boxes; collection aliases use transparent wrappers. Rebase standalone contract schema pointers when exporting OpenAPI components, escaping component names and retaining literal examples/defaults/constants. Fresh relocated clients pass strict TypeScript compilation (including a rejected invalid recursive request), strict Rust SDK clippy, recursive HTTP/MCP invocation and graceful shutdown through both artifact sources. All 977 CLI tests, 31 Axum/two live MCP tests, nineteen HTTP codec tests, fifteen MCP tests, six OpenAPI tests, both framework SDK suites, strict full-framework clippy/formatting and 299 embedded assets pass. Ordinary HTTP/multipart/SSE regressions and 40 query/thirteen form/35 SSE actual TypeScript comparisons pass. The expanded archive-installed CLI fixture compiles recursive clients through both artifact formats alongside all four worker types/modules and passes HTTP/MCP listener settings and managed callbacks. CI includes the recursive runtime fixture. Framework-native typed responses/recursive models, broader schema constraints, multipart responses and typed realtime/mixed SDKs remain open.

- [x] Share standalone-schema relocation between OpenAPI and MCP through the optional forklaunch-contract/json-schema feature. Rewrite schema-pointer edges only, retaining examples/defaults/constants/enums/extension data and properties named $ref; escape body/query/header/path field locations. Handle tuple/conditional and unevaluated schemas, and preserve embedded $id resource bases. CLI imports explicitly reject resource IDs and dynamic references until their resolver is implemented. Live recursive tool discovery preserves an example containing a literal $ref while HTTP/MCP invocations and both SDKs pass through both artifact formats. All 977 CLI tests, 31 Axum/two live MCP tests, sixteen MCP tests, five contract/five OpenAPI tests, nineteen HTTP tests, both SDK suites, strict full-framework clippy/formatting and 299 embedded assets pass. The contract crate also checks without default features. A fresh archive-installed CLI passes recursive client compilation through both artifacts, all worker/module compilation, HTTP/MCP shared/separate listeners and all five managed callbacks. Rust field omission/null distinctions, framework-native response/recursive models, resource/anchor resolution and broader realtime/mixed SDK work remain open.

- [x] Preserve generated Rust SDK field presence: omit absent optional fields, reject null for non-nullable optional fields, distinguish optional null from omission and reject missing required nullable fields, including nested records, arrays and unions. Compare 94 cases against the actual TypeScript validator through contract/OpenAPI generation, strict TypeScript/Rust compilation and both live SDKs against Rust/TypeScript HTTP, with MCP invocation and graceful shutdown. Fix the canonical TypeScript null schema enum to contain JSON null. Box generated JSON model response variants to keep status enums small; update inner-union matching accordingly. All 977 CLI tests, fourteen existing/new Zod validator tests, ordinary HTTP/multipart/recursive/SSE regressions, formatting and 299 embedded asset checks pass. A fresh archive-installed CLI compiles both SDK artifact formats, all four worker types/modules and passes HTTP/MCP listener configurations and all five capability callbacks. CI includes the presence fixture and pinned actual-validator dependencies. Framework-native typed response/recursive generation and broader schema/realtime/mixed SDK tasks remain open.

- [x] Share the bounded local schema graph, Rust/TypeScript type compiler, Serde model renderer and Rust client/SSE templates between CLI and native framework generators. Retain request/field JSON schemas and response kind/MIME/schema metadata in SdkGenerationPlan. Add checked generate_rust_http_sdk_source with typed status responses, guarded recursive models, field presence, auth/hooks, multipart uploads and bounded owned SSE streams. Reject duplicate/runtime-reserved names, invalid media/statuses and unsupported schema graphs before source output. Bound literal data depth/node/bytes before cloning and stream-count serialization bytes; reject oversized JSON schema strings before parsing. Native clients pass strict compilation and the same 94 actual validator cases against both language servers, recursive tree/array/dictionary/union/mutual-record calls and live SSE cancellation/error/resource-cleanup checks with HTTP/MCP startup/discovery/invocation/shutdown. All 978 CLI tests, eight native codegen/four Rust SDK/three TypeScript SDK tests, full framework all-feature test suites, strict workspace clippy/formatting and 311 embedded assets pass. Existing native request-builder/media and ordinary HTTP/multipart SDK regressions pass. An archive-installed CLI compiles all four workers/modules and both SDK artifact formats, passes listener settings/capability callbacks, and invokes the native recursive generator entirely from bundled sources with generated-SDK warnings denied. Native TypeScript typed response/recursive emission, advanced schema constraints/resources, multipart responses and realtime/mixed SDK work remain open.

Native typed TypeScript HTTP SDK checkpoint: the same CLI model/runtime now handles status/business-error unions, recursive aliases, exact presence/null and bounded SSE. The 95-case validator, recursive and streaming fixtures exercise both native generators and both CLI artifact sources. Real media tests cover Blob/ArrayBuffer/sliced bytes, URL-encoded/multipart records, quoted MIME parameters, escaped paths, invalid JSON and status narrowing. Correct the messaging callback fixture to sign record composites as single JSON values, matching the actual TS universal SDK and MCP generator; explicit URLSearchParams retains repeated fields. All five generated module runtime suites (IAM, CAC, base/Twilio messaging, base/Stripe billing and ecommerce) pass HTTP/MCP discovery/invocation, SDK checks and their authenticated persistence/provider/lifecycle checks. Broader parity gates above remain open.

- [x] Share checked TypeScript HTTP generation with CLI types/runtime/templates; validate typed media/status errors, 95-case presence, recursive and SSE clients, all five module HTTP/MCP runtime suites and portable native generation from an archive install. All 979 CLI tests, nine codegen/four Rust SDK/four TypeScript SDK tests, strict workspace clippy/formatting and 319 embedded assets pass. Both artifact sources preserve quoted MIME parameters and escaped metadata.
- [x] Adapt SSE response ownership/cancellation to cross-fetch's Node byte streams. Own an abort controller, forward/detach caller signals and abort upstream requests on cancellation; live tests cover both CLI artifact formats, native injection/imported fetch, pending reads, unstarted cancellation and exact server cleanup.

- [x] Preserve Blob/FormData uploads across native and custom fetch adapters through shared body serialization in checked TypeScript and dependency-free native clients. Actual native/injected/imported cross-fetch tests cover file bytes, sliced views, repeated/empty multipart files, encoder-owned boundaries, URL-encoded composites, quoted media and status/business-error decoding.


- [x] Validate official BullMQ lazy age retention in both TS/native producer/consumer directions and abandoned-job recovery across languages, including maximum stalled/start attempts. Preserve original failure events even when preprocessor failures immediately remove their jobs, through a feature-gated outcome-data hook in a pinned vendored protocol crate. Keep existing failure notifications, Redis scripts and wire semantics; retain upstream MIT/provenance metadata and package the source in generated apps. Broader advanced worker options remain a separate gate.

- [x] Carry supported BullMQ queue defaults through the producer, preserving actual TypeScript retry/backoff overrides and ignored default job IDs. Persist job metadata and read Node log-limit aliases through an optional protocol patch. Actual cross-language metadata, size rejection, log retention, delay/priority/LIFO order and concurrency checks pass alongside native tests, 66 isolated protocol tests, generated worker lifecycle and HTTP/MCP startup. Broader advanced options remain open.

- [x] Validate BullMQ rate limits with concurrent workers and age retention cleanup limits against actual TypeScript/native consumers/producers. Measure start times and remaining old/fresh job identities/counts; strict worker clippy/formatting passes. Repeat/deduplication and broader options remain open.

IAM directory checkpoint (2026-10-11): real PostgreSQL transactions, strict workspace Clippy/tests, all 999 CLI tests and 422 embedded Rust assets pass. A fresh archive install compiles portable services, all worker/module families and both SDK generators, and passes the HTTP/MCP listener matrix and five signed capability callbacks. Generated IAM exercises thirty configured directory routes with request HMAC, asymmetric JWT/RBAC, storage-resolved organization scope, atomic batch rejection, authenticated MCP calls, persistent sessions and lifecycle operations. Full provider, compliance, mixed orchestration and release/deployment gates remain open.

IAM configured discovery/compliance checkpoint: direct conformance checks all 33 base-module routes, their actual RBAC constants and 22 request/response schema layouts. Generated IAM discovers 40 MCP tools (33 configured operations plus seven legacy session/health routes). HTTP/MCP verify public discovery, system-role authorization and the configured empty compliance behavior alongside all CRUD, both live SDKs, scoped atomic rejection and lifecycle checks. Generic entity compliance extensions and enabled Better Auth behavior remain open.

Better Auth foundations archive checkpoint: fresh packaged install `kSwMp4`
includes all native auth source/migrations and compiles/tests them after relocation.
Packaged live Axum HTTP/MCP integration, generated listener startup matrix and all
five native managed callbacks pass. Portable services/workers, both SDK generators
and strict generated consumers pass. Checkout CLI restored afterward. This does
not claim a generated Better Auth preset or its raw API endpoints are complete.

Better Auth HTTP archive checkpoint: packaged install `pUGSo8` at `ba1da0e00`
passes relocated native auth tests, including live session/JWT HTTP and MCP plus
secure cookies. Portable modules/workers, both SDK generators/strict consumers,
the complete HTTP/MCP startup/listener matrix and five signed managed callbacks
pass. Checkout CLI restored afterward. This snapshot includes the nine session/JWT
routes; the subsequent credential HTTP work and complete Better Auth preset are
separate gates.

Fresh packaged install `ac1c0T` validates the twenty-route auth checkpoint at
`54901dd98`, including relocated native auth tests and live HTTP/MCP, listener
startup configurations, portable modules/workers, CLI/native SDKs and all five
managed callbacks. Checkout CLI restored. Token-reset additions are validated
separately and are newer than this archive snapshot.

- [x] Match the configured server-only initial-password helper against 36 actual Better Auth API/storage outcomes, with strict credential identity, empty/missing passwords, session refresh/expiry and validation ordering. Verify one concurrent assignment wins in memory/restricted PostgreSQL and deterministic revocation during derivation prevents a write. Keep this helper server-only; remaining provider/org/JWT helper and CLI preset gates remain open.

Fresh archive `oLl8Px` at `953969c17` passes relocated auth tests (13 passed,
one explicit-PostgreSQL test ignored), live auth HTTP/MCP, all generated listener
startup configurations and five managed callbacks. Portable services/workers and
both SDK generators pass; checkout CLI restored. This snapshot includes 24 raw
auth routes and the server-only password helper, before organization additions.

- [x] Port organization creation, slug checks, listing, full graphs and active selection with actual configured default team/owner/keyed membership and session side effects. All 134 actual HTTP/storage/cookie comparisons pass; memory and restricted PostgreSQL check concurrent slug creation and HTTP/JWT organization claims beside MCP. Remaining organization/member/team/invitation/dynamic-role operations are still tracked by the open organization endpoint gate.

- [x] Add organization metadata/update/delete/permission routes with scoped stored dynamic grants, platform-only static-role denials, per-role composite authorization and actual deletion/session side effects. All 212 actual HTTP/storage/cookie comparisons pass, including validation/alias and malformed stored-grant errors. Memory and restricted PostgreSQL validate concurrent deletion and live granted HTTP mutations beside JWT/MCP lifecycle. Member/team/invitation/dynamic-role management and the complete generated preset remain open.

- [x] Port active member/role reads, role updates, removal and organization departure with exact configured owner safeguards, creator bypass ordering, scoped grants and team/session cleanup. All 146 actual HTTP/storage/cookie comparisons pass. Memory and restricted PostgreSQL validate a concurrent departure leaves one owner and preserves team counters; live HTTP membership operations run beside JWT/MCP lifecycle. Member listing/addition, team/invitation/dynamic-role management and generated preset gates remain open.

- [x] Add trusted server-only member creation with explicit-org/session-fallback behavior, exact role joining, duplicate/user/team/error ordering, configured membership limit and keyed team/counter effects. All 40 actual API/storage comparisons pass. Memory and restricted PostgreSQL verify duplicate and last-slot races; live HTTP rejects a public add-member route. Member listing, team/invitation/dynamic-role management and remaining helper/preset gates stay open.

Fresh archive `PHoCdV` at `a030eb345` passes relocated native auth tests (14 passed,
one explicit-PostgreSQL test ignored), auth HTTP/JWT/MCP and the complete generated
HTTP/MCP listener/startup matrix. Portable services/workers, both SDK generators
and five signed capability callbacks pass. This snapshot includes 38 raw routes
and server-only member/password helpers, before member listing. Checkout CLI is
rebuilt from the subsequent listing sources.

- [x] Port scoped member listing with schema/error ordering, public-user joins, filtered totals, stable en-US ICU sorting and ECMAScript Number/slice pagination. All 340 actual HTTP/storage/cookie comparisons pass, including unknown fields, malformed filter operators and orphan users. Memory/restricted PostgreSQL and live HTTP/JWT/MCP cover the new route. Real ORM collation/query conformance and remaining team/invitation/dynamic-role/preset gates stay open.

Fresh archive `YdLskR` at `05723e998` passes portable auth tests (14 passed, one
explicit-PostgreSQL test ignored), including ICU member listing, live auth HTTP/
JWT/MCP, all generated listener/startup configurations, portable workers/services,
both SDK generators and five signed capability callbacks. Checkout CLI is rebuilt
from the subsequent team sources.

- [x] Add organization/user team lists, team-member reads and active-team selection with exact scope, membership, permission, projection and cookie rules. All 318 actual HTTP/storage/cookie comparisons pass, including transient cookies, timestamp changes, retained memberships and malformed grants. Memory/restricted PostgreSQL and live HTTP/JWT/MCP validate team/session persistence. Team mutations, invitation/dynamic-role management, helpers and preset gates remain open.

- [x] Port team creation/update/removal with scoped dynamic grants, mutation-specific error ordering, field stripping, active/last-team protections and live pending-invitation cleanup. All 110 actual HTTP/storage/cookie comparisons pass. Memory/restricted PostgreSQL validate concurrent removal preserves the last team, cleanup and live HTTP mutations beside JWT/MCP. Team-member mutations, invitations, dynamic roles and remaining preset/provider gates remain open.

- [x] Add team-member addition/removal with required/coerced user IDs, scoped grants, shared keyed membership creation and exact counter/session effects. All 164 actual HTTP/storage/cookie comparisons and the 40-case server-only addition regression pass. Memory/restricted PostgreSQL verify duplicate-add and competing-remove races; live HTTP executes both routes beside JWT/MCP. Invitation/dynamic-role/provider/helper and generated preset gates remain open.
- [x] Match raw auth numeric parsing and serialized metadata against 114 actual HTTP/storage/cookie comparisons: overflow, IEEE-754 rounding, exponent formatting, negative zero, duplicate/index/insertion keys, literal serde marker keys, reserved metadata keys and update-only ISO date revival. Preserve update persistence before strict metadata response failures. All 233 framework and 101 blueprint workspace tests pass with unified JSON features; explicit restricted PostgreSQL auth, 96 JOSE cases and HTTP/organization/team/password regressions pass.
- [ ] Audit literal serde marker keys across shared generic request, MCP and persistence deserializers under unified arbitrary-precision/raw-value features; the auth boundary uses a literal-key-preserving raw parser. Unpaired surrogate/deep nesting and real ORM metadata conformance remain open before universal raw JSON parity.

Fresh archive `5DF6N8` at `f014f7547` passes portable auth tests, live auth
HTTP/JWT/MCP and the complete generated HTTP/MCP listener/startup matrix.
Portable services/workers, both SDK generators and five signed capability
callbacks pass. This snapshot contains 48 raw routes and both completed
server-only helpers. Checkout CLI rebuilt afterward (16.84 seconds).
