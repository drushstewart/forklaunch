# TypeScript package inventory for the Rust parity port

This inventory records every non-root framework/blueprint package in this checkout,
including private generated modules. A Rust crate or passing smoke test does not
mean the package has full parity. The implementation plan retains the unchecked
provider, route, DTO, storage, deployment and conformance work described below.

The current target has 31 published packages and 13 private package/module fixtures.
Root `framework/package.json` and `blueprint/package.json` are workspace tooling.
JavaScript runtime adapters map to equivalent Rust behavior rather than literal
Express/HyperExpress/Bun clones. Provider protocols and broker/storage formats
require explicit interoperability fixtures; sharing a Redis/Kafka server is not
proof that TypeScript and Rust producers/consumers can exchange jobs.

| Source package | Rust implementation | Remaining scope / evidence |
| --- | --- | --- |
| [`@forklaunch/bunrun`](../framework/bunrun/package.json) | CLI Cargo/watch and mixed orchestration | Rust build/run exists; mixed development orchestration remains. |
| [`@forklaunch/common`](../framework/common/package.json) | forklaunch-common | Shared names, paths, errors and JSON helpers; audit all utility exports individually. |
| [`@forklaunch/core`](../framework/core/package.json) | forklaunch-core, forklaunch-http, forklaunch-auth, forklaunch-contract, forklaunch-mcp | HTTP/contracts/config/DI/cache/object store/telemetry/encryption and an optional MCP HTTP server exist; SQL entity/compliance/retention services and full auth/streaming conformance remain. |
| [`@forklaunch/e2e-tests`](../framework/e2e-tests/package.json) (generated/private) | Rust wire/runtime suites | Actual TypeScript framework service conformance remains beyond the plain Node wire fixtures. |
| [`@forklaunch/express`](../framework/express/package.json) | forklaunch-axum | Equivalent HTTP behavior is the target; multipart/SSE and full validation/auth/error fixtures remain. |
| [`@forklaunch/hyper-express`](../framework/hyper-express/package.json) | forklaunch-axum | All 27 exported HTTP methods dispatch through shared validation/auth and nested contracts; both SDK artifact sources and actual compiled TypeScript router token forwarding are covered. Runtime-specific adapter internals need no clone. |
| [`@forklaunch/infrastructure-s3`](../framework/infrastructure/S3/package.json) | forklaunch-infrastructure-s3 | Async store, bounded streaming/multipart transfers, codecs, signed downloads and browser POST policies are implemented with real MinIO fixtures. POST size/type/key/expiry enforcement, AWS signature vectors, SDK credential cache/rotation and actual TypeScript policy fixtures are covered; ordinary SigV4 bucket endpoints only. |
| [`@forklaunch/infrastructure-redis`](../framework/infrastructure/redis/package.json) | forklaunch-infrastructure-redis | Async cache/queue/pubsub and real fixtures exist; audit remaining provider-specific options and typed codecs. |
| [`@forklaunch/internal`](../framework/internal/package.json) | contract macros, CLI embedded templates and Rust tooling | Map remaining exported build/runtime helpers; JavaScript compiler utilities remain in the TypeScript path. |
| [`@forklaunch/testing`](../framework/testing/package.json) | forklaunch-testing and runtime fixtures | Local calls and response helpers exist; actual cross-language conformance harness remains. |
| [`@forklaunch/universal-sdk`](../framework/universal-sdk/package.json) | forklaunch-sdk-rust, forklaunch-sdk-typescript, forklaunch-sdk-codegen and CLI SDKs | HTTP/multipart uploads, bounded CLI SSE responses and guarded recursive CLI models and omission/null presence exist through both artifact formats, with actual TypeScript conformance and live clients; native Rust typed HTTP/SSE responses and recursive models now share the CLI compiler; native TypeScript typed responses/models, remaining schema conformance, typed realtime and one mixed SDK surface remain. |
| [`@forklaunch/validator`](../framework/validator/package.json) | forklaunch-validation and schema contracts | JSON Schema validation/coercion exists; audit Zod/TypeBox exported behaviors against shared fixtures. |
| [`@forklaunch/ws`](../framework/ws/package.json) | forklaunch-ws, forklaunch-asyncapi | Typed native server/client transports now cover authenticated upgrades, serde/garde payloads, bounded I/O, validated ping/pong/close and bidirectional actual TypeScript interoperability. Optional Axum upgrades share HTTP/MCP listeners, preserve negotiated subprotocols and use the same typed engine. Raw socket metadata/checked AsyncAPI exports match actual TypeScript channel/operation behavior in 34 conformance documents, with standalone recursive and reachable legacy annotation schema relocation, once-decoded local pointers and official schema validation. References into literal assertion/default/example data now use separately bundled schema copies, preserving the original values and recursive targets under copy/output bounds. Optional compiled raw JSON Schema codecs validate original payloads before serde, with bounded compilation and disabled external retrieval; 4,104 actual-helper/AJV and 168 Zod cases pass. Generated native Rust clients share the typed transport, with actual TS-server and invalid-peer fixtures plus recursive/empty consumer compilation. Generated Node TypeScript clients pass strict ESM/CommonJS consumers, 4,104 codec comparisons, actual TS/native peers, bounded queues and cancellation/close cleanup. CLI `--from socket-contract` now generates multi-socket Rust/Node packages and checked AsyncAPI, with service selection, check/watch, custom artifact/output paths and validation before writes. Relocated live clients, parser conformance and HTTP/MCP startup pass. Rust service registries now export portable socket artifacts automatically, with custom contained paths and content-owned move/removal cleanup preserving edited/manual artifacts. Owned socket SDK module/package pruning, empty-registry watch recovery and service rename/delete now pass with preserved user files/compiler options and updated Cargo membership. External AsyncAPI 3.0.0 JSON WebSocket server imports now support native/CLI generation/check/watch, local schema extraction, explicit directions, checked disjoint messages, input ownership and SDK rename/pruning. Relocated strict consumers/live peers and HTTP/MCP startup pass. Broader AsyncAPI event/protocol profiles and binary payload metadata remain. |
| [`@forklaunch/blueprint-billing-base`](../blueprint/billing-base/package.json) (generated/private) | forklaunch-billing-axum + Rust CLI | Generated base module: 33 HTTP/MCP routes, both live SDKs, five persistent service families, compliance authorization, tenant isolation, encryption/audits, restart and CLI lifecycle pass. Fresh archive installation passes. Sixty-five actual configured mapper cases now audit schema resolution, HTTP stripping, local IDs, ownership, read normalization and missing-row failures. Mixed deployment remains. |
| [`@forklaunch/blueprint-billing-stripe`](../blueprint/billing-stripe/package.json) (generated/private) | forklaunch-billing-axum + Rust CLI | Generated Stripe module: 35 HTTP/MCP routes, authenticated provider services, both live SDKs, restart and CLI lifecycle pass, including fresh archive installation. Stripe raw callbacks and signed platform payment events now synchronize business state with durable tenant receipts. Managed outbound Stripe transport passes 23 actual TS gateway comparisons; generated keyless HTTP/MCP startup, signed provider calls, ignored local keys, incomplete configuration rejection and lifecycle pass. Provider reconciliation and mixed deployment remain. |
| [`@forklaunch/blueprint-cac-base`](../blueprint/cac-base/package.json) (generated/private) | forklaunch-cac-interface/base/axum and Rust init module cac-base | Generated module mounts HMAC lookup and JWKS/system-role compliance; no entities are registered, so authorized export/erase return upstream 404s. Uses verified claims or a trusted role resolver in place of the TS admin-only stub. Mixed coexistence, both SDKs, upgrade, rename/sync/delete and installed source archives pass. |
| [`@forklaunch/blueprint-client-sdk`](../blueprint/client-sdk/package.json) (generated/private) | forklaunch-blueprint-client-sdk and CLI SDK generation | Native SDK prototypes and generated HTTP packages exist; all added modules/mixed surfaces remain. |
| [`@forklaunch/blueprint-core`](../blueprint/core/package.json) (generated/private) | forklaunch-blueprint-core and framework crates | Core DTOs/contracts exist; compliance/service registrations and shared role/provider behavior remain. |
| [`@forklaunch/blueprint-ecommerce-stripe`](../blueprint/ecommerce-stripe/package.json) (generated/private) | forklaunch-ecommerce-axum; Rust CLI ecommerce-stripe module | Core 27 catalog/cart/order/payment routes plus health start over HTTP/MCP with PostgreSQL, HMAC, provider selection and durable inventory events. Live generated HTTP/MCP, both SDKs, separate inventory worker, restart persistence and init/sync/rename/delete checks pass. Tax/shipping calculators pass 32 actual TS comparisons; bulk catalog import passes actual TS and generated PostgreSQL HTTP/MCP/SDK fixtures. Checkout HTTP/MCP/SDK wiring passes eight actual TS controller comparisons, cross-process payment races and ten parallel-cart checks. Verified Stripe/PayPal callbacks pass actual TS business comparisons and cross-process capture/replay/inventory checks; 31 application routes plus health start over HTTP/MCP. Generated Compose starts the separate inventory worker; Linux HTTP/MCP, paid/cancelled delivery and graceful shutdown pass. Persistence compliance and interrupted capture reconciliation remain. |
| [`@forklaunch/blueprint-iam-base`](../blueprint/iam-base/package.json) (generated/private) | Rust init module iam-base | Generated sessions plus thirty configured directory CRUD/grant routes, JWKS/HMAC/RBAC, transactional organization scope and HTTP/MCP startup are covered. Configured discovery/compliance also match the actual all-`none` policy; application-specific metadata and provider lifecycle remain. |
| [`@forklaunch/blueprint-iam-better-auth`](../blueprint/iam-better-auth/package.json) (generated/private) | Native parity in progress | All 70 raw auth routes and server-only password/member helpers pass configured comparisons and encrypted PostgreSQL/live HTTP/MCP checks. Organization/member/team/invitation lifecycle is implemented; Application surfacing/compliance, remaining JWT helpers, real ORM/compliance/callbacks and CLI preset remain open. |
| [`@forklaunch/implementation-billing-base`](../blueprint/implementations/billing/base/package.json) | forklaunch-billing-base | Five asynchronous base services implement plan/subscription CRUD and checkout/payment-link/portal cache lifecycles, with typed DTOs, optional backups and tenant-RLS PostgreSQL persistence. 168 actual TS base-class operations and native concurrency/expiry/encryption/audit/rotation fixtures pass. Generated billing modules and HTTP/MCP startup now pass; 65 actual configured mapper cases now pass; Redis cache interop remains. |
| [`@forklaunch/implementation-billing-stripe`](../blueprint/implementations/billing/stripe/package.json) | forklaunch-billing-stripe | Version-pinned authenticated Stripe client and five persistent async provider services pass 21 official SDK transport cases and 144 actual TS wrapper operations with backup/cache lifecycle. Raw-body authentication is shared with ecommerce and rejects stale callbacks. Legacy synchronous adapter fails closed. Generated modules/contracts and HTTP/MCP startup now pass. Twenty-six actual TS webhook state/provider-effect comparisons pass, with paginated product updates and subscription reassignment protection. Native tenant-RLS webhook receipts and replica-safe platform nonces pass PostgreSQL gates. Managed outbound transport passes 23 actual TS gateway comparisons and native JSON capability signature checks. Configured mapper audit now covers 65 actual cases; reconciliation remains. |
| [`@forklaunch/implementation-cac-base`](../blueprint/implementations/cac/base/package.json) | forklaunch-cac-base | Mock codes and provider descriptor/lookup match actual TS fixtures; authenticated generated module, both SDKs and CLI lifecycle now pass. |
| [`@forklaunch/implementation-ecommerce-base`](../blueprint/implementations/ecommerce/base/package.json) | forklaunch-ecommerce-base | Six base services, PostgreSQL persistence, restricted-role tenant RLS and encrypted/audited order addresses. Actual TypeScript 208-operation base and 35-case catalog-controller conformance pass, alongside real PostgreSQL concurrency/security and bounded catalog reads. Generated module integration remains separate. |
| [`@forklaunch/implementation-ecommerce-paypal`](../blueprint/implementations/ecommerce/paypal/package.json) | forklaunch-ecommerce-paypal | Bounded OAuth/Orders/signature-verification client and shared-store payment wrapper. Actual TS conformance covers 22 client cases and 19 payment operations; live fixtures cover concurrent capture and rollback. Generated webhook/controller wiring passes live HTTP/MCP, provider verification and cross-process capture tests; interrupted-capture reconciliation remains. |
| [`@forklaunch/implementation-ecommerce-stripe`](../blueprint/implementations/ecommerce/stripe/package.json) | forklaunch-ecommerce-stripe | PaymentIntent creation/retrieval, resume, direct Connect fees and shared-store confirmation/failure. Fifteen actual TS scenarios and bounded live HTTP fixtures pass; secrets stay out of persistence. Generated raw-body webhook/controller wiring passes official SDK verification, actual TS controller comparisons and live HTTP/MCP/SDK checks. |
| [`@forklaunch/implementation-iam-base`](../blueprint/implementations/iam/base/package.json) | forklaunch-iam-base, forklaunch-iam-axum | Typed single/batch directory CRUD, configured mapper behavior, nested relationships, encrypted PostgreSQL/RLS, atomic audit/rollback, retained-key uniqueness and session persistence exist. Actual TS service/route/schema conformance covers 44 operations, all 33 configured routes and 22 schema layouts, plus actual configured compliance export/erase behavior. Provider/session integration and generic application compliance extensions remain. |
| [`@forklaunch/implementation-messaging-base`](../blueprint/implementations/messaging/base/package.json) | forklaunch-messaging-base | Hooks-only sent behavior, encrypted tenant Postgres persistence, audited reads, subject export/erasure and key rotation; actual TS service fixtures pass. |
| [`@forklaunch/implementation-messaging-twilio`](../blueprint/implementations/messaging/twilio/package.json) | forklaunch-messaging-twilio | Actual TS REST/status/callback/error conformance; bounded single-attempt dispatch, signed form callbacks and generated SDK/runtime fixtures. |
| [`@forklaunch/implementation-worker-bullmq`](../blueprint/implementations/worker/bullmq/package.json) | forklaunch-worker-redis optional bullmq feature | Official wire protocol passes actual TS/native producers/consumers, retry/backoff, default retention, waiting/active peeks and restricted ACL fail-stop. Generated lifecycle, per-attempt failure observers (including removed/draining jobs), HTTP controls and count-based retention pass. Lazy age retention, bidirectional abandoned-job recovery, stalled/start limits and retained/removed preprocessor failure callbacks now pass. A feature-gated outcome-data hook in the pinned vendored protocol preserves original events through deletion, without changing Redis scripts or existing failure notifications. Job defaults/metadata, size-limit rejection, bounded log retention, delayed/priority/LIFO ordering and concurrency also pass actual TS/native checks. Concurrent rate limiting and bounded age-cleanup limits also pass actual TS/native fixtures. Repeat/deduplication and broader advanced-option conformance remain open. |
| [`@forklaunch/implementation-worker-database`](../blueprint/implementations/worker/database/package.json) | forklaunch-worker-database | Persistent lease/runner/retry/dead-letter/retention/control suites exist; public option and cross-language DB-wire audit remains. |
| [`@forklaunch/implementation-worker-kafka`](../blueprint/implementations/worker/kafka/package.json) | forklaunch-worker-kafka | Transactional native producer/consumer/admin and generated/Linux suites exist; TS Kafka event-array envelope interoperability remains. |
| [`@forklaunch/implementation-worker-redis`](../blueprint/implementations/worker/redis/package.json) | forklaunch-worker-redis | Persistent lease/runner/admin/control suites exist; native protocol differs from TS, so interoperability remains explicit. |
| [`@forklaunch/interfaces-billing`](../blueprint/interfaces/billing/package.json) | forklaunch-billing-interface | Typed DTOs and asynchronous traits cover all five TS service families, including CRUD/update, provider fields, dates, party lookups and session lifecycle. Thirty shared application contracts plus compliance per variant pass 66 actual TS route and 28 actual Zod runtime/OpenAPI DTO schema comparisons, including enums, required fields, UUID IDs and provider-specific `stripeFields`. Generated runtime, SDK and compliance checks pass; callback/platform contracts now match actual TS metadata. |
| [`@forklaunch/interfaces-cac`](../blueprint/interfaces/cac/package.json) | forklaunch-cac-interface | Provider interface/DTOs, code-set and compliance contracts, authenticated runtime, generated module and both SDKs are validated. |
| [`@forklaunch/interfaces-ecommerce`](../blueprint/interfaces/ecommerce/package.json) | forklaunch-ecommerce-interface | Catalog/inventory/cart/order/payment DTOs and six service traits implemented; native integer money/stock and absent-versus-null validation. Thirty-one application contracts match actual TS route metadata; generated HTTP/MCP, both SDKs, worker and lifecycle fixtures pass. |
| [`@forklaunch/interfaces-iam`](../blueprint/interfaces/iam/package.json) | forklaunch-iam-interface | Core/session DTOs plus typed asynchronous user/role/permission/organization CRUD interfaces and configured contracts exist; generic application mapper/filter extensions and provider interfaces remain. |
| [`@forklaunch/interfaces-messaging`](../blueprint/interfaces/messaging/package.json) | forklaunch-messaging-interface | Send/record DTOs, status enum, async service/store traits and module contracts; optional metadata matches omission and rejects null. |
| [`@forklaunch/interfaces-worker`](../blueprint/interfaces/worker/package.json) | forklaunch-worker-interface | Async queue/admin/lease interfaces exist; TS producer/consumer option/event DTO mapping remains. |
| [`@forklaunch/blueprint-messaging-base`](../blueprint/messaging-base/package.json) (generated/private) | generated Rust messaging-base + forklaunch-messaging-axum | HMAC routes, JWKS/system-role compliance, encrypted Postgres/RLS/auditing, both SDKs, restart and CLI lifecycle fixtures. Shared MikroORM schema and generic retention metadata remain separate work. |
| [`@forklaunch/blueprint-messaging-twilio`](../blueprint/messaging-twilio/package.json) (generated/private) | generated Rust messaging-twilio + forklaunch-messaging-axum | Trusted-URL callback signature verification, exact upstream delivery transitions, both SDKs including forms, mixed-app init/rename/sync/delete. Callback verification intentionally strengthens upstream public controller. |
| [`@forklaunch/blueprint-monitoring`](../blueprint/monitoring/package.json) (generated/private) | forklaunch-monitoring-interface and core telemetry | Monitoring DTOs and exporter bootstrap exist; audit all generated monitoring registrations/instruments. |
| [`@forklaunch/blueprint-sample-worker`](../blueprint/sample-worker/package.json) (generated/private) | sample-worker and generated durable workers | Database/Redis/Kafka lifecycles exist; audit producer/consumer options, interop and BullMQ. |

## CLI extension surfaces outside published packages

`cli/src/constants.rs` also exposes the `relay` module, which injects an OAuth
session-ingest endpoint into an existing IAM service, and `init module storefront`,
which extends an app with its frontend. Rust/mixed support for both needs an
explicit behavior and lifecycle audit. They must remain in scope even though
there is no corresponding published blueprint package.

## Completion criteria

For each row, map exported service methods, DTO fields/enums, route/status/auth
contracts and provider options to implementation and fixture names. Validate
init/change/sync/delete, offline artifacts, both generated SDKs, relocated builds,
actual provider/storage behavior and mixed applications where applicable. The
package-level inventory is complete; detailed symbol/DTO and conformance mapping
is still in progress and the implementation-plan inventory task stays open.

Configured billing audit: HTTP parsing strips unknown fields and allocates local UUIDs for Stripe plan/subscription creates; optional `stripeFields` now deserialize in generated SDKs, including verified-webhook-created portals. Sixty-five actual configured mapper cases execute repository schema resolvers, Zod validators and mapper definitions over owned ORM boundaries. Intentional differences remain: native scalar coercions and provider-object validation are stricter; native cached Stripe resources retain provider IDs that configured TS mappers discard; native updates can use primary cache without an optional database backup. These are recorded differences, not literal full validator/mapper parity. Generated base/Stripe HTTP/MCP and both SDK regressions pass.

Managed payments: `forklaunch-blueprint-core::payments_client::PaymentsClient` implements instance-signed onboarding links and readiness status with typed DTOs, gateway status and positive finite numeric Retry-After hints. Sixteen actual TypeScript/native client cases cover routes, exact JSON-byte HMAC verification, fresh nonces, redirects, readiness, refusal/JSON failures and response bounds. Native redirect URL/response validation is stricter and remote error bodies remain redacted. This completes the payments helper; other managed capability helpers remain in scope.

Port managed SMS client, typed send/event DTOs and GSM/UCS accounting. Fifty-five actual TypeScript/native cases cover all GSM extensions, UTF-16 surrogate-pair and ten-segment boundaries, E.164, ECMAScript whitespace, default/promotional purpose, deferred refusal, signed JSON wire bytes, fresh nonces, opt-out/refusal statuses and retry hints. Native response DTOs are validated and remote errors redacted. CLI managed-capability registration/event stubs for native modules remain in scope; client coverage alone does not complete infra add/remove parity.

Port managed WhatsApp template/text/listing client and delivery DTOs. Twenty-one actual TypeScript/native comparisons cover template component pass-through, empty listings, recipient/text validation, Unicode whitespace, platform policy refusals (403/409/422/429), signed exact JSON bodies, nonce freshness and retry hints. Typed native responses reject malformed data. CLI capability registration/event routes remain open.

Port managed voice start/read/end and event DTOs. Forty-seven actual TypeScript/native comparisons cover catalog flow names, E.164, attribute key/value/count/UTF-16/byte limits, reserved prefixes, call-ID isolation, status, empty 200/204 hang-up responses, exact signed bodies, nonce freshness, refusal statuses and retry hints. Empty-response support is opt-in; existing provider JSON response validation remains strict. CLI capability wiring/event mounting remains open.

Port managed email send/suppression and delivery DTOs. Fifty-one actual TypeScript/native comparisons cover mailbox rules, scalar/array recipients and replies, 50-recipient bounds, UTF-16 subject length, UTF-8 body size, tag limits/reserved names, suppression lowercase and encodeURIComponent paths, signed body bytes, gateway refusals and retry hints. Test servers collect complete bytes before UTF-8 decoding, including a 512 KiB multibyte body. Native email responses remain typed; CLI capability/event wiring and model gateway remain open.

Add reusable platform-event verification with an asynchronous replaceable nonce-store interface and bounded atomic memory default. Thirty-four actual TypeScript/native comparisons cover exact raw bytes, method/path/key binding, minimal event shape, skew boundaries, replay, malformed payloads and same-event-ID redelivery. One hundred concurrent deliveries have one nonce winner. Native retains future-dated nonces through their full inclusive validity window, closing an actual TS replay gap, and bounds bodies/storage without evicting valid nonces. Memory is single-process; replica callers must supply shared storage, and event-ID business deduplication remains handler-owned. Generated capability routes/wiring remain open.

Add trusted managed-settings resolution and explicit fixed Docker gateway-mock transport. Fourteen actual TypeScript/native configuration cases cover missing/partial/complete settings, loopback, fixed mock opt-in, remote TLS, invalid opt-in and credential redaction; normal transport continues to reject remote plaintext. Generated Stripe billing uses the validated gateway constructor and passes HTTP/MCP, both SDKs, callbacks, restart, CLI lifecycle and invalid-opt-in startup rejection. Actual Docker capability add/remove/startup wiring remains open.

Add native managed-capability CLI wiring for email, SMS, WhatsApp, voice and payments. `infra add/remove` resolves the registered native crate path, stages typed client factories, handler stubs, callback contracts and shared HTTP/MCP middleware, and records source/dependency ownership. Clients share a gateway connection pool; SMS defers unmanaged refusal as the TypeScript registration does. Fresh relocated mixed applications compile both SDKs and pass HTTP/MCP startup, actual-TypeScript-signed callbacks for every feature, forgery/staleness/replay rejection, authenticated MCP boundaries, dry-run and stacked removal preservation. Untouched files restore byte for byte; human handler/Cargo/library/env edits survive. All 60 CLI infrastructure tests pass, including 31 existing TypeScript capability tests. Linux Compose verifies all five outbound clients over signed Docker-DNS calls and signed gateway-to-service callbacks, plus HTTP/MCP startup. Archive-installed CLI generation, service HTTP/MCP settings, callback validation and contract/SDK exports pass. Ownership snapshots are Git-ignored and private on Unix because restoration records may contain pre-existing local settings. Capability lifecycle rename/delete, safe removal without local ownership records, shared replica nonce storage, and in-place S3/Redis CLI wiring remain open.

Preserve native capability hooks and custom source through service sync, rename and deletion. Renames move private ownership records, update Cargo restoration snapshots and local callback targets, and update generated config/smoke-test names without rewriting customized files. Registered custom crate locations survive sync and rename. A fresh-clone recovery path removes generated hooks without a local ownership record while retaining dependencies/settings of unknown origin. Deletion removes ownership records and releases or retains the shared mock according to remaining manifest capabilities. Generated relocated mixed-app fixtures pass HTTP/MCP before/after rename, SDKs, six service smoke tests, custom nested crate paths, clone recovery and deletion; 62 infrastructure tests pass. Multi-service mock baseline/target preservation and human edits to managed dependency entries remain under review.

Track the local gateway mock at application scope so a custom pre-existing mock survives both service removal orders and deletion even when the first capability owner leaves early. Removing the active owner promotes another registered service; removing an inactive owner preserves the existing target. Rename handling retains exact host boundaries and URL suffixes. Real CLI/Compose configuration tests pass both orders, callback promotion, deletion and custom baseline restoration. Cargo ownership comparisons now use TOML values, avoiding false human overrides when sync changes inline dependencies into tables; custom features/paths/removals remain preserved. All 960 CLI tests and the expanded generated HTTP/MCP/SDK/lifecycle fixture pass. Edits made to the shared mock while capabilities are active still need ownership-aware merging; local-mock routing supports one target per feature.

Shared gateway restoration now merges active developer edits, preserves adopted mocks and honors explicit mock removal; real CLI/Compose ownership fixtures pass. Compose long-form network/port/volume parsing remains a separate CLI parity gap.

Generate a preserved `src/capabilities/nonce_store.rs` factory returning `Arc<dyn PlatformEventNonceStore>`. Callback dispatch creates one verifier with that store for every feature; application code can supply shared atomic storage without editing the owned dispatch section. A customized capacity-two store survives sync/rename, accepts email and SMS, then returns 503 for voice, proving shared capacity rather than a new store per callback. Default stub removal and custom factory retention pass alongside generated HTTP/MCP, SDK, source/dependency ownership, clone recovery and custom crate lifecycle checks. All 960 CLI tests pass; the default remains single-process memory and applications must configure shared storage for replicas.

Preserve standard structured Compose service fields through CLI edits. Native/TypeScript shared parsing accepts network maps, long ports/mounts and short dependencies, retaining aliases/IP assignments, port metadata, bind options, dependency required/restart flags and build arguments/secrets. Application rename retains those options and host port allocation understands addresses/protocols/ranges. All 964 CLI tests, normalized Compose capability ownership edits and generated HTTP/MCP, SDK and lifecycle fixtures pass. This closes the Compose parser gap; native Redis/S3 in-place CLI wiring remains open.

Wire native in-place Redis/S3 infrastructure commands and honor init/change infrastructure flags. Editable typed factories, relative Cargo dependencies, local settings, manifest resources and shared Compose backends support registered nested crate paths, sync, rename, removal, deletion and clone recovery. Customized factories/dependencies/settings and adopted backends remain application-owned. Generic service metadata edits preserve source. Fix relative directory paths and update ejection to use the source parent; capability removal no longer restores unrelated removed infrastructure settings or discards adopted/deleted gateway dependencies. All 967 CLI tests, real CLI/Compose ownership fixtures and relocated generated Redis/MinIO binary round-trips pass; HTTP/MCP enabled/disabled/custom-path startup and both SDK compilers pass. Factories currently expose raw adapters with explicit local S3 credentials; default production AWS credential-chain configuration and generated encryption/compliance wiring remain open.

Packaged CLI validation also passes native Redis/S3 generation alongside all five managed capabilities, relocated compiler checks, service HTTP/MCP settings, signed callbacks and contract/SDK exports. Fresh IAM, CAC and base/Twilio messaging runtime fixtures pass HTTP/MCP and lifecycle checks; the IAM fixture now rejects an actually unsupported module instead of the now-supported billing module.

Add production S3 environment resolution and an async generated factory using the official AWS default credential chain with profile/SSO/process/container-role support. Complete S3 key pairs override the chain; partial pairs follow the TypeScript fallback. Both factories honor namespace and presign caps. Thirty-one actual TypeScript/native configuration cases cover endpoint/region/key combinations, prefixes, numeric/radix/fractional values and ECMAScript whitespace. An owned ECS credential endpoint supplies session credentials used to sign actual SDK object/tag requests under the configured prefix. Generated relocated factories round-trip data through real Redis/MinIO using standard AWS environment credentials, pass HTTP/MCP settings and both SDK compilers. All 967 CLI tests, 18 S3 unit tests, the actual S3 backend test and strict all-target clippy pass. Preserve the synchronous explicit-local factory for compatibility; encrypted/compliance-aware cache and object-store wiring remains open.

The production S3 checkpoint also passes archive-installed CLI generation, portable framework compilation, HTTP/MCP settings, signed capabilities and contract/SDK exports. The package-source inventory now checks 268 embedded source/template assets, including the new S3 example directory and infrastructure/capability factories.

Align native Redis list queues with the actual TypeScript LPUSH/RPOP wire layout and add non-destructive batch peeks in dequeue order. Keep an explicit LegacyNative RPUSH/LPOP mode for existing native backlogs; producers/consumers must use one consistent layout. Actual TypeScript queue methods and native clients exchange data through the isolated Redis backend in both directions, covering batch/single FIFO, mixed producers, peeks and legacy backlog reads. Strict all-target clippy and all six Redis tests (including the actual backend fixture) pass. This closes list orientation interoperability; JSON/encryption/compliance, complete TTL/cache metadata and BullMQ worker wire conformance remain open.

Add shared fallible `JsonRecordCodec`, typed `JsonCache` and async `JsonObjectStore` wrappers plus explicit generated encrypted factories. Per-operation validated tenant context selects authenticated v2/v3 envelopes; encrypted reads without context, wrong tenants, malformed payloads and serialization failures fail closed. Plaintext JSON migration and key rotation work, including false/zero/null values. Pre-serialize cache batches before writing. Actual TypeScript Redis methods and native clients exchange encrypted JSON in both directions against Redis, covering rotation, expiry, corruption and tenant rejection. Relocated generated services use the factories against real Redis/MinIO with encrypted at-rest checks, metadata preservation and wrong-tenant rejection, then pass HTTP/MCP enabled/disabled/custom endpoint startup and both SDK compilers. All 967 CLI tests, five core tests and six non-ignored Redis tests pass; strict infrastructure all-target clippy and 273 packaged source assets pass. Direct TypeScript/native S3 storage comparisons, encrypted queue convenience methods and complete TTL/cache metadata remain open.

Add generated `encrypted_queue(name)` and typed enqueue/dequeue/batch/peek helpers, with TypeScript-compatible FIFO envelopes and explicit decode-after-removal semantics. Add transactional GET/PTTL JSON cache reads retaining subsecond expiry and persistent-key metadata. Actual TypeScript/native encrypted cache/queue exchanges pass v2/v3, both producer directions, batch order, non-destructive wrong-tenant peeks and false/zero/null preservation. Actual TypeScript S3 methods over their injected SDK boundary and signed native AWS SDK requests exchange v2/v3 JSON in both directions, including prefixes, content types, rotation, plaintext migration, misses and tenant/corruption rejection. Real MinIO encrypted factories remain covered by the generated runtime fixture. Fresh relocated encrypted-queue/service runtime passes HTTP/MCP settings, shutdown and both SDK compilers; strict infrastructure all-target clippy, six non-ignored Redis tests and 274 packaged source assets pass. The prior encrypted-factory checkpoint also passed archive-installed CLI generation, HTTP/MCP, managed callbacks and SDK exports. Per-record batch TTLs, complete S3 helper coverage and worker/BullMQ wire parity remain open.

Complete typed Redis per-record TTL batches with prevalidated keys/expirations and transactional value/expiry reads. `put_records` supports a separate TTL or configured default per entry; `read_records` keeps order, misses and persistent-key metadata; literal-prefix SCAN reads expose decoded records with expiry. Actual TypeScript/native batch writes and reads compare values and relative expirations in both directions, including encrypted envelopes. Invalid TTL/key batches leave the first valid entry unwritten, empty batches work, and glob characters are matched literally. The full Redis fixture, including the actual backend, passes all seven tests; strict all-target clippy and the extended encrypted cache/queue conformance harness pass. Retain the intentional native semantics of precise milliseconds, explicit persistent expiry, misses as Option and falsy-value preservation rather than TypeScript's rounding/filtering accidents. Worker/BullMQ interoperability remains open.

Add typed S3 JSON batch writes/reads and single/batch deletes, defaulting absent content types to application/json while preserving custom types/tags and ordered misses. Serialize/encrypt all JSON before starting batch writes; backend partial-write behavior remains explicit. Actual TS object-store methods (including JSON stream-upload aliases) and signed native AWS SDK calls pass encrypted single/batch exchange, false/zero/null payloads, deletes, missing objects and empty batches. A serialization-failure regression proves no S3 write occurs and serializer details stay redacted. Generated encrypted factories pass real MinIO single/batch writes, metadata/default-type reads, missing keys, deletes and wrong-tenant rejection, then HTTP/MCP settings/shutdown and both SDK compilers. Strict S3 all-target clippy, 19 unit tests and encryption-only feature tests pass; all 274 embedded source assets package correctly. Complete binary/stream/presign helper parity, worker/BullMQ interoperability and release gates remain tracked separately.

Wire platform Redis key/channel namespace conformance into native leased workers and Pub/Sub controls. Resolve REDIS_KEY_PREFIX before FORKLAUNCH_REDIS_KEY_PREFIX with actual TypeScript-compatible whitespace/separator behavior; preserve dedicated-cache keys and native cluster hash tags. Add injected RedisKeyScope/connect_scoped APIs and sanitized NOPERM errors. A real restricted ACL fixture compares actual TS prefix helpers, enqueues/claims/acks native jobs, fences tenants, delivers Pub/Sub controls and denies foreign keys/channels and FLUSHDB. The runner fails promptly on NOPERM rather than looping. The fixture exposed redis 0.32 PubSub subscribe silently discarding server-error payloads; add a bounded subscription-ack preflight without publishing control messages, and verify denied subscription startup fails. All seven Redis-worker unit tests and both actual Redis integration tests pass, as do strict all-target clippy, 276 packaged assets and generated worker runtime/control/retention/batches/relocation/rename/backend-change/delete regressions. This closes platform prefix/ACL startup behavior; BullMQ wire compatibility remains open and is being implemented separately.

Add an optional official BullMQ Rust protocol adapter and actual TypeScript/native interoperability fixture. Both producer/consumer directions pass single/batch jobs, fixed/exponential retries, terminal failures, duplicate event IDs, default retention, and waiting/active inspection against a real restricted Redis ACL. A denied native consumer fails promptly and drains. Strict all-feature blueprint workspace clippy, seven worker unit tests, both actual Redis integration tests, formatting and 278 packaged source assets pass. CI now runs infrastructure JSON/encryption and worker ACL/BullMQ conformance fixtures using pinned BullMQ 6.3.10. Generated BullMQ lifecycle support, native failure observers and complete option/retention coverage remain open.

Generate native BullMQ workers through init/change/sync/rename/delete with the official protocol adapter, preserved application handlers, retries/backoff/concurrency/timeouts, inspection and scoped optional Pub/Sub controls. Add HTTP health/readiness and authenticated pause/resume/drain with active-job shutdown, plus a watch-based control transition API. Per-attempt native failure observers retain original events through retries, automatic removal and graceful drain; consumer peeks work before start. Actual TS/native fixed/exponential retry and default/count-based retention fixtures pass both directions under restricted Redis ACLs. Fresh relocated generation passes explicit stub failures, real processing/retries, HTTP controls, service HTTP/MCP initialization/discovery/health/enabled/disabled/custom-path checks, contracts/SDKs and source-preserving lifecycle changes. A nested workers/team/jobs registration passes sync/reinit/backend conversion/rename/delete, closing hardcoded worker crate/dependency paths. All 967 CLI tests, strict Redis/database-worker clippy, three control-transition tests and 283 packaged assets pass. E2E CI includes the pinned BullMQ runtime fixture. Age-based retention, stalled-job/custom-option behavior and broader worker conformance remain separate gates.

Bind additional typed MCP tools to the real Streamable HTTP runtime. ManualTool::typed derives discovery metadata from FrameworkSchema, parses/validates before application work, supplies fresh headers for operation authorization and preserves transport authentication. Reject manual/contract name collisions at startup; normalize explicitly public handler failures, redact serializer details, bound invocation time/output and preserve object/scalar/array results. Eleven MCP tests exercise initialization/discovery/invocation, HTTP/manual coexistence, schema/type/range/unknown-field rejection before side effects, tenant credentials per call, timeout, output limits and serializer failures. Strict MCP/Axum and whole-blueprint all-feature clippy, formatting and 285 embedded assets pass. Fresh generated generic HTTP/MCP and SDK fixtures pass after the new dependency. Fresh IAM, CAC, base/Twilio messaging, billing/Stripe and ecommerce runtime/lifecycle suites also pass HTTP/MCP startup and authenticated dispatch. Separate listeners, multipart and complete actual-generator conformance remain open.

Add configurable separate MCP listener orchestration to generated generic, IAM, CAC, messaging, billing and ecommerce executables. MCP_LISTENER=separate uses the actual TypeScript HTTP-port-plus-2000 fallback; MCP_PORT/MCP_HOST select explicit/ephemeral ports and hostname/IP binding. Preserve the native shared-listener default explicitly. The second router exposes only MCP while tools retain internal HTTP validation/authentication; both listeners bind before serving, then drain together with a 30-second bound. Live two-listener tests verify route credentials, denied calls, concurrent active HTTP/MCP completion and closed ports. Fresh generated startup covers shared/separate/disabled/custom endpoints, explicit/ephemeral/default-offset ports, hostname binding, invalid settings and occupied-port rejection without a leftover HTTP listener. Generated BullMQ runtime and nested lifecycle regressions also pass. All 967 CLI tests, 31 Axum unit tests, two live MCP integration tests, eleven MCP tests, strict Axum/MCP clippy, formatting and 287 packaged assets pass. Multipart and remaining actual-generator conformance are still open.

Fix managed-capability wiring for the new serve_with_default_mcp entrypoint while retaining legacy into_router_with_default_mcp mounts. Insert the capability router exactly once and preserve byte-for-byte removal; reject ambiguous/missing mounts. A regression exercises all six current generated entrypoint templates plus the legacy call. All 968 CLI tests pass, and fresh billing/Stripe and ecommerce HTTP/MCP/SDK/capability/lifecycle fixtures pass after the correction. IAM, CAC and base/Twilio messaging fixtures also pass with coordinated listener startup.

Add MCP multipart text-field transport, File/Binary JSON-argument encoding and URL-encoded composite values matching the actual TypeScript generator. Generate collision-checked MIME boundaries and escape field names/line breaks; reject non-string multipart parts and SSE request bodies. Thirty-two actual-generator/body-discriminator comparisons and fifteen MCP unit/transport tests pass, including real Axum Multipart/Form/Bytes extractors behind route authorization. Strict full-framework all-feature/all-target clippy, formatting and 289 embedded assets pass. Expand the archive-installed CLI fixture with official BullMQ generation and assertions for typed, separate-listener and multipart MCP sources. Streaming, arbitrary multipart upload parts and broader MCP option conformance remain open.

Complete multipart uploads in both CLI SDK targets through contract and OpenAPI generation. TypeScript accepts FormData or typed field records and uses the actual shared safeStringify source for multipart/URL-encoded values; files, repeated names, JSON composites and special JavaScript values match thirteen actual universal SDK comparisons. Normalize header casing and let encoders own boundaries. Rust SDKs consume native reqwest forms, preserve authentication/hooks and repair reqwest zero-byte empty forms with a closing MIME delimiter. Preserve multipart bytes in both HTTP adapters. The framework Rust request builder now encodes owned multipart text/file parts and exposes dynamic content types; the framework TypeScript generator clears conflicting multipart headers. Strict standalone Rust/TypeScript compilation and standard MIME parsing pass; fresh relocated SDKs call Rust and TypeScript services successfully from both artifact formats. All 969 CLI tests, 31 Axum unit/two live MCP tests, fifteen MCP tests, both SDK unit suites, strict framework workspace clippy, formatting, 290 embedded assets and a fresh archive-installed CLI fixture pass. The archive fixture also typechecks its generated TypeScript SDK and verifies the bundled shared stringifier, all four workers, modules, capability callbacks and HTTP/MCP startup. Multipart responses, automatic typed extraction, SSE and remaining recursive/realtime SDK tasks remain open.

Add transport-independent bounded SSE encoding/decoding with arbitrary chunk boundaries, UTF-8/BOM, CR/LF/CRLF, multiline data, comments, IDs/resets, event names and retry metadata. Reject metadata injection, fail closed on event overflow and discard unterminated EOF frames according to the HTML standard. Eighteen HTTP tests, strict HTTP clippy, formatting and 292 embedded assets pass. Thirty-five actual TypeScript universal SDK comparisons pass for common terminated frames; CI now runs them. Document existing TS multiline/whitespace/EOF differences explicitly. Generated streaming clients, cancellation/reconnection and full typed runtime behavior remain open.

Generate bounded pull-driven SSE responses in CLI TypeScript/Rust SDKs from both contract and OpenAPI artifacts. Return typed async iterables or boxed owned Rust streams, preserve authentication/hooks, enforce configurable 1 MiB default event bounds and response MIME, deserialize Rust envelopes and release responses on iterator return/throw, abort, close/drop and terminal failures. Preserve complete events before later oversized events even when coalesced in one chunk. Live relocated service tests verify delivery before EOF, split UTF-8, BOM/multiline/CRLF/bare-CR framing, ID resets, auth before handler work, finite/truncated streams, wrong media, type/transport/size failures, unstarted cancellation, server stream cleanup and graceful HTTP/MCP shutdown. Both artifact formats pass, including strict generated-SDK clippy; the streams are boxed to keep response enums small, and request-hook type aliases close an existing clippy warning. All 970 CLI tests, nineteen HTTP codec tests, 31 Axum/two live MCP tests, fifteen MCP tests, both framework SDK suites, strict full-framework clippy, formatting and 298 embedded source/template checks pass. Thirty-five actual TS universal SDK/native codec/generated SDK SSE comparisons and thirteen form comparisons pass; ordinary HTTP/multipart/MCP SDK regressions pass against Rust/TypeScript servers from both artifacts. A fresh archive-installed CLI also passes portable generation, all four workers/modules, TypeScript SDK compilation, capability callbacks and HTTP/MCP startup; E2E CI now includes the dedicated streaming fixture. Framework-native typed response decoding, remaining emitter/schema conformance, recursive models and realtime/mixed SDK work stay open.

Support guarded recursive CLI SDK models through contract and OpenAPI artifacts: self/mutual records, optional/null fields, recursive arrays/dictionaries and object/array unions. Preserve named graph edges with bounded local reference processing; inline acyclic definitions to retain existing types. Reject external/missing references, validation siblings and unguarded aliases before writes. Bound cached wide-graph traversal as well as initial expansion, and omit Rust Default implementations across required recursive edges. Rust vectors avoid redundant boxes; collection aliases use transparent wrappers. Rebase standalone contract schema pointers when exporting OpenAPI components, escaping component names and retaining literal examples/defaults/constants. Fresh relocated clients pass strict TypeScript compilation (including a rejected invalid recursive request), strict Rust SDK clippy, recursive HTTP/MCP invocation and graceful shutdown through both artifact sources. All 977 CLI tests, 31 Axum/two live MCP tests, nineteen HTTP codec tests, fifteen MCP tests, six OpenAPI tests, both framework SDK suites, strict full-framework clippy/formatting and 299 embedded assets pass. Ordinary HTTP/multipart/SSE regressions and 40 query/thirteen form/35 SSE actual TypeScript comparisons pass. The expanded archive-installed CLI fixture compiles recursive clients through both artifact formats alongside all four worker types/modules and passes HTTP/MCP listener settings and managed callbacks. CI includes the recursive runtime fixture. Framework-native typed responses/recursive models, broader schema constraints, multipart responses and typed realtime/mixed SDKs remain open.

Share standalone-schema relocation between OpenAPI and MCP through the optional forklaunch-contract/json-schema feature. Rewrite schema-pointer edges only, retaining examples/defaults/constants/enums/extension data and properties named $ref; escape body/query/header/path field locations. Handle tuple/conditional and unevaluated schemas, and preserve embedded $id resource bases. CLI imports explicitly reject resource IDs and dynamic references until their resolver is implemented. Live recursive tool discovery preserves an example containing a literal $ref while HTTP/MCP invocations and both SDKs pass through both artifact formats. All 977 CLI tests, 31 Axum/two live MCP tests, sixteen MCP tests, five contract/five OpenAPI tests, nineteen HTTP tests, both SDK suites, strict full-framework clippy/formatting and 299 embedded assets pass. The contract crate also checks without default features. A fresh archive-installed CLI passes recursive client compilation through both artifacts, all worker/module compilation, HTTP/MCP shared/separate listeners and all five managed callbacks. Rust field omission/null distinctions, framework-native response/recursive models, resource/anchor resolution and broader realtime/mixed SDK work remain open.

Generated Rust SDKs now preserve field presence: absent optional fields are omitted, explicit null requires a nullable schema, optional nullable fields retain both states, and required nullable fields reject omission. Ninety-four actual TypeScript validator cases cover nested records, arrays and unions through both artifact formats, strict SDK compilation and Rust/TypeScript HTTP with MCP startup/invocation/shutdown. Correct the canonical TypeScript null schema enum from the string `"null"` to JSON null; all fourteen Zod tests pass. Box generated JSON model status payloads to bound enum size. All 977 CLI tests, HTTP/multipart/recursive/SSE regressions and 299 embedded assets pass; a fresh archive-installed CLI passes both SDK formats, all four workers/modules and HTTP/MCP listener settings/capability callbacks. Framework-native typed responses/recursive models, broader schema constraints and realtime/mixed SDKs remain open.

Share the CLI schema graph/type compiler, Serde model renderer and Rust client/SSE templates with framework-native generators. SdkGenerationPlan retains complete JSON schemas and response media kinds. The checked native Rust HTTP generator emits typed status/business-error responses, recursive models, exact omission/null presence, auth/hooks, multipart uploads and bounded owned SSE. Reject invalid graphs/media/statuses and reserved/conflicting names; bound literal annotations before cloning and JSON strings before parsing. Native clients pass the 94-case actual validator fixture against both language servers, recursive shapes and live SSE cancellation/resource cleanup with HTTP/MCP startup/invocation/shutdown. All 978 CLI tests, eight codegen/four Rust SDK/three TypeScript SDK tests, full all-feature framework tests, strict workspace clippy/formatting and 311 embedded assets pass. Ordinary HTTP/multipart and original native media builders pass regressions. An archive-installed CLI also compiles both artifact formats/all worker/module families and runs the native recursive generator from portable bundled sources. Native TypeScript typed responses/models, advanced schema/resource constraints and realtime/mixed SDK work remain open.

Share the TypeScript CLI fetch/SSE/form runtime and template structures with the checked framework-native HTTP generator. Emit typed request fields/bodies, status/business-error unions, guarded recursive models and bounded owned SSE; preserve omission/null states and escape control characters in schema literals/property names. Both native generators pass 95 actual validator cases, recursive models and live SSE cancellation/resource cleanup against HTTP/MCP services. Media tests caught and fixed request MIME parameter loss and empty Blob/ArrayBuffer uploads in the shared runtime. Native/CLI request media validation now preserves full headers while rejecting control-character injection. All 979 CLI tests, full all-feature framework tests, strict workspace clippy/formatting and 319 embedded assets pass. Multipart responses, broader schema constraints/resource resolution and realtime/mixed SDK work remain open.

Revalidate generated IAM, CAC, base/Twilio messaging, base/Stripe billing and ecommerce HTTP/MCP startup, discovery and invocation. Their complete runtime suites pass SDK, authenticated tenant/provider/persistence and lifecycle checks; ecommerce includes actual TS webhook comparisons and a separate durable inventory worker. Thirteen actual universal SDK form and 32 actual MCP generator/body comparisons pass. Fix stale messaging fixture signatures to follow the already-shared single-JSON-value record encoding while retaining explicit repeated URLSearchParams coverage.

A fresh archive-installed CLI passes relocated all-worker/module compilation, both SDK artifact formats, HTTP/MCP shared/separate/default/explicit listener settings and all five capability callbacks. Both native recursive generators run entirely from the portable embedded source tree; generated Rust SDK strict clippy and TypeScript compilation pass. Quoted request MIME profiles containing apostrophes/ampersands also pass both CLI languages/artifacts against Rust and TypeScript wire servers. Correct HTML-escaped metadata in shared source templates. An additional actual cross-fetch probe confirms an open SSE adapter gap (`response.body.getReader` is absent on its Node stream); this is the next runtime task.

Fix Node fetch-adapter SSE compatibility in the shared TypeScript runtime. Read native Web Streams or Node async-iterable bytes; own per-stream abort controllers to close cross-fetch upstream requests, forward caller aborts and remove listeners at completion. Actual cross-fetch 4.1.0 and native fetch pass both CLI artifacts and native clients, including imported-fetch generation, pending-read cancellation, unstarted return/throw, abort, MIME/size/transport errors, event ordering and exact server resource cleanup with HTTP/MCP graceful shutdown. All 979 CLI tests, strict framework clippy/formatting, native SDK tests, media and ordinary HTTP regressions pass. CI installs pinned cross-fetch and runs both transports. A separate probe finds cross-fetch Node upload gaps: native Blob bodies fail `.pipe`, and standard FormData is coerced to `[object FormData]`; those are next.

The SSE transport checkpoint also passes a fresh archive-installed CLI: both SDK artifact formats, portable native Rust/TypeScript generators, all four worker/module source families, strict generated Rust SDK clippy, TypeScript compilation, HTTP/MCP listener matrix and all five managed callbacks. Checkout CLI rebuilt after installer validation.

Fix actual cross-fetch Node upload failures by sharing a Web-body compatibility encoder across CLI checked TypeScript clients and dependency-free native TypeScript clients. Native fetch retains standard upload bodies; custom adapters receive Blob bytes or standard-encoded multipart bytes/MIME. Native, injected and imported cross-fetch pass raw/typed SDK byte comparisons, sliced arrays, Unicode/text/quoted vendor profiles, repeated and empty multipart files, URL-encoded composites and typed status/business errors. Thirteen actual universal SDK form, forty URL and 35 SSE comparisons pass. All 979 CLI tests, nine codegen/four Rust SDK/four TypeScript SDK tests, strict workspace clippy/formatting, fresh ordinary HTTP/SSE runtime regressions and 321 packaged assets pass. A fresh archive install passes both native generators, SDK formats, all worker/module families and HTTP/MCP listener settings/managed callbacks.

Decode schema URI fragments once in shared native/CLI SDK models and OpenAPI reference objects. Reuse canonical recursive edges, preserve literal percent names, and emit URI-safe relocated references after pointer-token escaping. Generated recursive Rust/TypeScript fixtures and raw codec comparisons cover Unicode/slash/tilde/percent definition names. Full parity remains open for schema resources/dynamic scope, external AsyncAPI imports, raw binary metadata and the broader feature gates above.


### Checked external WebSocket AsyncAPI import checkpoint

Import explicit AsyncAPI 3.0.0 JSON WebSocket server documents through `forklaunch_asyncapi` and CLI generation/check/watch. Validate offline against the licensed official specification schema, extract bounded transitive/recursive local payload graphs, preserve Draft 7 constraints, map server receive/send to client outgoing/incoming, retain disjoint multi-message `oneOf` rules and reject undeclared directions. Imported documents remain unchanged; per-target source ownership supports module pruning and service rename/delete. Mixed native/imported targets reject output/input collisions before writes. Relocated strict Rust/TypeScript consumers, live peers, official parser checks, configured watch recovery, invalid later-service staging and default/custom input lifecycle pass. Generated and renamed services pass the HTTP/MCP shared/separate listener, endpoint, port, atomic rejection and shutdown matrix. All 998 CLI tests, 22 AsyncAPI tests, full framework tests, strict workspace clippy/formatting and 342 packaged source assets pass. A fresh archive-installed CLI also passes external import, both SDK compilers, all worker/module families, HTTP/MCP startup and all five managed callbacks. Broader event projects, other payload dialects/protocols, JSON string wire encoding, upgrade parameters/authentication, resource-scoped schemas and richer message matching remain tracked separately.

### BullMQ abandoned-job and retention checkpoint

Actual TypeScript/native conformance passes age-based lazy retention in both producer directions, abandoned-job recovery in both consumer directions, maximum stalled/start attempts and retained/removed preprocessor failure callbacks. The pinned, licensed protocol source adds an optional outcome-data notification while preserving existing notifications and Redis scripts. All 66 protocol library tests pass on isolated Redis; full blueprint tests, strict workspace clippy/formatting and native Redis integration tests pass. Generated worker lifecycle and HTTP controls pass alongside service HTTP/MCP startup. All 998 CLI tests and 410 embedded source assets pass; a fresh archive install compiles relocated services/workers, both SDK families and portable native generators, and passes HTTP/MCP startup and all five managed callbacks. The vendored protocol also checks with its optional feature disabled. Broader worker options remain under audit.

### BullMQ job-default compatibility checkpoint

Preserve supported queue defaults before overriding wrapper retry/backoff settings and intentionally clearing default job IDs, matching actual TypeScript `add`/`addBulk`. An optional vendored protocol patch persists log/stack-trace limits, timestamps and size limits and reads the compressed Node log-limit key. Actual TypeScript/native fixtures pass persisted metadata, auto-generated IDs, retry overrides, oversized-job rejection before writes, bounded processing logs, delay/priority/LIFO order and concurrency limits. Strict worker all-target clippy, native tests and all 66 isolated protocol tests pass; generated worker runtime/lifecycle and the complete HTTP/MCP startup matrix pass again. Broader options and stack-trace processing semantics remain separate audit work.

### Checked schema pointer-profile checkpoint

Reject percent-encoded JSON-pointer separators at active schema locations before checked SDK, raw AsyncAPI export or AsyncAPI import generation. Preserve literal assertion/annotation data, encoded names, escaped tokens and general OpenAPI reference-object URI decoding. The codec conformance harness now records invalid-schema compilation rather than assuming every schema compiles; 4,320 actual TypeScript/native/generated codec comparisons pass, including rejected leading separators. Full framework tests, strict workspace clippy/formatting, 999 CLI tests and 410 packaged source assets pass. A relocated imported service passes rejected-profile generation/check with unchanged inputs/SDK files, both strict/live SDKs, watch/lifecycle and original/renamed HTTP/MCP startup matrices. Raw native/AJV interior-separator resolution beyond this checked profile remains an explicit separate task.

The checked pointer-profile checkpoint also passes a fresh archive-installed CLI: portable framework/blueprint and all worker/module families compile after relocation, both SDK artifact formats and native generators pass, and HTTP/MCP listener startup plus all five managed callbacks pass. This archive also contains the BullMQ job-default/log-retention compatibility patch.

BullMQ rate limits with concurrent workers and per-completion age-cleanup limits now pass actual TS/native conformance. Tests measure processing start times and retained job identities/counts, with strict worker all-target clippy and formatting passing. No runtime patch was required for these options.

Raw native socket codecs now match AJV's per-token URI-fragment decoding,
including interior encoded slashes, once-decoded percent names, Unicode,
annotation targets, recursive references and embedded resource scopes. The
isolated, renamed jsonschema/referencing dependencies preserve upstream RFC
behavior by default and opt socket codecs into AJV pointer and older-draft
`$ref` sibling validation without rewriting any schema data. All five dialects
pass 12,960 actual TypeScript-helper/AJV and generated TypeScript codec comparisons. Resolver library tests
pass in both modes (150 each); 1,992 upstream validator library tests pass,
with one test requiring an absent upstream Git-submodule fixture explicitly
filtered and documented in provenance. Checked SDK/export/import profiles
continue rejecting encoded separators. Broader parity remains open.

Fresh generated IAM, CAC, base/Twilio messaging, base/Stripe billing and
ecommerce runtime suites all pass after the schema compatibility update:
HTTP/MCP startup and authenticated calls, both SDKs, persistence, tenant/RBAC
checks and init/rename/sync/delete. IAM also exercises all 33 configured routes
and the retained credential/session routes.

The fresh archive-installed CLI passes portable relocation with both isolated
validator source trees, licenses, provenance, locks and bundled meta-schemas.
All module/worker families compile, native and TypeScript SDK generation
passes, and generated HTTP/MCP startup plus all five managed callbacks pass.
Strict framework workspace linting/formatting, framework tests, 999 CLI tests
and all 591 embedded Rust source/template assets also pass.

The configured Better Auth 1.7.6 reference audit now captures 70 HTTP routes
and four server-only helpers, with 48 actual protocol checks. It executes the
repository config plus real plugins, discovery and surfacing services against
an owned UUID/SQL adapter boundary. Signed/revoked cookies, password rotation,
Origin/callback validation, EdDSA/JWKS claims, default organization teams and
configured role denials are covered. Native Better Auth protocol, real ORM,
Google callback/provider behavior and account PCI compliance remain open.
Password reset/change-email/user-deletion/reference UI are disabled by actual
configuration; Google discovery lists the provider even without credentials.
Configured static access-control permissions do not create surfaced dynamic
role records. The implementation plan now tracks these scopes separately.

The actual Better Auth custom callback helpers now enforce configured trusted
callback origins, use the configured token-exchange authority, escape script
JSON and clear the configured session-cookie name/secure flag under HTTPS.
Forty checks execute the real server handlers and generated browser scripts
with actual session/JWT handlers under owned app/fetch/storage boundaries.
Untrusted destinations fail before token exchange; forged Host headers cannot
redirect exchange requests; malicious query values remain JSON data. Native
callback/helper implementation remains in the Better Auth module task.

Better Auth invitation expiry is corrected from milliseconds to seconds,
matching the intended 48-hour lifetime. A trusted operator-seeded dynamic
invitation grant exercises the actual handler and checks its persisted expiry;
the actual protocol fixture now passes 49 checks. Stored dynamic role grants
augment configured static roles, so native organization behavior must support
those grants as well as the default permission denials.

Native application-owned HTTP namespaces now support the raw Better Auth API's
routing model. Mounted Axum handlers retain global limits/request IDs while
remaining outside exported contracts and MCP discovery. Fixed-prefix validation
also covers parent nesting; namespace collisions with registered routes, docs
and MCP are rejected. Live HTTP cookie checks, exact MCP tool discovery,
authenticated invocation and shutdown pass, along with the full Axum suite and
strict framework clippy. Better Auth handler/storage implementation remains open.

The isolated native `forklaunch-iam-better-auth` crate now matches configured
password hashes and signed session-cookie primitives. A bidirectional fixture
runs 322 comparisons against actual Better Auth/Better Call, including Unicode
normalization, cross-language password verification, secure cookie attributes,
quoted/duplicate cookies, tampering and malformed URI data. CLI embeds the new
crate; protocol endpoints, persistent sessions and preset selection remain open.

Native Better Auth JWT/JWK primitives now match the actual configured Ed25519
signing profile and 15-minute claims, with 26 cross-library comparisons against
Better Auth/JOSE. Restoring private key bytes preserves IDs and signatures;
public JWKs exclude secrets. Durable encrypted key storage, key lifecycle and
the HTTP token endpoint remain open.

Native Better Auth storage now covers all eleven configured models with atomic
memory and fully encrypted PostgreSQL backends. Live database tests cover forced
tenant RLS, rejected bypass roles, atomic rollback/audit behavior, read-only
transactions, private key restoration, blind-index uniqueness across key
rotation and record/model ciphertext binding. Its schema migration ledger is
independent of other modules. Auth endpoint semantics, graph relationships and
indexed/paginated queries beyond the 4096-row scan bound are still open.

The subsequent keyring checkpoint completes encrypted signing-key persistence,
atomic initialization, live-key selection and operator rotation. Memory and
live PostgreSQL verify service reconstruction and concurrent callers; 27 actual
Better Auth/JOSE comparisons verify selection ties, default 30-day public-key
grace boundaries and expired-key replacement. Corrupted public/private key
metadata fails closed. Session-authenticated HTTP integration remains open.

The configured TypeScript Google provider now maps given/family name claims into
required application user fields. The actual handler previously rejected valid
verified Google sign-in with `400 MISSING_FIELD: firstName is required`. The new
fixture executes real RS256/JWKS verification and auth handlers, owning only the
Google certificates HTTP boundary, with account/session reuse and invalid token
rejection. Native Google OAuth and real ORM conformance remain open.

Actual session lifecycle now has 60 additional checks: exact hourly refresh,
creation-based freshness, explicit refresh suppression, scoped revocation,
transient sessions, replay rejection and millisecond expiry boundaries. The
configured get-session POST descriptor returns 405 without deferred refresh.
Fresh archive `kSwMp4` passed relocated auth foundation tests, live HTTP/MCP,
the complete generated startup/listener matrix, all five managed callbacks,
portable services/workers and both SDK generators; checkout CLI was restored.
Better Auth raw endpoints/preset and indexed auth lookups remain open.

The native persistent session backend now matches actual hourly refresh,
creation-based freshness, revocation scoping and transient expiry behavior.
The fixture passes 63 checks and 48 native body/storage comparisons, with shared
credential records seeded at the storage boundary. Memory and real encrypted
PostgreSQL lifecycle tests also pass. Native signup/sign-in, production HTTP
cookies/Origin/CSRF integration and indexed lookup remain open.

Native email signup/signin now commits encrypted user/account/session records
atomically. Thirty-nine comparisons execute actual configured email handlers and
match response/storage behavior, callbackURL naming, case normalization and
UTF-16 password limits. Memory/live PostgreSQL exercise concurrent signup, and a
controlled password-change race proves stale verification cannot mint a session.
Bounded blocking password work stays outside tenant write locks. HTTP validation,
cookie/CSRF/Origin integration, credential/account updates and indexed queries
remain open.

Native credential comparisons now total 81, including actual authenticated
password verification/change handlers. Wrong and oversized passwords preserve
credentials/sessions; revocation replaces all own sessions with one new persistent
session and preserves unrelated users. Memory and encrypted PostgreSQL tests cover
the same lifecycle. Production raw HTTP validation/cookies/Origin/CSRF and the
remaining user/account/provider/organization routes stay open.

Native trusted-origin and mutation/login CSRF policies now pass 2,822 comparisons
against the actual Better Auth matcher and configured HTTP handlers. Wildcard and
custom schemes, relative redirects, encoded traversal/separators, Cookie header
presence, null origins, Referer fallback and Fetch Metadata are covered. These
policies still require wiring into the production raw HTTP adapter.

Nine native raw session/JWT HTTP endpoints now integrate signed-cookie sessions,
Origin/callback guards, credentialed CORS and public JWKS/token issuance. The
get-session response emits a verifiable JWT header. A 161-comparison fixture checks
actual HTTP bodies, errors, persisted state and JWT claims. Live memory and
restricted-role encrypted PostgreSQL exercise HTTP/MCP startup, discovery/calls,
authentication, CORS/CSRF and shutdown; secure-cookie extraction/clearing is tested.
Raw auth endpoints remain outside the registered MCP contract surface. Credential
HTTP, providers/orgs, custom callbacks and the generated Better Auth preset are open.

Fresh packaged install `pUGSo8` validates the nine raw session/JWT routes from
bundled sources, including live HTTP/MCP and secure-cookie tests. Portable
services/workers, CLI/native SDKs, listener startup configurations and all five
managed callbacks pass. Checkout CLI restored; credential HTTP additions and the
remaining Better Auth preset are tracked separately.

Thirteen raw native HTTP endpoints now include credential signup/signin and
password verification/rotation. The credential HTTP fixture passes 122 actual
body/storage/cookie/callback comparisons; 63 parser/schema/guard/method cases match
full errors. Transient replacement cookies inherit the browser flag, raw additional
profile values preserve presence, private input fields stay ignored, and forwarded
IP/agent metadata matches. Live memory/restricted-role PostgreSQL exercise these
credential operations with MCP startup and shutdown. The 161 session/JWT HTTP
comparisons remain green. Production rate limiting now matches 153 actual HTTP
responses, including rolling idle windows, fallback/IPv6 buckets, normalized paths,
special endpoint rules, retry headers and pre-parser/CSRF ordering. Bounded
process-global memory indexes preserve insertion eviction; live TCP concurrency
admits exactly three of 32 requests and shuts down cleanly. User/account/provider/
organization routes, real TypeScript ORM conformance, callbacks and CLI generation
remain open.

Sixteen raw native auth routes now include profile updates and linked-account
listing/unlinking. Fifty actual HTTP response/cookie/storage comparisons pass,
including opaque profile fields, private token/hash filtering, transient cookies,
ECMAScript scope trimming and scoped errors. Atomic concurrency preserves one
remaining account, and revoked sessions cannot update profiles. Live memory and
restricted-role PostgreSQL verify profile HTTP operations beside MCP. Remaining
user/account/provider/org routes, real ORM conformance and CLI preset stay open.

Twenty raw native auth routes include the four configured-disabled reset-email,
verification-email, change-email and delete-user endpoints. Their 128 actual HTTP
response/cookie/storage comparisons preserve schema/Origin/session ordering and
empty deletion 404 bodies. Live memory/restricted-role PostgreSQL validate these
responses beside MCP. Token-based reset/verification, remaining user/account/
provider/org operations, real ORM and CLI preset remain open.

Fresh archive `ac1c0T` passes the twenty-route auth checkpoint at `54901dd98`: relocated
auth HTTP/MCP tests, portable modules/workers, both CLI/native SDK languages,
listener startup configurations and all five managed callbacks. Checkout CLI
restored. Subsequent token-reset work is newer than this snapshot.

Twenty-one native raw auth routes include single-use token password reset. Its
44 actual HTTP/storage comparisons cover body/query token precedence, validation,
UTF-16 password limits, duplicate selection/invalidation, expiry, missing users,
missing credential creation and session retention. Atomic consumption allows one
concurrent winner and hashing runs outside tenant locks. Live memory and
restricted-role PostgreSQL exercise reset and subsequent verification via HTTP
beside MCP. Email verification/callback routes, remaining user/account/provider/
org operations, real ORM conformance and CLI preset remain open.

Twenty-three raw native auth routes include GET reset-token and disabled deletion
callbacks. Fifty-two actual HTTP/storage comparisons cover trusted destinations,
raw encoded token segments, combined query errors, redirect query/content type,
configured authority despite forged Host headers and expired-verification cleanup.
GET redirects retain valid tokens for single-use POST reset. Live memory and
restricted-role PostgreSQL follow that lifecycle over HTTP beside MCP. Email
verification, remaining user/account/provider/org operations, real ORM conformance
and CLI preset remain open.

Configured HS256 email-token verification passes 96 actual JOSE/HTTP comparisons,
including signature/algorithm/critical headers, malformed claims, optional numeric
claim ordering, expiry boundaries, padding/noncanonical bits, BOMs, duplicate keys
and numeric overflow. Raw JSON number handling is local to token verification.
Email-verification HTTP/user/session/redirect integration remains open; native
raw route count stays twenty-three.

Twenty-four native raw auth routes now include signed email verification. Its 52
actual HTTP response/storage/cookie/header comparisons cover idempotent normal
verification, confirmation/verified/legacy email-change tokens, authoritative
session checks, session reuse/creation and transient cookies. Error callbacks
append existing query text; success callbacks preserve relative locations, Latin-1
byte strings and HTTP whitespace normalization. Malformed claims retain empty 500
responses. Live memory and restricted-role PostgreSQL verify HTTP mutations,
forgery rejection, issued cookies and subsequent session/JWT access beside MCP.
Configured email delivery/change requests remain disabled. Provider/org/account
operations, real ORM conformance, custom callbacks and CLI preset stay open.

Reset credential selection/update also requires accountId=userId, preserving
mismatched legacy rows and creating a proper credential when none matches. The
44-case actual HTTP fixture preserves hash equality groups to detect accidental
updates to legacy rows; memory and restricted PostgreSQL exercise both paths.

The native server-only `CredentialService::set_password` helper matches 36 actual
configured Better Auth API/storage comparisons. It creates only strictly scoped
credentials, fills empty/null/missing passwords and rejects already-set passwords;
authoritative session refresh/expiry and password validation retain actual order.
Memory and restricted PostgreSQL exercise concurrent initial assignment, legacy
row preservation and empty credentials. A deterministic memory-store race revokes
the session after derivation and proves no password is written. No public route is
added for this server-only API; the native raw HTTP count remains twenty-four.

Fresh archive `oLl8Px` at `953969c17` passes relocated auth tests (13 passed,
one explicit-PostgreSQL test ignored), live auth HTTP/MCP, all generated listener
startup configurations and five managed callbacks. Portable services/workers and
both SDK generators pass; checkout CLI restored. This snapshot includes 24 raw
auth routes and the server-only password helper, before organization additions.

Twenty-nine native raw auth routes include organization creation, slug checks,
listing, full organization graphs and active selection. All 134 actual configured
HTTP/storage/cookie comparisons pass. Creation atomically adds the owner, default
team and keyed team membership; it updates active organization/team unless told
to preserve the current selection. Metadata remains serialized in storage/list
responses and parsed in creation responses, matching the configured adapter.
Selection/full reads enforce membership and persist rejection-driven clearing
without clearing the active team. Password/session HTTP regression remains green.
Live memory and restricted PostgreSQL verify organization HTTP, session/JWT
organization claims, cookies and MCP discovery/invocation/shutdown. Organization
updates/deletion, remaining member/team/invitation/dynamic-role APIs, provider
flows, real ORM conformance, custom callbacks and CLI preset remain open.

Thirty-three native raw auth routes include metadata-only organization reads,
updates, deletion and permission checks. All 212 actual HTTP/storage/cookie
comparisons pass. Dynamic stored grants merge into each configured static role
and remain scoped to one organization; composite roles cannot pool permissions
across separate roles to satisfy a single request. Platform-only default roles
retain mutation denials. Schema/alias ordering, malformed permission JSON/schema
(including numeric overflow), field whitelists and endpoint-specific error bodies
and statuses match actual handlers. Deletion removes organization/member/invite
records and clears only the actor's matching active organization; configured
team/role rows and other sessions' selections remain, matching the actual adapter.
Memory/restricted PostgreSQL cover granted mutations, atomic concurrent deletion
and HTTP storage persistence beside JWT/MCP startup/lifecycle. Remaining member,
team, invitation, dynamic-role management, provider, real ORM, callback and CLI
preset gates remain open.

Thirty-eight native raw auth routes include active member/role reads, role updates,
member removal and organization departure. All 146 actual HTTP/storage/cookie
comparisons pass, including creator bypass boundaries, dynamic grants, duplicate
roles, Unicode trimming, error ordering and email-versus-ID user projections.
Departure/removal delete team memberships and release stored counters atomically;
only the actor's matching active organization clears, retaining active team.
Memory and restricted PostgreSQL exercise competing owners leaving, ensuring one
owner survives, and live HTTP member operations alongside JWT/MCP lifecycle.
Member listing, server-only addition, team/invitation/dynamic-role management,
provider flows, real ORM conformance, callbacks and CLI preset remain open.

The trusted server-only `OrganizationService::add_member` helper matches 40 actual
API/storage outcomes. Typed inputs preserve role arrays by joining them verbatim;
the explicit organization requires no session, while an optional session provides
active-scope fallback and refresh/expiry effects. User/team/organization checks,
duplicate ordering, the configured 100-member limit, keyed team membership reuse,
undercount reconciliation (including its timestamp) and retained overcounts match
the configured adapter. Memory/restricted PostgreSQL verify concurrent duplicate
and last-slot admission; live HTTP rejects public add-member access. Raw auth
route count remains 38. Member listing and the remaining team/invitation/dynamic
role, provider, real ORM, callback, JWT-helper and CLI preset gates remain open.

Thirty-nine native raw auth routes include scoped member listing. All 340 actual
HTTP/storage/cookie comparisons pass across schema ordering, active/ID/slug scope,
filtered totals, public-user projection, missing-user failures, stable date and
Unicode sorting, all filter operators and ECMAScript Number/slice pagination.
Unicode sorting uses ICU's en-US collation, matching the fixture's Node locale;
real ORM collation and query behavior remain a separate open conformance gate.
Memory/restricted PostgreSQL cover filtering, Unicode pages and orphan failures;
live HTTP listing runs beside session/JWT and MCP lifecycle. Team/invitation/
dynamic-role management, provider flows, remaining server JWT helpers, real ORM,
callbacks and generated preset remain open.

Forty-three native raw auth routes include organization/user team lists, team
member reads and active-team selection. All 318 actual HTTP/storage/cookie
comparisons pass, including self scope across organizations, permission-gated
other-user reads, malformed grants, distinct organization/team membership guards,
internal counter/key projections, transient cookies and selection timestamps.
Selection preserves the active organization; clearing an existing selection emits
a cookie once, while successful selection emits one even when unchanged. Memory
and restricted PostgreSQL verify team/session persistence and live HTTP alongside
JWT/MCP lifecycle. Team mutations, invitations, dynamic-role management, provider
flows, remaining JWT helpers, real ORM/callback and generated preset gates remain
open.

Forty-six native raw auth routes include team creation, name updates and removal.
All 110 actual HTTP/storage/cookie comparisons pass, including mutation-specific
authorization and unauthenticated error ordering, schema/field stripping, stored
grant failures, active-team protection and last-team safeguards. Creation stores
zero members without enrolling the actor; updates retain organization/counters.
Removal atomically deletes memberships and removes the team's comma-separated
references from the first 100 pending invitations that have not expired. Expired/
accepted invitations and other sessions' active-team selections remain unchanged.
Memory/restricted PostgreSQL verify concurrent removals preserve the last team,
invitation cleanup and live HTTP mutations beside JWT/MCP lifecycle. Adding and
removing team members, invitations, dynamic roles, providers, server JWT helpers,
real ORM/callback and generated preset gates remain open.

Forty-eight native raw auth routes include team-member addition/removal. All 164
actual HTTP/storage/cookie comparisons pass, covering scoped member permissions,
required/coerced user IDs (including ECMAScript number formatting and failed
object coercion), keyed duplicate reuse, legacy duplicate removal, counter
reconciliation/release and retained active-team references. Organization addition
shares the same membership helper; its 40 actual API/storage comparisons remain
green. Memory/restricted PostgreSQL verify concurrent additions reuse one row and
seat, concurrent removal has one winner and sessions retain active-team values.
Live HTTP runs the new routes beside JWT/MCP lifecycle. Invitation/dynamic-role
management, providers, server JWT helpers, real ORM/callback, raw JSON numeric
edge cases and generated preset gates remain open.

Raw auth JSON/metadata now passes 114 actual HTTP/storage/cookie comparisons,
including numeric overflow, IEEE double rounding/formatting, index/insertion key
ordering, literal serde marker keys, reserved-key failures after persistence and
update-only ISO date revival. All 233 framework and 101 blueprint workspace tests
pass with unified JSON features; explicit PostgreSQL auth passes. Shared generic
deserializer marker keys, unpaired surrogates/deep nesting and real ORM metadata
behavior remain separate gates. Fresh archive `5DF6N8` validates the preceding
48-route snapshot's generated service/worker HTTP/MCP startup matrix, both SDK
generators and five managed callbacks; checkout CLI rebuilt afterward.

Fifty-one raw routes include invitation lookup, scoped organization lists and
received-user lists. All 258 actual HTTP/storage/cookie comparisons pass, with
verified-email ordering under configured database-generated IDs, exact expiry
boundaries, joined inviter projection, cross-organization visibility and the
adapter's 100-row limit before user-list status filtering. HTTP rejects explicit
email queries; the trusted native helper accepts an email without a session.
Native memory/restricted PostgreSQL and live HTTP/JWT/MCP cover invitation reads.
Invitation creation/acceptance/rejection/cancellation, dynamic roles, providers,
JWT helpers, real ORM/callback, shared parser edges and preset gates remain open.

Fifty-three raw routes include rejection and cancellation. All 318 actual HTTP/
storage/cookie comparisons pass: rejection requires a pending invitation and
verified recipient but accepts expired invitations; scoped invitation:cancel
can replace any invitation status and repeated cancellation succeeds. Neither
operation changes membership or active selections. Malformed stored grants,
missing organization/member ordering and unsupported methods match. Memory/
restricted PostgreSQL verify repeated/concurrent cancellation and retained
membership; live HTTP executes both operations beside JWT/MCP. Creation and
acceptance, dynamic roles and remaining full-preset gates stay open.

Fifty-four raw routes include invitation creation/resend. All 196 actual HTTP/
storage/cookie comparisons pass across schema/error ordering, scoped grants,
verbatim roles and dynamic role existence, creator-role restrictions, duplicate
members/invitations, team validation and pending-invitation limits. Resend updates
expiry while retaining stored role/teams and runs before team/limit checks. The
adapter's 100-row limit precedes expiry filtering. Memory/restricted PostgreSQL
verify concurrent last-slot admission and retained team/member state; live HTTP
executes creation/resend beside JWT/MCP. Acceptance and remaining dynamic-role/
provider/helper/ORM/callback/preset gates stay open. Fresh archive `v8nGhW` at
`51973fa19` validates the preceding 53-route snapshot's portable services/workers,
HTTP/MCP startup matrix, both SDK generators and five signed callbacks. Checkout
CLI is restored from the subsequent creation sources.

Fifty-five raw routes include invitation acceptance. All 186 actual HTTP/storage/
cookie comparisons pass across recipient/verification/error ordering, exact
expiry, 100-member admission, duplicate organization memberships, verbatim roles,
team/key/counter enrollment, active scope and transient cookies. Single-team
acceptance selects that team and emits cookies; absent/multiple teams preserve
the existing active team and emit none. Memory/restricted PostgreSQL validate
competing acceptance and last-slot admission, plus rollback of invitation/team/
member/session state after missing later teams. Live HTTP executes no-team and
single-team acceptance beside JWT/MCP. This completes configured invitation
lifecycle; dynamic roles, providers, JWT helpers, real ORM/callback, shared parser
edges and generated full-preset gates remain open.

Fifty-seven raw routes include dynamic role listing/lookup. All 272 actual HTTP/
storage/cookie comparisons pass: explicit ac:read grants, raw membership, nullish
scope fallback, exact case-sensitive name/ID selection, union validation and
first-100 listing. Direct lookup can read later roles; malformed later JSON
fails lookup without breaking capped lists, and literal serde keys are retained.
Permission strings stay unchanged in storage while responses contain parsed JSON.
Memory/restricted PostgreSQL and live HTTP/JWT/MCP validate both routes. Dynamic
role mutations and remaining provider/helper/ORM/callback/full-preset gates stay
open.

Fifty-eight raw routes include dynamic role creation. All 212 actual HTTP/storage/
cookie comparisons pass, including nullish scope, lowercase names without
trimming, the configured predefined-name guard, permission/resource ordering,
duplicate names beyond 100 rows and ignored additional fields. Public grants
permit only the configured platform resource, and each requested action requires
an actor grant. Missing actions retain order and duplicates in missingPermissions.
Memory/restricted PostgreSQL verify atomic competing duplicate creation and stored
permission JSON; live HTTP runs creation beside JWT/MCP. Role update/deletion and
remaining provider/helper/ORM/callback/full-preset gates stay open.

Sixty raw routes include all five configured dynamic-role operations. Update and
deletion pass 332 and 324 actual HTTP/storage/cookie comparisons. Updates enforce
per-action grants before renaming, reject same-name renames, update duplicate
matches and retain member role strings. Stored updatedAt advances while the
response retains the prior projection. Deletion checks the first 100 matching
members after substring filtering, then exact trimmed roles, and removes matching
stored rows. Name-based predefined protection differs from ID selection. Memory/
restricted PostgreSQL verify atomic deletion, blocked assigned roles and duplicate
updates/deletes; live HTTP runs both routes beside JWT/MCP. Providers, server JWT
helpers, real ORM/callback, shared parser and full-preset generation remain open.

Fresh archive `bnXBOp` at `10349eea0` passes portable Better Auth tests and live
HTTP/JWT/MCP, the complete generated HTTP/MCP listener/startup matrix, portable
services/all four worker backends, CLI/native SDK targets and all five capability
callbacks. This archive includes sixty raw auth routes and the two completed
server-only helpers, including the configured invitation/dynamic-role lifecycle.
Remaining provider/helper/ORM/callback/shared-parser/full-preset gates stay open.

The configured server-only verifyJWT helper now supports persisted Ed25519 keys
with EdDSA/Ed25519 token aliases and issuer overrides. All 159 actual API and 144
key-store comparisons pass: optional claims, numeric/expiry boundaries, audience/
subject truthiness, critical headers, compact encoding, JWK metadata/import rules,
retired keys and first-100 key selection. Invalid tokens/storage/key metadata
return null without minting keys or reading private material. Memory/restricted
PostgreSQL verify restart/retirement and corrupt-private-key independence; live
HTTP confirms signing/verifying helper paths remain absent beside JWT/MCP. Three
of four configured server helpers now have native implementations. Signing,
additional-algorithm provisioning/interop, providers, real ORM/callback, shared
parser and full-preset generation remain open.

All four configured server-only helper entrypoints now have native implementations.
The configured EdDSA signJWT profile passes 129 actual API and 134 key-state
comparisons, with independently verified signatures. Cover schema/default-duration
ordering, key persistence after claim failure, exact claim setters, issuer/audience
precedence, fractional/negative/overflow durations and numeric key rotation. Memory/
restricted PostgreSQL exercise signing and verification beside key lifecycle; live
HTTP/JWT/MCP retain absent public helper paths. Additional signing-algorithm/key
provisioning, typed date/function options, custom callbacks and real ORM/storage
interop remain open; unsupported fresh key profiles fail explicitly. Sixty raw
HTTP routes remain implemented, with provider/OpenAPI/full-preset work outstanding.

Main through `b5852a5a1` is merged. Its dependency vulnerability audit now covers
Cargo's crates.io releases through the shared OSV/RustSec cache and reporting path.
Legacy/v3/v4 locks, local-package exclusions, malformed/future locks, managed-source
exclusions and per-ecosystem missing-lock detection pass; Git/private-registry
sources explicitly report incomplete coverage without exposing source URLs. All
1,016 CLI tests pass, including 15 dependency-audit tests. Commit-based Git advisory
queries remain unsupported.

JWT key profiles now include Ed25519 alias, ES256/P-256 and ES384/P-384. Native
PKCS8 persistence retains encrypted private material while public JWKS exposes
only coordinates. Actual TypeScript comparisons pass 151 signing calls/165 key
states and 213 verification calls/182 key-store operations. Memory/restricted
PostgreSQL validate concurrent provisioning, restored signing/session reuse,
rotation and retired public-key verification; all 17 native tests and live HTTP/
JWT/MCP pass. Strict all-feature clippy, rebuilt CLI and 709 embedded sources pass.
RSA/ES512, typed options, callbacks, real ORM and full preset remain open.

ES512/P-521 now joins the persistent signing/verification profiles. Actual
comparisons pass 159 signing API calls/176 key states and 240 verification calls/
201 key-store operations, including independent JOSE signature verification. Memory
and restricted PostgreSQL cover atomic provisioning, restart, session signing,
rotation and retired-key verification. Strict default/all-feature clippy, rebuilt
CLI and 709 embedded sources pass. RSA and broader option/provider/ORM/callback/
full-preset gates remain open.

RSA RS256/384/512 and PSS PS256/384/512 now support encrypted PKCS8 persistence,
restored signing/session reuse and public verification. Actual comparisons pass
189 signing calls/216 key states and 393 verification calls/309 key-store
operations, with independent JOSE verification and invalid hash/algorithm/PSS
salt rejection. Memory/restricted PostgreSQL validate concurrency, restart,
rotation and retired keys. Prime generation uses the blocking executor; default
session issuance prefers EdDSA before another live profile. Final keyring tests,
strict all-feature clippy, rebuilt CLI and 711 embedded sources pass; live HTTP/
JWT/MCP and 27 prior keyring comparisons pass. RSA provisioning supports 2048–4096
bits; larger keys and typed options/custom adapters/provider/ORM/full-preset gates
remain open.

Fresh archive `n6okrp` at `74213970a` passes 18 portable auth tests (one explicit
PostgreSQL test ignored), including live HTTP/JWT/MCP and all asymmetric signing
profiles. The complete generated HTTP/MCP startup/listener matrix, portable
services/all four worker backends, CLI/native SDK targets and all five signed
capability callbacks pass. This snapshot includes sixty raw auth routes, four
configured server helper entrypoints and Ed25519/ECDSA/RSA/PSS provisioning.
Larger RSA/typed options, Google OAuth, ORM/callback/shared-parser and full-preset
gates remain open.

Database OAuth state primitives now pass 66 actual state/cookie and 66 storage
comparisons, using real Better Auth endpoint context, Better Call and the memory
adapter. Native random state/PKCE, signed cookies, expiry/cleanup, malformed and
legacy data, link coercion and atomic consumption pass memory/restricted
PostgreSQL checks. Strict all-feature clippy, rebuilt CLI and 715 embedded sources
pass. Google HTTP authorization/callback and token/account lifecycle remain open;
this foundation adds no raw routes.

Configured Google authorization URL construction now passes 37 actual provider
comparisons: S256 PKCE, scopes, callback URI, login hints, reserved parameters,
overwrite behavior, Unicode/query encoding and JavaScript numeric-key ordering.
Strict all-feature clippy, rebuilt CLI and 717 embedded sources pass; CI runs the
fixture. HTTP social-login/ID-token user-account-session integration, callback
code exchange and token lifecycle remain open. No raw route is added yet.

Google ID-token verification primitives now pass 147 comparisons against the
actual provider, including RS256 signature/JWK import behavior, issuer/audience,
nonce, numeric claim boundaries and duplicate key selection. The verifier takes
trusted supplied certificate keys; HTTP certificate fetching and persistent
user/account/session integration remain open. Strict all-feature/all-target
clippy, rebuilt CLI and all 719 embedded Rust sources pass; CI runs the fixture.

Configured `/sign-in/social` now supports Google authorization URL/state issuance
and direct verified ID-token sign-in with persistent user/account/session
resolution. All 165 actual HTTP/storage comparisons pass, including nested body
validation, missing provider/claims, forged/expired tokens, duplicate-key order,
profile preservation, token replacement and safe verified-email linking. Google
certificate transport has fixed trusted endpoints, no redirects, timeouts and a
one-MiB response bound. The centralized direct sign-in verifier imports all
selected keys and uses the first, matching the configured TypeScript path.
Concurrent signup, orphan rejection, encrypted tokens and tenant isolation pass
memory/restricted PostgreSQL checks. Authorization-code callbacks and the
remaining account/token HTTP lifecycle remain open.

Google authorization-code callbacks now pass 226 actual HTTP/storage/wire
comparisons, including S256 verifier binding, trusted fixed token endpoints,
refused redirects, one-time state/cookie correlation, exact ten-minute expiry,
provider errors, POST normalization, success/new-user/error redirects and stored
access/refresh/ID tokens. Token-response normalization covers numeric coercion,
invalid dates and scope forms. Explicit callback linking preserves account
ownership, verified-email policy and merged scopes; malformed provider tokens
match the upstream empty-500/cookie-header boundary. Native memory tests cover
identity races, link conflict rejection and token/scope preservation. The same
checks pass encrypted restricted PostgreSQL with tenant isolation. Strict default and
all-feature/all-target clippy, all 21 native auth tests, rebuilt CLI and 725
embedded sources pass. The `/link-social` initiation route, access/refresh-token
and account-info routes, OpenAPI/error/update-session routes, full ORM/application
hooks and CLI preset remain open. The implemented raw-route count is 62 of 70.

A fresh relocated archive installation (`VwxVQI`, snapshot `f1a966960`) passes
all portable services and four worker backends, CLI/native SDK generation and
all 21 portable auth tests. The complete generated HTTP/MCP startup matrix
passes shared/separate listeners, enabled/disabled/custom endpoints, explicit
and ephemeral ports, TypeScript +2000 fallback, atomic startup rejection and
graceful shutdown. MCP initialization/discovery/health invocation/Origin
rejection and all five actual-TypeScript-signed capability callbacks pass. This
snapshot contains 62 raw auth routes and Google code callbacks; later linking
initiation work is validated separately. The checkout CLI was restored after
the archive install.

Google `/link-social` now supports direct verified ID-token linking and browser
authorization with server-owned user/email state. Existing own accounts update
tokens; new links enforce verified provider email and matching local email;
another owner's provider identity returns a conflict. The combined social/link
HTTP fixture passes 289 actual response/storage comparisons, including nested
schemas and ignored direct-link fields. The browser callback fixture passes 236
actual response/storage/wire comparisons through the real linking entrypoint.
Direct-link ownership/policy/token checks pass memory and encrypted restricted
PostgreSQL. Configured `/update-session` matches the TypeScript plugin field
policy: protected organization/team fields cannot be set, and there are no
configured editable session fields. The implemented route count is 64/70.
Access-token refresh, account-info, OpenAPI/error routes, full ORM/application
hooks and CLI preset remain open.

Google `/get-access-token`, `/refresh-token` and `/account-info` now pass 157
actual response/storage/wire comparisons. Account selection uses local owned
account IDs; automatic refresh observes the five-second boundary and preserves
stored scopes and omitted provider fields. Live provider tests verify token
rotation and reject an account unlinked during an in-flight refresh. All 22
native tests, encrypted restricted PostgreSQL, strict all-feature clippy, the
161 HTTP and 289 social regressions, rebuilt CLI and 728 packaged assets pass.
The implemented raw-route count is 67/70. Error/OpenAPI/reference routes,
full ORM/application hooks and the native CLI auth preset remain open.

Configured `/error`, `/open-api/generate-schema` and disabled `/reference` now
complete all 70 raw routes. The default error HTML is adapted under the upstream
MIT license and matches full rendered responses, URLSearchParams first-value
selection, preserved entities and production redirects. The OpenAPI response
uses the fixed preset's declarative contract with the trusted server URL; it is
compared in full with the actual configured TypeScript generator. Custom plugin
schemas/options are outside this fixed preset. All 27 development and 27
production public-response comparisons, 22 native tests and strict all-feature
clippy pass. Full ORM/application hooks, application surfacing/compliance,
retention and native CLI preset integration remain open.

Fresh archive installation `IqcM1T` (snapshot `4f0540ba6`, all 70 raw routes)
passes 22 portable auth tests, all portable services/four worker backends and
CLI/native SDK generation. The complete generated HTTP/MCP startup matrix,
initialization/discovery/health invocation/Origin rejection, shared/separate
listeners, enabled/disabled/custom endpoints, explicit/ephemeral ports,
TypeScript +2000 fallback, atomic startup rejection and graceful shutdown pass.
All five actual-TypeScript-signed managed capability callbacks pass. The
checkout CLI was restored after the archive installation. Later application
surfacing changes are validated separately from this snapshot.

Native application `SurfacingService` now matches 37 actual TypeScript result
comparisons. It selects the newest unexpired organization-bearing session,
retains exact member role strings, and returns stored permission strings
without parsing, splitting or unioning them. Expiry boundaries, duplicate
ordering, other users/organizations and missing/falsy scopes are covered. The
shared memory/restricted PostgreSQL check passes with ciphertext inspection
and tenant isolation; strict clippy and 734 packaged assets pass. HTTP/MCP
application-route wiring, compliance/retention and CLI preset remain open.

Real MikroORM 7.2.3 / PostgreSQL conformance exposed a TypeScript adapter
integration defect: compliant password/private-JWK objects reached Better Auth
without explicit plaintext access, causing sign-in and session-JWT 500s. The
configured adapter now reveals classified results only inside Better Auth's
trusted request context, including transaction adapters, preserving audit reads
and leaving ORM entity fields wrapped. All 24 real-ORM checks pass: encrypted
columns, audited access, missing/wrong tenant rejection, rollback, restart and
native consumption of actual ORM passwords/sessions/accounts with JWT checks.
The adapter utility passes a strict isolated type check against the installed
adapter signature. Existing 161 HTTP, 157 token, 27 public and 37 surfacing
comparisons pass. CI now installs the pinned real ORM/adapter and supplies shared
fixture environment settings. This is initial real-ORM conformance; complete
storage/hook coverage, compliance/retention and CLI preset remain open.

Native Better Auth compliance now preserves classified export fields as opaque
objects with explicit audited plaintext access. Fourteen comparisons against
the real encrypted ORM cover Account/Jwks selection, other subjects, erasure,
remaining user/session records and partial per-entity read failures. Native
memory/restricted PostgreSQL also verify atomic rollback on write failure,
report labels, tenant isolation and ciphertext protection. Erasure rechecks
ownership before deleting a previously selected account.

Seven-calendar-year Account retention passes 41 actual TypeScript comparisons:
UTC, Los Angeles, Lord Howe and Apia cutoff behavior, leap days, repeated hours,
half-hour gaps, skipped days, exact age boundaries, dry runs, entity filters,
batches, repeat runs, preserved metadata and persisted anonymization timestamps.
The implementation validates timezone round-trips and selects the earliest
valid UTC instant. All 26 native auth tests, restricted encrypted PostgreSQL,
strict all-feature clippy, rebuilt CLI and 739 embedded assets pass. Application
HTTP/MCP wiring, custom callback helpers, full ORM/hook coverage and CLI preset
integration remain open.

The optional `application-http` feature now combines the five configured
application contracts with the raw `/api/auth` namespace. Discovery is public;
role/permission surfacing verifies request HMAC; compliance export/erasure
verifies JWKS JWTs and resolves the `system` role from current trusted storage.
Role claims in tokens do not bypass revocation. All five actual TS controller
contracts and nine response schemas match, including string IDs and opaque
permission strings. A live TCP test verifies HTTP and MCP initialization,
discovery, signed tools, missing/invalid credentials, role revocation, classified
export protection, erasure/replay, raw-route separation and graceful shutdown.
All 27 native tests, strict all-feature clippy and 742 packaged assets pass.
The native CLI preset, custom callback helpers and remaining full ORM/hooks
coverage are still open; this does not declare full parity.

Native CLI generation now accepts `iam-better-auth` in Rust/mixed applications
and module initialization. It retains the default service name `iam`, rejects
conflicting IAM presets before writing files, embeds the encrypted auth store,
and emits operator configuration, migrations, application contracts and health.
The relocated generated fixture passes sync/reinitialization preservation,
TypeScript/Rust SDK compilation, real restricted PostgreSQL startup, tenant
session isolation, independent JWT verification, HMAC MCP tools, shared/separate
and disabled MCP listeners, restart persistence, graceful shutdown, rename,
contract/SDK refresh and deletion. All 1,016 CLI tests pass. E2E CI includes the
fixture and archive installation includes the new preset. Custom callback
helpers, additional docs sources and complete ORM/hooks remain open.

Fresh archive installation `XXfFfM` (snapshot `f288c81c4`) passes with the
new native Better Auth preset: portable service and SDK compilation, all 27
packaged auth tests, every other portable service/four worker backends,
CLI/native SDK generation and the complete generated HTTP/MCP startup matrix.
All five actual-TypeScript-signed managed capability callbacks pass. The checkout
CLI was restored after archive installation. Callback-helper changes after this
snapshot have separate validation.

Both configured internal popup helpers are native. `/api/auth/test-callback`
validates trusted destinations, exchanges through the existing auth handlers,
clears only the configured session cookie and replaces the destination fragment
with JWT/session/email values. `/api/auth/oauth-redirect` emits matching
script-safe OAuth/SSO HTML, including duplicate-query coercion. Neither becomes
an MCP tool. Ninety-two actual Express 5.2.1 / TypeScript comparisons cover HTTP
and HTTPS, invalid origins/userinfo/schemes, anonymous/signed/forged/expired
cookies, corrupted signing keys, response media/Accept negotiation, HEAD, cookie
headers and HTML escaping. Independently generated JWTs verify against their
own public keys and match claims; signatures are not compared byte-for-byte.

The relocated generated service also passes these helpers against a fresh
PostgreSQL container, plus shared/separate/disabled HTTP/MCP listeners, tenant
isolation, SDKs, restart, graceful shutdown and rename/delete. All 27 native
tests, strict default/HTTP/all-feature clippy, 40 original callback checks,
161 raw HTTP regressions, five application contracts/nine response schemas and
749 packaged sources pass. `application-http` no longer enables unused SQL or
field-encryption features unless PostgreSQL is selected. Additional docs
sources, nondefault base paths, complete ORM/hooks/client integration and
broader custom options remain open.

### Additional Scalar documentation sources

`RouterConfig.docs_sources` accepts typed URL or inline JSON sources with optional
slug/default selection. Scalar receives the generated application API first,
then the additional documents. BetterAuth applications now include the complete
raw auth schema without adding its endpoints to OpenAPI application contracts,
SDK operations or MCP tools. The TS preset preserves the schema's existing
server URL and relative paths, fixing its duplicated `/api/auth` request paths.
Documentation titles and both Scalar/Swagger inline configuration are escaped.

Twelve comparisons execute the actual pinned Scalar 0.10.25 renderer, the actual
TS registration factory and Better Auth schema generator, then evaluate served
native HTML in a JS VM. They cover remote/inline/string documents, multiple
sources, slugs/defaults and script terminator/Unicode escaping. Live native TCP
and generated fresh PostgreSQL services verify both docs sources and unchanged
MCP discovery. All 27 auth tests, 38 framework tests, strict all-feature/all-target
clippy for both crates, generated HTTP/MCP/SDK/restart/rename/delete checks and
751 packaged sources pass. Nondefault base paths, complete ORM/hooks/client
integration and broader custom options remain open.

### Configurable non-root authentication namespaces

`AuthHttp::with_base_path` and generated `BETTER_AUTH_BASE_PATH` configure a fixed
non-root namespace; a path already present in `BETTER_AUTH_URL` takes precedence.
Raw routes, both popup helpers, Google/reset callbacks, docs and the generated
default JWKS URL use that resolution. JWT issuer/audience use the canonical
origin, matching Better Auth's normalized options. Unsafe namespaces and MCP
collisions fail before service startup. The actual TS server helpers now use
resolved auth URLs, and its configured Google callback no longer concatenates
an ignored base path or omits a slash for relative base-path settings.

All 343 actual Express/TS/schema/provider comparisons pass across seven
HTTP/HTTPS/default/custom/trailing-slash/URL-override configurations. The full
application HTTP/MCP authentication/compliance test runs under three namespaces.
A fresh generated PostgreSQL service passes custom-path separate MCP, URL-path
override on persisted-session restart, disabled MCP, default self-JWKS without
an override, SDKs and rename/delete. All 28 native tests, 1,016 CLI tests, strict
clippy, 161 raw HTTP regressions, 40 existing callbacks, 12 Scalar comparisons
and 751 packaged sources pass. Root/dynamic/encoded namespaces, full ORM/hooks/
client integration and broader custom options remain open.
