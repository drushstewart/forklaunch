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

The checked source inventory is [rust-typescript-public-api.json](rust-typescript-public-api.json).
It records 608 resolved declaration entry points and 1,662 declaration/re-export
records across the 44 packages, with source locations, public service/member
signatures, type aliases, explicit external re-export boundaries, private module
sources and CLI bins. Counts include repeated declarations exposed by different
packages; they are not counts of independent features or completed parity items.
`scripts/rust_api_inventory.mjs` follows manifest type exports (including wildcard
subpaths), build output/root directories and local source/re-export graphs. It
checks the package table against tracked manifests. Regenerate with `TSC_BIN`
pointing to TypeScript 5.9.3's `tsc`, then run
`node scripts/test_rust_api_inventory.mjs`; CI checks for drift without executing
application code or installing application dependencies.

This is a syntactic source inventory, not a type checker or proof of equivalent
Rust behavior. Imported aliases, external exports, computed schema/DTO shapes,
class inheritance, compiler-generated declarations and dynamic runtime behavior
still need semantic review. Private application source lists and binary paths are
capability leads, not public npm exports. Detailed Rust symbol/fixture mappings
remain open under checklist item 849; this inventory does not close that item.

| Source package | Rust implementation | Remaining scope / evidence |
| --- | --- | --- |
| [`@forklaunch/bunrun`](../framework/bunrun/package.json) | CLI Cargo/watch and mixed orchestration | Native builds and mixed Node/Bun Compose orchestration pass live HTTP/MCP, source reload and shutdown fixtures; automatic contract/SDK refresh remains. |
| [`@forklaunch/common`](../framework/common/package.json) | forklaunch-common | Fifteen public naming/hash/slash exports now have direct Rust equivalents and 10,455 actual-source comparisons; operation naming uses the TypeScript separator, cleanup, prefix and fallback semantics. Path normalization, other utilities and JavaScript-only value/type boundaries remain under symbol audit. |
| [`@forklaunch/core`](../framework/core/package.json) | forklaunch-core, forklaunch-core-macros, forklaunch-http, forklaunch-auth, forklaunch-contract, forklaunch-mcp | HTTP/contracts/config/DI/cache/object store/telemetry/encryption, explicit compliance metadata derives and an optional MCP HTTP server exist; Bounded tenant-scoped retention hooks exist; generic SQL entity/compliance/retention adapters and full auth/streaming conformance remain. |
| [`@forklaunch/e2e-tests`](../framework/e2e-tests/package.json) (generated/private) | Rust wire/runtime suites | Actual TypeScript framework service conformance remains beyond the plain Node wire fixtures. |
| [`@forklaunch/express`](../framework/express/package.json) | forklaunch-axum | Equivalent HTTP behavior is the target; multipart/SSE and full validation/auth/error fixtures remain. |
| [`@forklaunch/hyper-express`](../framework/hyper-express/package.json) | forklaunch-axum | All 27 exported HTTP methods dispatch through shared validation/auth and nested contracts; both SDK artifact sources and actual compiled TypeScript router token forwarding are covered. Runtime-specific adapter internals need no clone. |
| [`@forklaunch/infrastructure-s3`](../framework/infrastructure/S3/package.json) | forklaunch-infrastructure-s3 | Async store, bounded streaming/multipart transfers, codecs, signed downloads and browser POST policies are implemented with real MinIO fixtures. POST size/type/key/expiry enforcement, AWS signature vectors, SDK credential cache/rotation and actual TypeScript policy fixtures are covered; ordinary SigV4 bucket endpoints only. |
| [`@forklaunch/infrastructure-redis`](../framework/infrastructure/redis/package.json) | forklaunch-infrastructure-redis | Redis cache/queue/pubsub, typed EXISTS/delete batches, literal-prefix/glob scans and native client/config access have real fixtures. Opt-in safe-value codecs now use actual TS safeParse/safeStringify source for JSON-compatible string interoperability; Opt-in bulk queue methods also match falsy filtering and zero-count peeks. JavaScript-special values, number boundaries and remaining provider options remain. |
| [`@forklaunch/internal`](../framework/internal/package.json) | contract macros, CLI embedded templates and Rust tooling | Map remaining exported build/runtime helpers; JavaScript compiler utilities remain in the TypeScript path. |
| [`@forklaunch/testing`](../framework/testing/package.json) | forklaunch-testing and runtime fixtures | Test environment configuration, nine database aliases, Redis/Kafka/S3 endpoints and mock tokens match 970 actual TypeScript cases; child-process environment isolation passes. Container ownership/lifecycle, ORM setup/database clearing and the complete blueprint harness remain. |
| [`@forklaunch/universal-sdk`](../framework/universal-sdk/package.json) | forklaunch-sdk-rust, forklaunch-sdk-typescript, forklaunch-sdk-codegen and CLI SDKs | HTTP/multipart uploads, bounded CLI SSE responses and guarded recursive CLI models and omission/null presence exist through both artifact formats, with actual TypeScript conformance and live clients; native Rust and TypeScript typed HTTP/SSE responses and recursive models share the CLI compiler and pass strict consumers and live clients; one shared HTTP SDK now imports native contracts and TypeScript OpenAPI, with both live client languages, lifecycle preservation and custom-path watch recovery. Remaining schema conformance, multipart response scope and broader realtime profiles remain. |
| [`@forklaunch/validator`](../framework/validator/package.json) | forklaunch-validation and schema contracts | JSON Schema validation/coercion exists; audit Zod/TypeBox exported behaviors against shared fixtures. |
| [`@forklaunch/ws`](../framework/ws/package.json) | forklaunch-ws, forklaunch-asyncapi | Typed native server/client transports now cover authenticated upgrades, serde/garde payloads, bounded I/O, validated ping/pong/close and bidirectional actual TypeScript interoperability. Optional Axum upgrades share HTTP/MCP listeners, preserve negotiated subprotocols and use the same typed engine. Raw socket metadata/checked AsyncAPI exports match actual TypeScript channel/operation behavior in 34 conformance documents, with standalone recursive and reachable legacy annotation schema relocation, once-decoded local pointers and official schema validation. References into literal assertion/default/example data now use separately bundled schema copies, preserving the original values and recursive targets under copy/output bounds. Optional compiled raw JSON Schema codecs validate original payloads before serde, with bounded compilation and disabled external retrieval; 4,104 actual-helper/AJV and 168 Zod cases pass. Generated native Rust clients share the typed transport, with actual TS-server and invalid-peer fixtures plus recursive/empty consumer compilation. Generated Node TypeScript clients pass strict ESM/CommonJS consumers, 4,104 codec comparisons, actual TS/native peers, bounded queues and cancellation/close cleanup. CLI `--from socket-contract` now generates multi-socket Rust/Node packages and checked AsyncAPI, with service selection, check/watch, custom artifact/output paths and validation before writes. Relocated live clients, parser conformance and HTTP/MCP startup pass. Rust service registries now export portable socket artifacts automatically, with custom contained paths and content-owned move/removal cleanup preserving edited/manual artifacts. Owned socket SDK module/package pruning, empty-registry watch recovery and service rename/delete now pass with preserved user files/compiler options and updated Cargo membership. External AsyncAPI 3.0.0 JSON WebSocket server imports now support native/CLI generation/check/watch, local schema extraction, explicit directions, checked disjoint messages, input ownership and SDK rename/pruning. Relocated strict consumers/live peers and HTTP/MCP startup pass. Broader AsyncAPI event/protocol profiles and binary payload metadata remain. |
| [`@forklaunch/blueprint-billing-base`](../blueprint/billing-base/package.json) (generated/private) | forklaunch-billing-axum + Rust CLI | Generated base module: 33 HTTP/MCP routes, both live SDKs, five persistent service families, compliance authorization, tenant isolation, encryption/audits, restart and CLI lifecycle pass. Fresh archive installation passes. Sixty-five actual configured mapper cases now audit schema resolution, HTTP stripping, local IDs, ownership, read normalization and missing-row failures. Mixed deployment remains. |
| [`@forklaunch/blueprint-billing-stripe`](../blueprint/billing-stripe/package.json) (generated/private) | forklaunch-billing-axum + Rust CLI | Generated Stripe module: 35 HTTP/MCP routes, authenticated provider services, both live SDKs, restart and CLI lifecycle pass, including fresh archive installation. Stripe raw callbacks and signed platform payment events now synchronize business state with durable tenant receipts. Managed outbound Stripe transport passes 23 actual TS gateway comparisons; generated keyless HTTP/MCP startup, signed provider calls, ignored local keys, incomplete configuration rejection and lifecycle pass. Provider reconciliation and mixed deployment remain. |
| [`@forklaunch/blueprint-cac-base`](../blueprint/cac-base/package.json) (generated/private) | forklaunch-cac-interface/base/axum and Rust init module cac-base | Generated module mounts HMAC lookup and JWKS/system-role compliance; no entities are registered, so authorized export/erase return upstream 404s. Uses verified claims or a trusted role resolver in place of the TS admin-only stub. Mixed coexistence, both SDKs, upgrade, rename/sync/delete and installed source archives pass. |
| [`@forklaunch/blueprint-client-sdk`](../blueprint/client-sdk/package.json) (generated/private) | forklaunch-blueprint-client-sdk and CLI SDK generation | Native SDK prototypes and generated HTTP packages exist; all added modules/mixed surfaces remain. |
| [`@forklaunch/blueprint-core`](../blueprint/core/package.json) (generated/private) | forklaunch-blueprint-core and framework crates | Core DTOs/contracts exist; compliance/service registrations and shared role/provider behavior remain. |
| [`@forklaunch/blueprint-ecommerce-stripe`](../blueprint/ecommerce-stripe/package.json) (generated/private) | forklaunch-ecommerce-axum; Rust CLI ecommerce-stripe module | Core 27 catalog/cart/order/payment routes plus health start over HTTP/MCP with PostgreSQL, HMAC, provider selection and durable inventory events. Live generated HTTP/MCP, both SDKs, separate inventory worker, restart persistence and init/sync/rename/delete checks pass. Tax/shipping calculators pass 32 actual TS comparisons; bulk catalog import passes actual TS and generated PostgreSQL HTTP/MCP/SDK fixtures. Checkout HTTP/MCP/SDK wiring passes eight actual TS controller comparisons, cross-process payment races and ten parallel-cart checks. Verified Stripe/PayPal callbacks pass actual TS business comparisons and cross-process capture/replay/inventory checks; 31 application routes plus health start over HTTP/MCP. Generated Compose starts the separate inventory worker; Linux HTTP/MCP, paid/cancelled delivery and graceful shutdown pass. Persistence compliance and interrupted capture reconciliation remain. |
| [`@forklaunch/blueprint-iam-base`](../blueprint/iam-base/package.json) (generated/private) | Rust init module iam-base | Generated sessions plus thirty configured directory CRUD/grant routes, JWKS/HMAC/RBAC, transactional organization scope and HTTP/MCP startup are covered. Configured discovery/compliance also match the actual all-`none` policy; application-specific metadata and provider lifecycle remain. |
| [`@forklaunch/blueprint-iam-better-auth`](../blueprint/iam-better-auth/package.json) (generated/private) | forklaunch-iam-better-auth and native CLI preset | All 70 configured raw routes, four server-only helpers, seven application HTTP/MCP contracts, surfacing/compliance/retention, real encrypted ORM conformance, popup helpers, Scalar sources and default/custom/root namespaces pass actual comparisons. Generated restricted PostgreSQL, SDKs, shared/separate/disabled MCP, self-JWKS, restart and rename/delete pass. Broader custom options/hooks/client plugins, typed JWT/JWKS adapters and indexed storage remain. |
| [`@forklaunch/implementation-billing-base`](../blueprint/implementations/billing/base/package.json) | forklaunch-billing-base | Five asynchronous base services implement plan/subscription CRUD and checkout/payment-link/portal cache lifecycles, with typed DTOs, optional backups and tenant-RLS PostgreSQL persistence. 168 actual TS base-class operations and native concurrency/expiry/encryption/audit/rotation fixtures pass. Generated billing modules and HTTP/MCP startup now pass; 65 actual configured mapper cases now pass; Redis cache interop remains. |
| [`@forklaunch/implementation-billing-stripe`](../blueprint/implementations/billing/stripe/package.json) | forklaunch-billing-stripe | Version-pinned authenticated Stripe client and five persistent async provider services pass 21 official SDK transport cases and 144 actual TS wrapper operations with backup/cache lifecycle. Raw-body authentication is shared with ecommerce and rejects stale callbacks. Legacy synchronous adapter fails closed. Generated modules/contracts and HTTP/MCP startup now pass. Twenty-six actual TS webhook state/provider-effect comparisons pass, with paginated product updates and subscription reassignment protection. Native tenant-RLS webhook receipts and replica-safe platform nonces pass PostgreSQL gates. Managed outbound transport passes 23 actual TS gateway comparisons and native JSON capability signature checks. Configured mapper audit now covers 65 actual cases; reconciliation remains. |
| [`@forklaunch/implementation-cac-base`](../blueprint/implementations/cac/base/package.json) | forklaunch-cac-base | Mock codes and provider descriptor/lookup match actual TS fixtures; authenticated generated module, both SDKs and CLI lifecycle now pass. |
| [`@forklaunch/implementation-ecommerce-base`](../blueprint/implementations/ecommerce/base/package.json) | forklaunch-ecommerce-base | Six base services, PostgreSQL persistence, restricted-role tenant RLS and encrypted/audited order addresses. Actual TypeScript 208-operation base and 35-case catalog-controller conformance pass, alongside real PostgreSQL concurrency/security and bounded catalog reads. Generated module integration remains separate. |
| [`@forklaunch/implementation-ecommerce-paypal`](../blueprint/implementations/ecommerce/paypal/package.json) | forklaunch-ecommerce-paypal | Bounded OAuth/Orders/signature-verification client and shared-store payment wrapper. Actual TS conformance covers 22 client cases and 19 payment operations; live fixtures cover concurrent capture and rollback. Generated webhook/controller wiring passes live HTTP/MCP, provider verification and cross-process capture tests; interrupted-capture reconciliation remains. |
| [`@forklaunch/implementation-ecommerce-stripe`](../blueprint/implementations/ecommerce/stripe/package.json) | forklaunch-ecommerce-stripe | PaymentIntent creation/retrieval, resume, direct Connect fees and shared-store confirmation/failure. Fifteen actual TS scenarios and bounded live HTTP fixtures pass; secrets stay out of persistence. Generated raw-body webhook/controller wiring passes official SDK verification, actual TS controller comparisons and live HTTP/MCP/SDK checks. |
| [`@forklaunch/implementation-iam-base`](../blueprint/implementations/iam/base/package.json) | forklaunch-iam-base, forklaunch-iam-axum | Typed single/batch directory CRUD, configured mapper behavior, nested relationships, encrypted PostgreSQL/RLS, atomic audit/rollback, retained-key uniqueness and session persistence exist. Actual TS service/route/schema conformance covers 44 operations, all 33 configured routes and 22 schema layouts, plus actual configured compliance export/erase behavior. Provider/session integration and generic application compliance extensions remain. |
| [`@forklaunch/implementation-messaging-base`](../blueprint/implementations/messaging/base/package.json) | forklaunch-messaging-base | Hooks-only sent behavior, encrypted tenant Postgres persistence, audited reads, subject export/erasure and key rotation; actual TS service fixtures pass. |
| [`@forklaunch/implementation-messaging-twilio`](../blueprint/implementations/messaging/twilio/package.json) | forklaunch-messaging-twilio | Actual TS REST/status/callback/error conformance; bounded single-attempt dispatch, signed form callbacks and generated SDK/runtime fixtures. |
| [`@forklaunch/implementation-worker-bullmq`](../blueprint/implementations/worker/bullmq/package.json) | forklaunch-worker-redis optional bullmq feature | Official wire protocol passes actual TS/native producers/consumers, retry/backoff, default retention, waiting/active peeks and restricted ACL fail-stop. Generated lifecycle, per-attempt failure observers (including removed/draining jobs), HTTP controls and count-based retention pass. Lazy age retention, bidirectional abandoned-job recovery, stalled/start limits and retained/removed preprocessor failure callbacks now pass. A feature-gated outcome-data hook in the pinned vendored protocol preserves original events through deletion, without changing Redis scripts or existing failure notifications. Job defaults/metadata, size-limit rejection, bounded log retention, delayed/priority/LIFO ordering and concurrency also pass actual TS/native checks. Concurrent rate limiting and bounded age-cleanup limits also pass actual TS/native fixtures. Simple/TTL/delayed-replacement deduplication now passes six actual TS/native producer and consumer scenarios, including cross-producer keys, completion release and TTL expiry. Sixteen worker scenarios preserve failure reasons and bounded histories for recovered/terminal jobs under immediate/delayed retries. Four additional actual TS/native consumer scenarios cover active-job keep-last deduplication, cross-producer submission, latest-payload follow-up and release after either completion or terminal failure. Twelve actual-library scheduler scenarios now cover bidirectional interval/cron schedules, three-iteration limits, success/failure, inspection/removal, scheduling before processing, active cancellation, preprocessor rejection and zero disabling start limits. Scheduler templates and every iteration also retain log/history/size options, with real bounded-log and failure-history checks. Iteration creation timestamps and cron scheduled-time metadata also match actual TS workers. Scheduler date/other metadata, broader cron/update profiles and advanced-option conformance remain open. |
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

### Root-mounted authentication and reserved namespaces

`BETTER_AUTH_BASE_PATH=/` (or empty) mounts raw authentication at root while
keeping application contracts, docs, SDKs and MCP reachable. The framework's
`merge_internal` API reserves declared fixed namespaces; collision validation
and parent nesting apply to each reservation. The actual TS preset now limits
its root catch-all to known auth namespaces, avoiding interception of application
routes. Root reset errors no longer introduce an extra slash.

All 450 actual Express/TS comparisons pass across nine configurations, including
630 raw-route dispatch probes and root signed sessions/Google/reset/popup flows.
The full native application HTTP/MCP test runs under five configurations. A fresh
generated PostgreSQL service passes root HTTP/MCP and default self-JWKS without
an override, as well as custom/separate and URL-override/disabled listeners,
persisted sessions, SDKs and rename/delete. All 28 native/40 framework tests,
strict all-feature/all-target clippy for both crates and 751 packaged sources
pass. Dynamic/encoded namespaces and broader ORM/hooks/client/options work
remain separate gates.

### Archive-installed root/docs snapshot

Fresh Cargo archive installation `EklWnG` validates committed snapshot `f20252656`.
All portable services and four worker backends compile, both SDK targets pass,
and all 28 packaged auth tests pass. The complete generated HTTP/MCP matrix
covers enabled/disabled/custom endpoints, shared/separate listeners,
explicit/ephemeral ports, TypeScript's +2000 fallback, atomic startup rejection
and graceful shutdown. All five actual-TypeScript-signed native capability
callbacks pass forgery/staleness/replay/redelivery checks. The checkout CLI was
rebuilt after the archive run. This confirms source portability and service
startup; broader parity and published registry installation remain open.

### Actual shared and generated frontend client interoperability

The shared `clientIamBetterAuthSdkClient` and generated `iamSdkClient` factories
now accept `authBasePath` (default `/api/auth`, `/` for root); a path already in
`host` takes precedence. Both initial templates and subsequent AST injection
carry the option. Eight live scenarios execute both actual TS factories and
Better Auth 1.7.6 over native PostgreSQL HTTP: default/custom/root/URL-override
namespaces, 80 requests total. Signup additional fields, sessions, profile
updates, independently verified JWT/JWKS, signout and signin pass. The fixture
owns Node cookie persistence and public-origin forwarding; universal SDK core
construction remains a separate boundary and does not establish unified mixed
SDK parity.

The final run also exercises automatic pinned-client dependency setup, actual
CLI TypeScript client generation, restricted PostgreSQL, shared/separate/disabled
MCP, self-JWKS, SDKs, persistence, shutdown and rename/delete. All 1,016 CLI tests
and 751 packaged source checks pass. E2E CI installs the pinned client and reacts
to changes in both the auth module and frontend factory. Broader custom client
plugins/hooks, unified mixed SDK behavior and indexed storage remain open.


### BullMQ deduplication and retry diagnostics (2026-10-12)

The actual `bullmq@6.3.10` wrapper comparison passes six simple/throttle/debounce
scenarios with both producer directions, including bulk submission, shared
cross-producer keys, delayed replacement, completion release and expiry. Sixteen
worker scenarios compare both implementations over immediate/delayed retries,
unlimited/zero/one/two-entry histories, recovered jobs and terminal failures.
Native failure reasons and histories now travel in the existing lock-checked
transition scripts, preserving removal and stale-lock semantics without Lua
changes. Rust diagnostics supply native history strings. The full existing
retry/retention/stall/start-limit/drain/ACL regression remains green.

All 66 pinned protocol tests pass against an isolated Redis container; all five
adapter unit tests pass (two real-server tests are ignored in that unit command).
Strict all-feature/all-target adapter Clippy and the 751-file source packaging
check pass. Logs: `/private/tmp/fl-rust-bullmq-history{,-unit,-adapter,-clippy,-package}.log`.
Native TypeScript typed SDK support was already complete; the current package
inventory now reflects that existing implementation rather than the older gap.


Active-job keep-last coverage also passes the full existing BullMQ wire
regression (`/private/tmp/fl-rust-bullmq-active-dedup.log`). Fresh generated
workers from CLI commit `182be8817` pass default-handler and explicit-handler
failure histories, health/readiness/authenticated controls, retry/drain/restart
and relocated sync/change/rename/delete. The companion service passes the full
shared/separate/disabled/custom HTTP/MCP listener, startup-rejection and shutdown
matrix (`/private/tmp/fl-rust-bullmq-history-generated.log`, owned Redis,
`run.0121Ws`). The CLI builds, and the protocol compiles with compatibility
features disabled.


### BullMQ scheduler timing and zero-limit parity (2026-10-12)

The separate scheduler fixture passes twelve actual `bullmq@6.3.10` scenarios:
interval and timezone-aware cron schedules in both producer/consumer directions,
three-iteration limits, successful/failed jobs, native inspection/removal, queued
cancellation during active work and sequence continuity after preprocessor
rejection. Eight lifecycle scenarios also assert zero disabling start limits.
The fixture first reproduced missing queued work during active native processing
and rejected native jobs under a zero start limit; both mismatches are fixed in
the optional compatibility feature. Existing Lua scripts are unchanged.

All 66 isolated protocol tests, strict all-feature/all-target adapter Clippy and
all 752 embedded source/template assets pass. CI now runs the scheduler fixture.
Logs: `/private/tmp/fl-rust-bullmq-scheduler-{final,clippy,unit,package}.log`;
negative reproductions: `/private/tmp/fl-rust-bullmq-scheduler-timing-before.log`
and `/private/tmp/fl-rust-bullmq-zero-start-before.log`.


### BullMQ scheduler option retention (2026-10-12)

The scheduler fixture reproduced missing native `keepLogs` metadata after
iteration. Ordinary job maps, persisted scheduler templates, first iterations
and subsequent iterations now share the diagnostic/size option encoder.
All twelve scheduler scenarios pass; every job in the eight bidirectional
interval/cron lifecycle scenarios retains `keepLogs: 2`, `stackTraceLimit: 1`
and `sizeLimit: 2048`. Actual workers retain the newest two of three log rows and
one failure diagnostic on failed jobs. This proves metadata and retention;
broader scheduler timestamp/date/options behavior remains separate.

All 66 isolated protocol tests, strict all-feature/all-target adapter Clippy,
all 752 source/template package assets and the CLI build pass. Logs:
`/private/tmp/fl-rust-bullmq-scheduler-metadata{,-clippy,-unit,-package,-cli}.log`;
negative reproduction: `/private/tmp/fl-rust-bullmq-scheduler-metadata-before.log`.


### BullMQ scheduler time metadata (2026-10-12)

The scheduler comparison reproduced missing iteration creation timestamps.
All twelve scheduler scenarios now pass with timestamp assertions on every
job in the eight bidirectional interval/cron lifecycle cases, alongside bounded
logs/histories and option retention. Cron `prevMillis` matches the scheduled
instant in each qualified job ID; interval jobs omit it as the actual Node
implementation does. All 66 isolated protocol tests, strict all-feature/all-target
adapter Clippy and the CLI rebuild pass. Logs:
`/private/tmp/fl-rust-bullmq-scheduler-time-{metadata,clippy,unit,cli}.log`;
negative reproduction: `/private/tmp/fl-rust-bullmq-scheduler-time-metadata-before.log`.

The preceding fresh generated CLI run also passes worker histories, controls,
retry/drain/restart and relocated lifecycle, plus the companion service's full
HTTP/MCP matrix (`/private/tmp/fl-rust-bullmq-scheduler-generated.log`, owned
Redis, `run.Y0u1m9`). The subsequent time-field-only scheduler change is covered
by actual bidirectional broker clients and the rebuilt checkout CLI.


Unify mixed-application HTTP SDK generation. Native contracts and TypeScript
OpenAPI now produce one TypeScript package and per-service Rust crates from either
HTTP artifact mode. Persist imported TypeScript names so missing exports cannot
silently erase clients during native lifecycle refresh. Watch the manifest and
actual configured artifacts for both languages, preserving output on errors and
recovering when inputs return. All 1,023 CLI tests and 752 packaged assets pass.
The new E2E fixture exports a generated native service and a fixture-owned real tsx
HTTP server, relocates the application, verifies custom-path watch loss/recovery,
and compiles/calls both generated SDK languages with escaped parameters, typed
success/error responses, JSON requests and request hooks. Generated Rust SDK Clippy
and native HTTP/MCP listener/startup/shutdown checks pass. This establishes the
shared HTTP client surface; mixed TypeScript scaffolding and dev/lifecycle
orchestration remain separate work.


Correct mixed project lifecycle dispatch and manifest preservation. Real CLI
regression reproduced TypeScript service registration loss during native sync;
service/worker/library sync/delete now dispatch by project metadata, and native
sync refuses non-native cleanup. Router deletion follows the owning service's
language and supports the generated async startup function. All root language,
SDK, workspace and artifact fields survive TypeScript project-view roundtrips,
as does the configured application name. A generated TypeScript-first mixed
fixture passes both languages' sync/delete, TypeScript rename roundtrips and
router removal with byte-preserving rejection of unsupported server edits. The
cleanup hook is fixture-owned to avoid modifying shared package-manager caches.
All 1,025 CLI tests and native sync/delete regressions pass. Automatic mixed
scaffolding, application-wide orchestration and deployment remain separate gates.


Support native backend attachment to existing TypeScript applications. Rust
service/worker/library/module init creates a root Cargo workspace and mixed
metadata for Node/Bun apps, preserves existing TypeScript files and Cargo
configuration, and supplies a separate native Dockerfile/ignore file. Dry-run,
configuration rejection, metadata aliases, output containment and customized
native-file preservation are tested. Actual Node/Bun scaffolds accept and compile
relocated native service/Redis-worker/library/CAC projects without TypeScript
source changes, and both native services pass the HTTP/MCP listener/startup/
shutdown matrix. The separate mixed native image builds all binaries and passes
HTTP health/OpenAPI, MCP initialization/discovery/tool invocation and SIGTERM.
All 1,030 CLI tests, 752 packaged assets and the mixed lifecycle regression pass.
CI and the migration guide include this workflow. Rust-first TypeScript setup,
application-wide mixed sync/dev orchestration and deployment remain open.


Application-wide mixed sync checkpoint (2026-10-12): `sync all --confirm` now
runs native discovery and TypeScript discovery together for Node/Bun applications,
retains native registrations during TypeScript orphan cleanup, restores registered
custom Cargo paths even without a crates directory, and refreshes both language
exports before generating the configured SDK targets. Native-only applications
with `sdkTargets = ["rust"]` do not acquire a TypeScript SDK workspace. Required
prompt values use checked argument lookup instead of panicking on synthetic
matches; confirmation accepts detected descriptions/types. Per-project sync
errors propagate rather than reporting successful completion. Removing a native
library at a custom path removes only dependencies matching its registration,
preserving unrelated dependencies and user path overrides. The generated mixed
fixture passes TypeScript discovery, custom-path restoration/removal, both SDK
checks, workspace compilation and the full HTTP/MCP listener/startup/shutdown
matrix; the Rust-only target fixture passes. All 1,032 CLI tests pass. CI includes
`rust_mixed_sync.sh`. Rust-first TypeScript scaffolding, development orchestration
and the wider deployment/runtime parity gaps remain open.


Configured OpenAPI export checkpoint (2026-10-12): native and TypeScript exporters
now refresh each registered `openapiPath` alongside the requested export-directory
layout, keeping SDK watch/import and release consumers consistent. All selected
services are checked together before export for cross-service destination
collisions; configured external/traversing/dangling-symlink paths reject.
Registered copies publish atomically, and aliases of the same exported file
preserve its contents. Three unit tests cover refresh, collision/path rejection
and symlink behavior. The generated mixed fixture now uses custom paths for both
languages, checks both SDK formats, replaces stale registered files through an
explicit `--output`, then passes workspace compilation, HTTP/MCP startup and
lifecycle cleanup. All 1,035 CLI tests and the release orchestration regression
pass. This closes artifact-path export consistency; broader parity gaps remain.


Rust-first TypeScript attachment checkpoint (2026-10-12): the previous generated
Rust app rejected TypeScript service initialization with `Invalid TestFramework:
cargo-test`. Initialization now stages the standard Node/Express/Zod workspace
and requested TypeScript project before publishing changed files. The app switches
to mixed metadata and Node tooling while retaining native Cargo paths, sources,
Dockerfiles, Compose entries, SDK targets, package scripts and custom dependency
versions. Root PNPM membership includes modules and existing SDKs; default build
runs Cargo plus all JavaScript packages, and other JS tool commands run where
their configuration lives. Atomic file replacement, internal symlink rebasing,
external-link rejection and changed-file conflict detection protect application
files. Dry-run and rejected project generation leave the app byte-for-byte intact.
The modules directory and foundational project names must be available.

The actual CLI fixture checks service/worker/library/CAC-module first attachment,
Rust-only SDK selection, subsequent native initialization, and reserved-name
rejection. It uses an empty named account: local TypeScript application generation
no longer performs the unused token lookup/refresh that previously gated rendering.
Real dependency installation and the combined Cargo/TypeScript workspace build
pass. The unmodified generated TypeScript service answers HTTP and MCP
initialization/discovery/tool invocation; the native service passes the full
HTTP/MCP configuration/startup/shutdown matrix. All 1,038 CLI tests and the existing
mixed rename/sync/delete/router regression pass. CI includes
`rust_typescript_attach.sh`. This closes default Rust-first workspace scaffolding;
unified development orchestration, mixed compliance aggregation and platform
deployment validation remain separate work.


Mixed compliance aggregation checkpoint (2026-10-12): generated compliance clients
now accept typed native adapters alongside the existing TypeScript SDK clients,
retain each success/business-error response type, and include every registered
database service/worker. Services initialized before IAM require explicit
compliance implementations; their ordinary client does not satisfy the generated
signature. Missing adapter methods fail at construction. TypeScript Better Auth
keeps its core wrapper; native adapters do not. IAM variants are recognized even
under custom native service names. Better Auth initialization now reuses existing
SDK imports and declares its client-sdk runtime dependency, preserving overrides.
Package dependency parsing accepts JSON fields in any order and reports missing
names instead of panicking.

The actual CLI fixture creates fresh mixed apps for iam-base and iam-better-auth,
installs dependencies, builds every TypeScript package, checks strict consumer
types and runs the real legacy/generated SDKs against owned HTTP responders.
Bearer propagation, export/erase results, native business errors, explicit legacy
adapters and missing-method failures pass for both variants. These checks cover
client transport and typing; persistence remains covered separately by module
suites. All 1,044 CLI tests pass. CI includes rust_mixed_compliance.sh. Unified
development orchestration, root workspace integration for TypeScript-first apps,
and deployment validation remain open.


TypeScript-first root workspace checkpoint (2026-10-12): reproducing HTTP SDK
generation on an actual Node-first mixed application showed a root PNPM workspace
containing only the SDK, omitting all four TypeScript module packages. SDK
workspace generation now merges module membership and tooling at the application
root, including configured modules paths, nested exclusions and missing workspace
policies. It parses YAML structurally, supports inline package lists, preserves
custom root scripts/dependency versions/policies, and adds Bun workspaces and
Bun SDK commands. A missing root build runs Cargo when present and then the
JavaScript packages. Redundant direct-child entries are omitted because Bun
otherwise executes their builds twice. Module source and nested configuration
remain unchanged; invalid workspace configuration rejects before either root
configuration file is rewritten.

All 1,047 CLI tests pass. Fresh Node and Bun mixed apps install dependencies and
build Cargo plus every module and the HTTP SDK from their application root. The
Bun fixture asserts exactly one successful build per module. Both applications'
TypeScript services answer HTTP and MCP initialization/discovery/tool invocation;
the native services pass the complete HTTP/MCP startup/configuration/shutdown
matrix. The Rust/mixed release orchestration regression also passes. CI includes
rust_sdk_workspace.sh. Unified development orchestration, socket-only workspace
integration and platform deployment remain separate work.


Socket-only workspace checkpoint (2026-10-12): socket SDK generation now uses
shared pure workspace planning, so TypeScript-first Node/Bun apps include their
modules and socket packages in root installation/builds. Separate sdk:socket:*
commands coexist with HTTP SDK commands and preserve overrides. Socket check mode
includes root package/workspace drift without writing them. Existing PNPM YAML
comments/layout are retained when membership suffices; inserted entries retain
sequence order/indentation and handle missing trailing newlines. Both protocol
workspace plans coexist, and invalid planning remains non-mutating.

All 1,049 CLI tests pass. The actual socket-only Node/Bun fixture invokes the
bundled compiler, generates/checks SDKs and AsyncAPI, rejects stale workspace
membership without rewriting configuration, installs real dependencies, and
builds every module/socket SDK plus Cargo from the root. Bun builds each module
exactly once. Both TypeScript services pass live HTTP/MCP calls, and native
services pass the complete HTTP/MCP startup/configuration/shutdown matrix. CI
includes rust_socket_workspace.sh. Carrying generated nested Bun install policy
to the new root remains a follow-up; unified development orchestration and
platform deployment remain open.


Bun root install-policy checkpoint (2026-10-12): HTTP and socket workspace planning
now carries generated minimumReleaseAge and exact minimumReleaseAgeExcludes from
the nested Bun configuration into root bunfig.toml. Root overrides, comments and
unrelated settings remain intact. Only these portable generated settings move;
module-relative Bun options remain nested rather than changing their path base.
Planning stays read-only and rejects malformed/outside-root destinations before
publishing root configuration. Socket check mode includes Bun policy drift.

All 1,051 CLI tests pass. Both previously validated Bun applications regenerate
with the real CLI, inherit the generated settings, preserve a custom root age and
comment, install real dependencies, and run their generated HTTP/socket SDK
build commands. Missing socket policy rejects in check mode without rewriting the
file and regenerates successfully. The shared CI workspace fixture now asserts
policy inheritance and override preservation for both protocols. Local Bun 1.2.19
confirms configuration/install/build compatibility; these checks do not establish
release-age enforcement behavior across Bun versions. Mixed development and
platform deployment remain open.


Mixed development checkpoint (2026-10-12): `forklaunch dev` resolves Compose and
starts native services/workers with debug compilation, source mounts, project-scoped
Cargo caches and restart watches for registered crates/libraries/frameworks/SDKs.
TypeScript services retain their existing development commands. Selection and
dependency closure preserve infrastructure containers and inactive native projects.
Private temporary configuration and signal supervision preserve application files
and stop Compose before CLI exit. Generated root workspaces expose the command
through their dev script while preserving custom scripts. Native watch requires
Compose 2.32; custom native entrypoints require explicit development configuration.

Actual Rust-first Node and TypeScript-first Bun applications both pass HTTP/MCP
startup, initialization/discovery/tool calls, Rust and TypeScript source reloads,
HTTP/MCP after edits and SIGTERM shutdown. The fixture caught and fixed root
Compose bind paths incorrectly interpreted as named volumes. All 1,057 CLI tests
pass; CI runs both runtime variants through rust_mixed_dev.sh. Production Compose
and Dockerfiles remain unchanged. Automatic source-to-contract/SDK refresh,
Windows runtime validation and platform deployment remain separate work.


Mixed release Dockerfile checkpoint (2026-10-12): release metadata now selects
the native root Dockerfile.rust for Node/Bun applications instead of assigning the
TypeScript Dockerfile to every service. Native worker service/consumer entries use
the same rule. Project-local Dockerfiles retain project-local contexts; configured
TypeScript Dockerfile names remain intact. All 1,058 CLI tests pass. The actual
release command also passes ten owned-platform/tool scenarios, now including Node
and Bun mixed build/export order and emitted buildContext/dockerfile pairs. The
fixture uses a unique synthetic named account and removes only that account on
exit; it makes no publication calls. Live platform/ECS launch-command and runtime
compatibility remain unverified; corrected metadata alone does not close that gate.


Captured storefront lifecycle checkpoint (2026-10-12): static captures now retain
a contained metadata.staticPath and use dedicated library lifecycle handling.
Rename/description changes preserve asset bytes and manifest comments, update
topology and publish the manifest atomically with filesystem rollback. Deletion
stages assets before publishing metadata and retains recoverable data if rollback
fails. Path checks reject external locations, symlinked capture directories,
application metadata and directories containing other registered projects.

Nested storefront initialization no longer attempts TypeScript attachment. A later
Rust-to-TypeScript workspace attachment preserves the capture's original path,
including legacy captures without path metadata. Sync validates captures without
creating package/Cargo artifacts; a captured Cargo.toml is never classified as a
native crate. Prebuilt release validation honors static paths and containment.
All 1,061 CLI tests and the actual Rust/Node/Bun lifecycle fixture pass, including
read-only previews, binary asset preservation, old-capture mixed migration, sync,
rename and deletion. CI includes rust_storefront_lifecycle.sh. Static captures
remain outside API/SDK generation; frontend hosting and native relay are separate
tasks. Existing live mixed HTTP/MCP checks remain the service-runtime evidence.


Archive installation checkpoint (2026-10-12): a fresh 2,004-file CLI archive
installs with --locked outside the checkout, generates and relocates the full
service/worker/module fixture, compiles both SDK families, and passes actual
HTTP/MCP startup, listener settings, capability callbacks and graceful shutdown.
The archive explicitly contains the mixed dev command and HTTP/socket/Bun
workspace planners. Strict ESM/CommonJS socket consumers, recursive native/CLI
HTTP SDK consumers, all four worker backends and bundled auth/transport tests
pass. This run validates the mixed-dev/release checkpoint through 76103f22f; the
subsequent storefront lifecycle has its separate CLI/runtime evidence above.

The fixture now installs its own TypeScript 5.9.3 consumer compiler. Its previous
checkout compiler had advanced to 7.0.2 and rejected an inferred fixture rootDir;
the source layout is now explicit and the archive test uses a reproducible
compiler. This does not establish general TypeScript 7 support.


Indexed authentication reads checkpoint (2026-10-12): AuthTransaction now exposes
find_unique for declared unique string fields and get_many for bounded primary-key
sets. PostgreSQL reuses current/previous-key blind indexes, deduplicates key-rotation
hits and verifies each decrypted field before returning a match. Primary-key reads
preserve record insertion order, deduplicate IDs, skip missing rows and bound inputs
and results to 4,096. Memory and fixture adapters implement the same semantics.

Credential signup/signin, Google email lookup, verification, member/invitation
email resolution and organization slugs now avoid complete user/organization scans.
Full organization user joins use primary keys while retaining their prior 100-user
selection and ordering. Memory and restricted-role PostgreSQL tenants seeded with
4,100 users pass credential operations and live HTTP/MCP initialization, discovery,
tool invocation and shutdown. PostgreSQL also passes cross-tenant misses, encryption
rotation, update/delete index cleanup, tampered-index rejection and the existing
transaction/encryption/audit suite. All 29 native tests pass; the normally ignored
PostgreSQL suite passes separately. Seven actual TypeScript Better Auth suites pass
979 response/storage/protocol comparisons. Strict default and all-feature/all-target
clippy pass and the CLI rebuild embeds the changes. Source/template inventory remains
752 files. Custom AuthTransaction implementations must add both read methods.

This closes exact unique reads and bounded user joins, not full storage scalability:
other account/session/member scans and general indexed pagination remain open.

### SDK regeneration stability (2026-10-12)

- [x] HTTP, socket-contract and AsyncAPI watchers fingerprint artifact contents with
  streaming SHA-256 instead of filesystem timestamps. Identical re-exports do not
  trigger regeneration; equal-size edits with preserved timestamps are detected.
- [x] HTTP/WebSocket SDK outputs, TypeScript workspace files, HTTP source tracking
  and Cargo membership updates preserve unchanged files, preventing avoidable
  development rebuilds when SDK generation repeats.
- Validation: 1,063 CLI tests pass; the final streaming fingerprint implementation
  additionally passes all four watch tests. Regression tests cover repeated native
  and TypeScript generation, workspace timestamps, and timestamp collisions.
- Still open: automatic contract export and SDK refresh integrated with `forklaunch
  dev`, including custom artifact paths and mixed-runtime restart handling. Existing
  live HTTP/MCP development validation remains recorded above; this checkpoint
  changes generation/watch behavior and does not claim a new startup validation.

### Native export package selection (2026-10-12)

- [x] Native contract export now invokes Cargo using the registered crate's manifest,
  honoring custom crate paths, package names and Cargo `default-run` selection rather
  than assuming that the service registration name is also the package name.
- Validation: 1,064 CLI tests pass. The new regression compiles and executes a real
  dependency-free Cargo package in a path containing spaces, with a different package
  name, two binaries and a default binary; it verifies custom contract and OpenAPI
  outputs and the requested export copy. The CLI rebuild passes with both this
  change and the SDK regeneration stability changes.

### Native development contract/SDK loop (2026-10-12)

- [x] `forklaunch dev --export-contracts` runs the freshly built native binary in
  contract mode before startup and each Compose reload, honoring registered contract
  and OpenAPI paths. Artifacts are staged before publication; failed exports retain
  prior outputs, and identical outputs retain timestamps. Shell variables are escaped
  for Compose interpolation. Path containment/collisions and read-only planning are
  checked, including native auxiliary workers.
- [x] `--sdk-target typescript|rust|all` implies native exports and owns an HTTP SDK
  watcher for the lifetime of development. `--sdk-service` scopes generation. Shutdown
  reaps the owned watcher; newly generated Rust SDK directories are watched even when
  absent at planning time. Dry-run does not create those directories.
- [x] Standalone SDK watch waits for initial artifacts instead of exiting; `--once`
  retains strict failure behavior. Watch captures input content before regeneration
  so concurrent exports are observed on the next pass. Unknown service selections fail.
- [x] Real Node and Bun mixed Compose fixtures pass native/TypeScript reloads, native
  schema-to-SDK refresh, HTTP/MCP before and after source edits, SIGTERM shutdown and
  production configuration preservation. Node starts standalone SDK watch before the
  first contract exists; Bun runs the owned watcher and checks it exits with dev.
- Validation: 1,068 CLI tests pass; CLI rebuild passes; Cargo source inventory passes
  with 754 required embedded source/template files. The initial live run exposed and
  led to correction of Compose variable interpolation; both corrected runs pass.
  Retained runs: `rust-mixed-dev/run.4t05u9pg` (Node), `run.v8f0ns32` (Bun).
- Still open: automatic TypeScript exports for combined mixed-app SDKs and automatic
  socket artifact/WebSocket SDK refresh in development. These remain separate from
  the now-validated native HTTP contract/SDK loop. Full parity is not claimed.

### Mixed TypeScript development export (2026-10-12, validation in progress)

`dev --export-contracts` now plans TypeScript workspace rebuild and OpenAPI export
before startup/reload, alongside native exports. Shared libraries are mounted and
watched; per-container dependency/build volumes isolate runtime outputs. Default
TypeScript OpenAPI version bundles and dotted operation namespaces import into
both SDK languages. Export failure preserves previous artifacts; dry-run remains
read-only. The Node/Bun fixture now covers service schemas, shared-library schemas,
combined SDK refresh, HTTP/MCP after reload, and a TypeScript-only selection replay.

Evidence so far: the 1,070-test CLI suite passes with the isolated-volume change;
the subsequent focused export suite passes both planning and failed/empty-export
artifact preservation, startup gating and unchanged timestamp checks. Cargo package
inventory includes all 755 required embedded source/template files. SDKs from an
actual retained TypeScript export compile in Rust and TypeScript.
The earlier live fixture passed initial HTTP/MCP startup but its schema edit
incorrectly added a request field that MikroORM rejected. That fixture input is
corrected to change only the response. Fresh full reload checks remain pending: the
shared Docker builder reported a missing snapshot. Retries with an owned separate
builder also exceeded the 20-minute image-build deadline before service startup
under heavy shared-machine load. Retained attempts: `run.xw81wnhl` (Node) and
`run.tn_lh14_` (Bun). These runs are not passes; the full reload gate remains open.
Automatic socket export/WebSocket SDK watch and named API-version selection remain
open. The full source-archive installation suite has not been rerun for this change.

Checklist reconciliation: four older billing entries now point to existing
phase-16 implementations and validation: embedded PostgreSQL migrations, direct
and managed Stripe transport, webhook subscription synchronization, and controlled
HTTP provider tests. Docker/Axum fixtures satisfy the original container/mock test
intent without adding testcontainers/wiremock dependencies. Customer persistence,
provider/local-write reconciliation, and configured mapper review remain open.

### Socket development follow-up (implementation in progress)

Native dev contract exports now feed a bounded host-side socket exporter. Planning
checks socket output/ownership collisions against registered service artifacts.
Valid changes refresh portable artifacts, empty registries retire owned output,
and malformed contracts preserve prior output. Atomic socket publication also
keeps unchanged timestamps. `dev --sdk-source http|websocket|all` selects the SDK
surfaces, while `sdk generate/watch --from all` processes HTTP and sockets
sequentially to preserve shared workspace metadata. One owned watcher handles both.

Compiler checking, all 1,074 CLI tests, CLI rebuild, 756-source packaging and
combined Node/Bun SDK workspace builds now pass, including real HTTP/MCP startup.
The socket export/remove/restore Compose integration remains under validation.
No development parity checkbox is closed until that final gate passes.

The earlier Docker cleanup failure has cleared: the owned builder and container
`forklaunch-parity-ts-dev-20261012` are absent after daemon recovery. No shared
daemon restart or global cache cleanup was performed.

### Smaller-task completion and regression checks (2026-10-12)

The compatibility guide records all 44 source package versions, native 0.1.0
counterparts, supported artifact profiles and upgrade boundaries. It explicitly
avoids registry-release and universal runtime-compatibility claims. Three bounded
architecture decisions are now recorded: the native registry OpenAPI emitter is
canonical; explicit transport coercion retains the existing stricter JSON policy;
built-in PostgreSQL persistence uses optional SQLx behind domain store traits.
The service/worker/mixed examples checklist now references the existing runnable
guide and previously validated generated runtime fixtures.

The native SDK crate now commits reviewed catalog OpenAPI, Rust HTTP SDK and
TypeScript HTTP SDK snapshots. They include path/query/header fields, optional
nullable fields, typed success/error responses, response headers and empty 204s.
Eight SDK crate tests and strict all-target clippy pass; CI's existing framework
workspace job runs the snapshot test. Snapshot updates require an explicit local
flag and are refused in CI. Full cross-language snapshot equivalence stays open.

The socket development code passes all 1,074 CLI tests, CLI rebuild and the
756-source packaging inventory (before the new snapshot sources). Combined
`--from all` generation/check, preserved application package policy and real
Node/Bun workspace builds pass in `rust-socket-workspace/run.i4az_z7b`, followed by
both languages' actual HTTP/MCP startup, discovery and invocation checks. A first
fixture attempt omitted its required HTTP artifact; the corrected fixture supplies
both surfaces explicitly. Automatic dev socket export/removal/restoration is now
covered by the pending Bun Compose run; it is not yet claimed as a live pass.

Cleanup removed seven obsolete failed mixed-dev applications while retaining their
logs, three old generated Cargo caches (about 23 GiB), and old test-only target and
node_modules trees across eight fixture families. Source worktrees and retained
fixture sources/logs remain. The previously stuck owned Docker builder is now
absent. Fresh Node and Bun dev checks own distinct projects and clean their Docker
resources on exit.

### Actual TypeScript conformance snapshots (2026-10-12)

`rust_ws_metadata_parity.mjs` now checks a committed 34-document AsyncAPI baseline
after exact actual-TypeScript/native equality. Normalization sorts object keys
only, preserving arrays, special/prototype keys, literal values and all schema
constraints. All cases pass, including official JSON Schema validation, 63 parser
documents, 11 documented parser limitations and independent recursive/literal
reference and payload checks. External non-WebSocket protocol scope stays open.

`rust_billing_contracts_parity.mjs` now records 66 configured TypeScript base/Stripe
routes and 28 complete validator/OpenAPI DTO schemas after the existing native
route and selected schema-property comparisons. Full DTO snapshots add drift
visibility without claiming that those selected comparisons prove every schema
constraint. Both suites pass again without the update flag. The shared JSON
snapshot helper rejects updates in CI; fixture/helper changes trigger the parity
workflow. This closes representative-TypeScript snapshot coverage and normalized
AsyncAPI snapshots, while canonical whole-service OpenAPI equivalence remains open.

The package inventory now includes all 760 required native embedded sources,
including the new native snapshots. The first fresh mixed-dev run reached real
HTTP/MCP and native reload but failed because its shared-schema fixture imported
an unexported `object` helper. The fixture now uses supported `optional({...})`
syntax and waits for service readiness between schema edits. Fresh corrected
Node/Bun runs remain pending; that failure is not a development parity pass.

Two older IAM architectural recommendations are also reconciled with existing
implementation evidence: ForkLaunch owns the base IAM credentials/directory/session
logic, and the separate native Better Auth crate follows API/behavior comparisons
rather than depending on a Rust clone of Better Auth. This does not close the
remaining provider, custom client, ORM/hooks, scalable storage or compliance work.

### Interactive native naming (2026-10-12)

Native initialization now prompts for an omitted name only when stdin and stderr
are terminals. The default is `my-<kind>`; service/worker/library names select their
Cargo package and project directory together. Explicit positional names retain
existing validation. Noninteractive omission fails immediately with guidance and
no application writes. The real-PTY fixture covers application, service, library
and worker prompts, custom/default selection, invalid-name rejection and closed
stdin behavior, then removes its temporary workspace. It is included in the init
command integration suite. All 1,074 CLI unit tests and the rebuilt CLI pass.

The combined SDK workspace fixture now removes its own node_modules/build caches
on success by default, retaining generated source; `FORKLAUNCH_KEEP_TEST_CACHES=1`
opts out for debugging. The cleanup block was exercised against the completed
Node/Bun run and both SDK source trees remain. The expanded Bun dev fixture also
adds the serde_json dependency required by its custom socket registration. That
missing fixture dependency was corrected in the in-flight run before service
startup checks; socket lifecycle/reload validation remains pending.

### Development validation follow-up (2026-10-12, ongoing)

The corrected Node full mixed loop passes in `rust-mixed-dev/run.xb4uz99y`:
HTTP/MCP before/after source edits, native/TypeScript/shared-library contract and
SDK refresh, supervised shutdown and preserved production configuration. Its
first TypeScript-only replay hit the fixture's four-minute fresh image-build limit;
that is not a pass. A retry uses the same retained application and a build-aware
startup deadline. Future full runs keep the owned service image until replay and
use `--no-build`, then remove it, avoiding a redundant dependency install.

Bun's expanded fixture now supplies serde_json for its explicit socket registration,
uses the actual `socket_development.ts` output name, and refreshes ephemeral host
ports while waiting for startup (Docker may change them after a restart). A fresh
run at `rust-mixed-dev/run.b25r_8kn` is validating socket export/change/removal/restore
plus the shared-library loop. The superseded attempts are retained as logs only.
No additional development checklist item is closed while these checks remain open.

The Node TypeScript-only replay now passes on the retained `run.xb4uz99y`
application: a shared-library-only edit updates the SDK, actual HTTP/MCP calls pass,
and the owned watcher exits on shutdown. Bun `run.b25r_8kn` completed socket
creation/change/removal/restoration and TypeScript/shared-library SDK refresh,
but its final probe retained a native host port from before a later restart.
That probe failed and the run is not counted as a full pass. The fixture now
refreshes native readiness/port again immediately before the final probe; fresh
Bun run `run.3r582t9d` includes this correction and the image-reuse replay path.

Billing snapshot review also replaced unresolved dependency-injection placeholders
and JSON-erased role Sets with explicit auth methods and role arrays. Both baseline
update and normal comparison pass all 66 route/28 schema cases again. The
compatibility guide now carries the documentation site's standard frontmatter.

### Explicit compliance metadata derives (2026-10-12)

The new core derive exposes explicit none/PII/PHI/PCI field classification, storage
names and nullability to application-owned persistence adapters. Only classified,
explicitly nullable fields qualify for anonymization. It does not infer compliance
from encryption or Option/serde metadata. Generic structs, raw identifiers and
renamed core dependencies compile; malformed/duplicate declarations fail closed.
Core/macro tests, compile-fail documentation examples and strict all-target clippy
pass. Generic retention enforcement and database compliance integration stay open.

A rebuilt CLI also generates an application containing the new macro crate, core
metadata API and integration tests; all core/macro tests and documentation examples
pass from that generated workspace. Its temporary source is removed after validation.
The CLI package inventory includes all 764 required embedded sources/templates.

### Combined development loop and image reuse (2026-10-12)

Bun `rust-mixed-dev/run.3r582t9d` passes the complete mixed loop: HTTP/MCP
startup and post-edit calls, native and TypeScript reloads, native/TypeScript/
shared-library contract and SDK refresh, socket creation/change/removal/restore,
SIGTERM watcher shutdown and unchanged production configuration. Its subsequent
TypeScript-only replay failed because Compose rejects `up --watch --no-build`.
This is a separate failed replay, not a failure of the completed mixed loop.

The CLI now implements watched `--no-build` through a private image-only plan:
remove build definitions, preserve explicit/default local image names and restart
rules, and forbid pulling replacement build-backed images. Custom rebuild rules
fail explicitly. All 1,075 CLI tests and the rebuild pass. Real TypeScript-only
Bun replay with image reuse is still pending; the earlier Node replay passed.
No additional development checklist gate is closed at this checkpoint.

### Development validation gate completed (2026-10-12)

The corrected Bun TypeScript-only replay exits successfully on retained
`rust-mixed-dev/run.3r582t9d/app`, using watched `--no-build` image reuse. It checks
a shared-library-only change, regenerated SDK content, the actual Compose restart,
HTTP/MCP calls and owned watcher shutdown. Its containers, volumes, network and
image are removed. The image rebuild was only needed because the preceding failed
replay had already performed cleanup. An independent live Compose probe also proves
source-triggered restart with an image-only plan; its owned resources are removed.

Together with the complete Bun mixed loop and Node `run.xb4uz99y` mixed loop plus
separate successful TypeScript-only replay, this closes unified mixed development
and source-triggered contract/SDK refresh. The aggregate old Bun command remains
recorded as failed at its first replay attempt; its main loop and corrected standalone
replay are the successful evidence, not a claimed fresh aggregate exit.

Evidence logs: `/private/tmp/fl-dev-typescript-runtime-bun-sockets-ports.log` (main
loop), `/private/tmp/fl-dev-bun-typescript-replay-final.log` (corrected replay),
`/private/tmp/fl-dev-node-typescript-replay-final.log`, and
`/private/tmp/fl-compose-restart-image-only-final.log`. All 1,075 CLI tests pass.
The earlier CLI package inventory passes all 764 embedded sources/templates.

Current checklist: **617/682 (90.5%), 65 unchecked**. This is an unweighted task
count, not a claim of universal feature coverage. Fourteen entries were resolved
from the 79-open checkpoint, including explicitly documented stale checklist
reconciliation. Production deployment/publication, broader authentication and
persistence, schema/protocol coverage, documentation and release gates remain.

### Generic retention enforcement hooks (2026-10-12)

Core persistence now exposes a typed async store boundary and bounded retention
runner. Policies use explicit compliance metadata and an application-resolved
exclusive cutoff. Calls require tenant identities; duplicate scopes, future
cutoffs and invalid batch bounds fail before store access. Dry runs count without
mutation. Delete and nullable classified-field anonymization preserve prior
committed counts on a failed batch and continue other tenant/entity passes. A
batch cap prevents a broken adapter from looping forever; remaining work is
reported as incomplete enforcement. Counter overflow is an error.

Six native regression tests cover tenant fences, cutoff boundaries, already
anonymized rows, field selection, dry runs, oldest-first deletion, committed-count
reporting, entity filters, empty anonymization field lists and faulty adapters.
The actual TypeScript RetentionService comparison passes 24 combinations of
action, dry-run, batch size and tenant selection. Strict core all-target clippy
passes; CI includes the comparison. Generic database adapters, full calendar/TZ
cutoff handling and directory/ecommerce compliance integration remain open.

Separately, user-authorized cleanup removed 19 clean inactive committed worktrees
and pruned stale registrations. Branch references remain intact; ignored lockfiles
and restoration metadata are backed up under
`~/.codex/workspace-archives/forklaunch-2026-10-12`. Dirty and active workspaces were
preserved. Free disk rose from approximately 74 GiB to 99 GiB.

Validation also passes core all-feature tests, the CLI rebuild and all 767 required
embedded package sources. Checklist: **618/682 (90.6%), 64 open**.


### Application-configured IAM directory compliance (2026-10-12)

Applications can register generic `ComplianceMetadata` types with
`DirectoryCompliance` and use `iam_router_with_compliance`. The default remains
unclassified. Explicit scalar subject mappings select records within the trusted
store tenant; export projects IDs and classified fields, while erasure deletes
whole records to match the actual TypeScript service. Each entity commits
separately. Failed reads/deletes/commits report sanitized failures, never claim
uncommitted deletes, and produce an HTTP/MCP error for incomplete operations.

Validation passed:

- Five actual TypeScript/native configured compliance comparisons, alongside
  the existing 44 directory operations, 33 route contracts and 22 schema layouts.
- Eleven native IAM-base tests, including commit failure/rollback and metadata
  validation; the shared compliance fixture also passed against encrypted
  PostgreSQL with restricted-role tenant RLS.
- Strict all-feature/all-target IAM-base and IAM-axum Clippy.
- Generated HTTP/MCP startup, system-role rejection/acceptance, classified export,
  erasure and repeated-operation 404; existing CRUD, both live SDKs, restart,
  source preservation, relocation, rename and deletion passed.
- Rebuilt CLI and all 770 embedded Rust source/template package entries.

Non-ID subject lookups retain the store's 4,096-record bound and fail if exceeded;
ORM relation inference and scalable indexed traversal are separate tasks. Logs:
`/private/tmp/fl-directory-compliance-{parity,postgres,all-tests,clippy,runtime,package}.log`.
Checklist: **619/682 (90.8%), 63 open**. Percentages count checklist items equally.


### Shared template rendering (2026-10-12)

`generate_with_template`, native scaffolding and SDK templates now share the
same Ramhorn rendering primitive. Native and SDK callers also share embedded
file lookup and contextual diagnostics. The legacy directory walk, TypeScript
namespace/database replacements, native preservation rules and SDK ownership
logic remain at their existing boundaries. All 1,075 CLI tests pass
(`/private/tmp/fl-shared-template-tests.log`).

Checklist: **620/682 (90.9%), 62 open**.


### Checked binary WebSocket metadata (2026-10-12)

`SocketSchemaEntry::binary()` persists an explicit binary marker with a canonical
schema. Checked plans emit byte codecs in both SDK languages, preserve invalid
UTF-8/JSON-looking payloads, and publish `application/octet-stream` per message
in AsyncAPI. JSON Schema format annotations alone do not opt into raw bytes.
Ambiguous message unions, extra binary schema assertions and binary close
reasons fail generation. Ping/pong may use binary codecs within protocol bounds.

Validation: both generated SDKs compile and exchange ten actual binary messages
with an isolated `ws` server, plus control frames and oversize rejection. Empty
payloads, all byte values, invalid UTF-8 and JSON-looking bytes round-trip exactly.
The existing TypeScript SDK suite passes 12,960 native and actual TypeScript/AJV
codec comparisons and its authorization, cancellation, queue, control/close and
malformed-peer checks. All 77 selected native tests, strict all-target/all-feature
Clippy, rebuilt CLI and all 770 package entries pass. CI includes
`cli/tests/rust_ws_binary_runtime.mjs`. Logs are
`/private/tmp/fl-binary-socket-{runtime,json-regression,tests-final,clippy-final,package}.log`.

Checklist: **621/682 (91.1%), 61 open**. External binary AsyncAPI imports remain
separately scoped; this closes explicit native artifact metadata and generation.


### Context-aware schema dictionary bundling (2026-10-12)

Checked unscoped schema bundling now traverses each referenced dictionary in a
separate interpretation context. It copies targets into collision-checked
schema definitions and rewrites only structural schema locations, preserving
literal `const`/annotation data even when the same source value is also a schema.
Recursive targets, nested contexts and document extraction use bounded shared
copies; missing references, active resources and invalid targets fail explicitly.
Socket union relocation now selects the checked unscoped helper.

Validation passed: 34 existing actual TypeScript/native metadata snapshots,
six original/exported AJV alias comparisons, native recursive/union constraint
checks, 74 selected framework tests, strict all-feature/all-target Clippy,
generated Rust consumers with warnings denied, and strict TypeScript ESM/CJS
consumers with runtime alias-union validation. CLI rebuild and all 770 package
entries pass. Logs: `/private/tmp/fl-schema-context-{metadata,tests-final,rust-sdk,ts-sdk-final,clippy,package}.log`.

Checklist: **622/682 (91.2%), 60 open**. General resource IDs/anchors/dynamic
resolution remain open; resource-preserving relocation still rejects aliases
whose scope cannot be safely resolved.


### Telemetry presence checkpoint (2026-10-12)

The generated telemetry fixture now starts both HTTP and MCP, invokes health,
and rejects untrusted origins before verifying real OTLP exports. Assertions
cover W3C parent IDs, trace correlation, response status/size/duration metrics,
zero active-request balances after completion, exclusion of correlation IDs
from metric dimensions, structured error logs and SIGTERM flushing. Offline
exports still succeed with intentionally invalid live telemetry settings.

The relocated generated app/worker build passes. Both native telemetry middleware
tests pass, covering auth/unmatched requests and streaming completion, error,
cancellation and HEAD. The actual receiver fixture passes HTTP/JSON and protobuf
signals, exporter authentication/resource configuration, disabled export and
sanitized invalid configuration. Existing CI runs both the direct OTLP fixture
and the generated shell fixture. Logs:
`/private/tmp/fl-telemetry-presence-{generated,middleware,otlp}.log`.

Checklist: **623/682 (91.3%), 59 open**. This verifies telemetry presence and
lifecycle behavior; it does not claim identical TypeScript metric names or
vendor-specific telemetry configuration.


### SSE emission and validation checkpoint (2026-10-12)

The optional `forklaunch-axum/sse` helper emits bounded `SseFrame<T>` streams.
`respond` validates typed data with garde; `respond_with` accepts whole-envelope
validation. Every-event validation is the default; explicit first-event mode
matches the actual TypeScript emitter's validation frequency. The first event
is prepared before headers, later errors terminate the body, and cancellation
drops producers without background tasks or prefetch queues. IDs and wire byte
limits are checked in both modes. Standard multiline framing and sanitized errors
are intentional differences from TypeScript's raw interpolation/error detail.
Applications supply validators matching their contracts; arbitrary contract JSON
Schema is not automatically applied by this helper.

Validation passed: 46 all-feature Axum tests, including six SSE tests for typed
validation, first/later errors, exact Unicode/escaped-string byte limits and
cancellation before the first event or while awaiting subsequent events; strict
all-target/all-feature Clippy; 16 frames compared with the actual TypeScript
emitter plus real Zod response validation and empty-stream behavior. The relocated
generated service passes both SDKs, contract/OpenAPI artifacts, native/imported
fetch, stream failures and resource release, with real HTTP/MCP startup and
shutdown. The final CLI rebuild and all 772 package entries pass. The generated
runtime run preceded the final escaped-string accounting fix; final unit tests
cover that boundary. CI runs the emitter conformance and generated runtime tests.
Logs: `/private/tmp/fl-sse-emitter-{tests-final,clippy-final,parity-final,generated,cli-build-final,package}.log`.

Checklist: **624/682 (91.5%), 58 open**. The broader schema-constraint,
multipart, resource-scope and external AsyncAPI tasks remain open.


### External AsyncAPI binary messages (2026-10-12)

External AsyncAPI 3.0 WebSocket imports now preserve explicit
`application/octet-stream` messages as native binary metadata. Canonical
string/binary payloads may be inline or referenced locally. Send/receive may
use different wire encodings; each binary direction selects one message.
Mixed wire alternatives, ambiguous byte messages, additional schema assertions,
reference siblings/cycles/external targets and encoded pointer separators fail
checked import. JSON `format: binary` alone does not enable byte transport.

Validation passed: 25 AsyncAPI tests, strict Clippy, both compiled SDKs with ten
live imported-binary roundtrips (including invalid UTF-8, JSON-looking bytes and
all byte values), and the existing native binary/control-frame regression.
The rebuilt CLI passes relocated import/check/watch, strict SDK compilation,
live JSON peers, rename/delete ownership, and generated HTTP/MCP startup and
shutdown before and after rename. All 772 embedded package entries pass. CI
runs both native-artifact and imported-binary modes. Logs:
`/private/tmp/fl-asyncapi-binary-{tests,clippy,runtime,native-regression,lifecycle,cli,package}.log`.

Checklist remains **624/682 (91.5%), 58 open**: this completes binary payload
support within the broader external AsyncAPI item; its other profiles remain open.


### External AsyncAPI JSON Schema dialects (2026-10-12)

The importer now accepts Multi Format payloads for Draft 4, Draft 6, Draft 7,
2019-09 and 2020-12 with explicit matching `schemaFormat`/`$schema` declarations.
Document-local references and dialect-specific constraints survive extraction;
message unions retain their dialect. Unknown/mismatched formats, nested mixed
dialects and alternatives with different dialects fail checked import. Draft 4
message discrimination no longer treats ignored `const` annotations as assertions;
required enum tags provide actual discrimination.

Validation passed: 45 AsyncAPI/codegen tests, strict AsyncAPI Clippy, generated
TypeScript compilation and Rust compilation for all five dialects, ten live
roundtrips across both SDKs and twenty invalid numeric-bound rejections. Native
checks also cover references inside Multi Format schemas, Draft 4 unions and
mixed-dialect rejection. The source document remains unchanged. The CLI rebuild
and all 772 embedded source/template entries pass. CI includes the new
`rust_asyncapi_dialects_runtime.mjs` fixture. Logs:
`/private/tmp/fl-asyncapi-dialects-{tests-final,clippy-final,runtime,cli,package}.log`.

Checklist remains **624/682 (91.5%), 58 open**. This completes supported JSON
Schema dialect imports within the broader AsyncAPI task; other formats/protocols,
JSON string wire encoding, upgrade auth/parameters and traits/replies remain.


### Explicit JSON WebSocket wire encoding (2026-10-12)

`SocketSchemaEntry::json` / portable `jsonEncoding: true` now preserve JSON
strings and null through shared SDK plans and both generated clients. Native
`PayloadCodec::with_json_encoding` retains validators and byte limits while
requiring strict UTF-8 JSON, including quotes/escapes in wire byte counts.
External AsyncAPI JSON messages automatically select this mode. Native artifacts
retain existing raw-string behavior by default; mixed raw/JSON alternatives and
simultaneous binary/JSON flags are rejected. Explicit JSON exports carry JSON
media even when another document default is configured.

Validation passed: 84 selected framework tests and strict all-feature/all-target
Clippy across WebSocket, AsyncAPI and SDK crates; all 12,960 existing actual
TypeScript/native codec comparisons and the complete generated TypeScript
WebSocket runtime fixture; both generated clients with 16 exact-wire string/null
roundtrips, malformed-peer rejection and encoding/UTF-8/byte-limit checks. The
five-dialect and imported-binary live fixtures also pass with the new codecs.
The rebuilt CLI passes relocated import/check/watch, strict SDK consumers,
live peers, rename/delete ownership, and generated HTTP/MCP startup/shutdown
before and after rename. All 772 embedded package entries pass. CI includes
`rust_ws_json_encoding_runtime.mjs`. User-facing generation and compatibility
docs now describe the supported importer instead of labeling all imports pending.
Logs: `/private/tmp/fl-ws-json-encoding-{tests,clippy,regression,runtime-final,dialects,binary,cli,package,lifecycle}.log`.

Checklist remains **624/682 (91.5%), 58 open**. Binary messages, the five JSON
Schema dialects and JSON string/null wire encoding are complete subparts of the
external AsyncAPI task. Its event protocols, upgrade auth/parameters and
traits/replies remain open; no full-parity claim is made.

### BullMQ options audit and generated runtime validation (2026-10-12)

Completed the [pinned BullMQ options audit](RUST-BULLMQ-OPTIONS-AUDIT.md), including
producer/consumer mapping, retry and retention policies, waiting/active inspection,
tenant prefixes and restricted Redis ACL behavior. Fixed missing event-stream
metadata limits and `skipMetasUpdate`, enforced the Redis 5 minimum with a working
`skipVersionCheck` for both queue and worker, and accepted zero attempts as one
execution without retry, matching TypeScript.

Validation: the full actual TS/native BullMQ interoperability fixture passed;
focused options coverage passed for metadata defaults/custom/preserved settings,
150-job event-stream trimming from both producers, zero attempts in both language
directions, and ACL-denied INFO/HSET opt-outs. All 67 pinned protocol unit tests
and all 12 actual TS/native scheduler scenarios passed; strict all-feature/all-target
adapter Clippy passed. The rebuilt CLI
included all 772 embedded package entries. A relocated generated BullMQ worker
passed processing/retries, lifecycle transitions and graceful shutdown; its API
passed real HTTP/MCP startup, initialization, discovery, health invocation,
listener configuration and shutdown checks.

Checklist: **625/682 (91.6%), 57 open**. Audit completion does not close the broader
production-worker gate: custom scheduler strategies, BullMQ telemetry hooks,
Cluster/Sentinel connections and full flow/parent interoperability remain explicit
implementation requirements in the audit.

### Optional TanStack Query SDK helpers (2026-10-12)

Both the native TypeScript SDK generator and CLI share optional, dependency-free
`forkLaunchQueryOptions` and `forkLaunchMutationOptions`. They preserve generated
request/variable types and successful status-specific response envelopes, throw
HTTP failure envelopes through `ForkLaunchSdkHttpError`, and propagate transport
errors. Query cancellation composes TanStack and caller signals with listener
cleanup; cache keys, retry policy, selection and invalidation stay explicit.

Validation: actual Query Core and React Query 5.104.2 checks passed for strict
request/response inference, scoped keys, cache reuse/invalidation, HTTP errors,
retries, mutations, network errors and caller/query/pre-aborted cancellation.
Eight SDK crate tests, reviewed output snapshots, strict SDK Clippy and all 31
CLI generation tests passed. The rebuilt CLI generated and typechecked both SDKs
through contract and OpenAPI paths; both clients passed live Rust and TypeScript
wire fixtures. The generated Rust service also passed real HTTP/MCP startup,
initialization/discovery/tool invocation and graceful shutdown checks.

Checklist: **626/682 (91.8%), 56 open**. No TanStack runtime dependency is added to
ordinary generated SDK use. See the framework README for cache scoping and usage.

### Literal JSON object keys across request, MCP and persistence boundaries (2026-10-12)

Completed the [JSON boundary audit](RUST-JSON-BOUNDARY-AUDIT.md) and fixed literal
`$serde_json::private::Number` / `$serde_json::private::RawValue` object keys being
interpreted as internal Serde markers under unified dependency features. The
vendored 1.0.151 parser distinguishes real map keys from internal number/raw
markers, preserving exact numbers, RawValue, nested DTO values and owned/borrowed
Value conversions. Framework, blueprint and generated application roots share
one patched parser; dependency pins prevent silent bypass by newer registry
versions. Generation/sync adds missing patches and preserves user overrides.

Validation: 237 default-feature and 262 all-feature upstream/parser regression
tests passed, plus alloc-only compilation. Framework and blueprint library
suites passed 242 and 108 tests respectively (4 and 5 environment-gated tests
ignored). Strict framework workspace Clippy, focused manual/routed MCP checks,
and five CLI workspace migration tests passed. Actual TS/native Redis and S3
fixtures passed both encrypted wire formats and both producer/reader directions
with literal marker objects under raw-value/arbitrary-precision features. The
expanded actual TS/AJV/native WebSocket matrix passed all **13,908** cases.

All **859** bundled source/template entries are packaged. A fresh source archive
installed the CLI, generated and relocated all native module families, compiled
both languages' SDKs through native/contract/OpenAPI/AsyncAPI paths, and passed
live HTTP/MCP startup and capability checks. A separate relocated HTTP fixture
passed literal-object HTTP and MCP roundtrips, both SDK languages against Rust
and TS wire servers, contract/OpenAPI regeneration and graceful shutdown.

Checklist: **627/682 (91.9%), 55 open**. External workspace consumers must apply
the documented root Cargo patch. Registry publication/upstream adoption,
unpaired surrogates, deeper nesting and broader ORM/JavaScript normalization
remain separate gates.


### Canonical actual TypeScript service fixture (2026-10-12)

The paired IAM runner now bundles the repository's actual Express, core, common
and Zod implementation, starts it beside the existing native sample IAM router,
and checks eighteen live HTTP outcomes plus both generated OpenAPI endpoints.
Successful user reads/lists, empty missing-user responses and missing credentials
match. Exact malformed/expired/forged JWT expectations retain three differences:
TypeScript returns 403 and native returns 401. TypeScript uses `authenticated`
for JWT-only access; the native IAM contracts still label it `protected` without
RBAC. These are explicit remaining auth/error conformance work, not normalized
away. Pagination is intentionally unchanged in the sample handlers.

External TypeScript dependencies are pinned in the fixture manifest; CI installs
them in a dedicated prefix. The runner bundles source in an isolated temporary
directory, disables telemetry export, uses ephemeral loopback listeners and
cleans up its own processes/files. The native example passes strict Clippy.
Whole-document OpenAPI comparison, remaining validation/auth modes and MCP
framework conformance remain open. Checklist: **628/682 (92.1%), 54 open**.


### Canonical fixture follow-up: access labels and valid OpenAPI (2026-10-12)

The actual service comparison exposed TypeScript OpenAPI security requirements
that did not resolve to their declared schemes and path parameters missing the
mandatory required flag. The emitter now references its declared default,
explicit Authorization or custom-header scheme names; HTTP Basic/JWT/API-key
requirements carry empty scope arrays rather than RBAC permission names. Four
actual-source emitter cases and both live documents enforce these invariants.
JWT-only native IAM contracts now use `authenticated`, matching TypeScript's
access semantics without changing credential checks or adding implicit RBAC.
All eight IAM-interface/sample-service library tests and the eighteen live
HTTP outcomes pass. The three rejected-JWT status differences remain explicit;
whole-document OpenAPI and broad auth/error conformance remain open.
Checklist remains **628/682 (92.1%), 54 open**.


### Canonical whole-document snapshots and normalized comparison (2026-10-12)

Both real service OpenAPI documents now have committed, key-sorted snapshots.
The live comparison resolves local schema references and normalizes schema
annotations, set ordering and disjoint primitive unions. Operation sets,
parameters, security requirements and successful response schemas match.
Ten remaining error-response entries are pinned separately in
`cli/tests/fixtures/canonical-iam/openapi-differences.json`; the comparison cannot
silently discard them or accept new differences. The fixture's optional nullable
DTO fields and integer query limit now match the native sample's published types.
Snapshot creation and a separate ordinary verification run pass alongside all
eighteen live HTTP checks. This closes the snapshot/comparison harness task,
not full document/error parity or the three rejected-JWT status differences.
Checklist: **629/682 (92.2%), 53 open**.


### Canonical rejected-JWT status alignment (2026-10-12)

Declared JWT routes now return 403 for presented but rejected credentials,
matching actual TypeScript behavior for malformed, expired and forged JWTs.
Missing credentials remain 401. The mapping is applied at the native HTTP
contract boundary; opaque bearer sessions and application-owned authenticators
without JWT metadata retain their 401 behavior. Error envelopes remain native
JSON and are still a separate parity task. A nine-case middleware regression
checks JWT versus opaque/untyped auth, missing/invalid/forbidden failures and
correlation IDs. All 58 auth/Axum library tests, strict all-feature/all-target
Clippy, the eighteen actual-framework HTTP outcomes and OpenAPI snapshots pass.
The canonical fixture has no remaining rejected-JWT status exceptions.
Checklist remains **629/682 (92.2%), 53 open**.


### Typed JWT issuance and actual JOSE interoperability (2026-10-12)

`forklaunch_auth::issue_jwt_hs256` now issues explicit server-owned `JwtClaims`
through jsonwebtoken, complementing existing HS256 and configured JWK/JWKS
verification. Empty subjects/keys fail; applications retain expiry, secret,
rotation and grant policy. The canonical native example issues a test token,
actual JOSE verifies every claim and both TypeScript/Rust services accept it.
JOSE-issued tokens continue to authenticate against native verification.
All twenty live HTTP outcomes, full OpenAPI snapshots, fifteen auth tests and
strict auth/example Clippy checks pass. Documentation distinguishes the simple
JWT helper from Better Auth's provider-specific signing and cookie/OIDC flows.
Checklist: **630/682 (92.4%), 52 open**.


### Six-surface runtime performance harness (2026-10-12)

The optional `forklaunch-testing/benchmarks` example and `rust_benchmarks.mjs`
measure typed JSON validation, contract-driven HTTP, generated TypeScript SDK
calls, Redis SET/GET, S3 PUT/GET and typed WebSocket round trips. Every result is
checked outside its timer. Warmups, sample distributions, build/compiler/runtime
versions, source revision/dirty state and host metadata are recorded in JSON.
Storage uses unique owned keys; missing infrastructure is explicitly skipped or
rejected with `--require-infra`. The optimized profile is the default; CI runs a
short debug smoke against its isolated Redis/MinIO and uploads the report without
unstable latency thresholds. Documentation states the single-client, 1033-byte,
unencrypted measurement boundaries and excludes production performance claims.

All six paths passed 50 debug samples after ten warmups and 100 optimized samples
after twenty warmups against live dedicated Redis/MinIO. Strict benchmark Clippy,
workspace formatting and 861 packaged-source checks pass. Redis reported zero
remaining keys; both owned infrastructure containers were removed after use.
The generated benchmark example is included in the CLI source bundle.
Checklist: **631/682 (92.5%), 51 open**.


### Strict HS256 time claims found during security review (2026-10-12)

The expanded actual TypeScript fixture first reproduced a native 200 response
for a token that expired thirty seconds earlier, where TypeScript returned 403.
The HS256 principal helper inherited jsonwebtoken's default sixty-second leeway
and disabled not-before checking. It now uses zero leeway, enforces `exp <= now`
as expired, validates optional `nbf`/`iat` numeric types, preserves fractional
not-before comparisons and rejects empty signing keys. Future `iat` alone remains
valid, matching JOSE without maxTokenAge. Explicit caller-owned JWK/JWKS Validation
policies remain unchanged. All 34 actual-framework HTTP outcomes, sixteen auth
tests, OpenAPI snapshots and strict auth Clippy pass. Earlier relocated generated
HTTP/MCP checks and 108 blueprint library tests also passed (five environment-gated
tests were ignored); their owned generated workspace was removed.
Checklist remains **631/682 (92.5%), 51 open**. Dependency and broader security
review continue; this is a concrete finding/fix, not a completed security audit.


### Engineering security review and advisory gate (2026-10-12)

[RUST-SECURITY-REVIEW.md](RUST-SECURITY-REVIEW.md) records auth/JWT/HMAC, encryption,
secrets, HTTP/MCP and generated SDK boundaries with concrete evidence and explicit
application-owned controls. Removing the redundant AWS legacy Rustls connector
removed four framework vulnerability findings (old h2 and webpki). Framework and
blueprint lockfiles upgrade anyhow to 1.0.103. The CLI now aliases its YAML parser
to serde_norway 0.9.42, removing the archived serde_yml/libyml implementations;
all 1,076 CLI tests pass. S3 passes nineteen local and two live MinIO tests.

The cargo-audit 0.22.2 gate retains three exact reviewed findings: AWS S3's LRU
uses a non-panicking String key; the RSA crate performs generation/encoding while
ring performs private signing; paste is a compile-time maintenance notice.
All findings stay visible, with no scanner ignores or severity/platform filters.
Changed versions/new advisories fail; offline tests exercise those failure paths.
CI uploads current reports. The broad framework library run passes 245 tests
(four ignored), and the rebuilt CLI again passes relocated HTTP/MCP startup,
invocation/Origin rejection/shutdown and both SDKs against Rust/TypeScript via
contract/OpenAPI artifacts. The owned generated workspace and MinIO container
were removed. This completes the engineering review, not the separate remaining
provider, deployment, scalability and implementation gates.
Checklist: **632/682 (92.7%), 50 open**.


### Generated Rust SDK diagnostic redaction (2026-10-12)

Both Rust emitters redact client URLs, default-header values and typed/transport
request values in Debug output, retaining field names and body-presence flags.
Reqwest transport errors strip their URLs. Generated consumers assert that URL
credentials, auth headers, query/path tokens and text/binary bodies do not leak.
Native SDK media/strict compilation, five Rust SDK tests and strict all-target
Clippy pass. Rebuilt CLI-generated clients pass the new assertions through both
contract/OpenAPI artifacts, calling Rust and compiled TypeScript services.
HTTP/MCP startup, initialization, discovery, invocation, Origin rejection and
graceful shutdown pass. The owned generated workspace was removed.
Checklist remains **632/682 (92.7%), 50 open**.


### Typed multipart request extraction (2026-10-12)

The opt-in Axum multipart feature supplies `Multipart<T>` with automatic Serde
decoding and garde validation, plus independent field/count/total byte limits.
Files preserve bytes and filenames never become paths. Requests reject malformed
MIME, missing/unknown fields, invalid UTF-8, model violations and size overflow
before handler entry, without echoing uploaded values. Seven comparisons execute
the actual TypeScript discriminator and Express content parser with Busboy; they
cover Unicode, binary/empty/repeated files, repeated text and filename-less
octet-stream classification. Legacy charset decoding and mixed text/file name
collisions are explicitly outside this comparison's portable contract.
All-feature Axum tests pass (48 library and three live integration tests), strict
all-feature/all-target Clippy passes, and all 863 embedded source/template entries
are packaged. CI runs the actual-parser comparison; owned fixture output cleans
itself up. The multipart checkbox stays open for SDK response decoding.
Checklist remains **632/682 (92.7%), 50 open**.


### Multipart SDK response decoding (2026-10-12)

CLI contract/OpenAPI generation and both framework-native emitters now decode
`multipart/form-data` responses into TypeScript FormData or Rust ordered part
records. Names, repeated parts, filenames and binary/empty file bytes survive.
Decoding enforces a configurable positive 8 MiB default wire-byte bound and a
1,024-part bound. Wrong media, absent/invalid boundaries, malformed/truncated
bodies and excessive bytes/parts fail. Multipart/mixed remains explicitly
unsupported; parts are transport values, not automatically validated JSON DTOs.
Rust uses multer; TypeScript uses the platform FormData parser with bounded
Web/Node stream collection and cancellation. Generated Cargo dependencies are
conditional. Standalone native source documents its required dependencies.

The full relocated HTTP fixture passes both artifact formats, both native SDK
generators, native/injected cross-fetch, positive/zero byte limits, excessive
part count, malformed cases and abort signals against Rust and compiled
TypeScript services. Native generated Rust Clippy is strict. HTTP/MCP startup,
initialization/discovery/invocation, Origin rejection and graceful shutdown pass.
All 84 CLI SDK tests and 32 framework codegen/SDK tests pass, as do strict
framework SDK Clippy and 864 packaged source/template checks. Reviewed snapshots
include the preceding diagnostic redaction and the new response runtime. Owned
generated workspaces were removed. Together with typed extraction, this closes
the multipart runtime/SDK checkbox. Broad MIME/MCP/schema conformance stays open.
Checklist: **633/682 (92.8%), 49 open**.


### Authentication framework candidate evaluation (2026-10-12)

[RUST-AUTH-FRAMEWORK-EVALUATION.md](RUST-AUTH-FRAMEWORK-EVALUATION.md) completes
the ten selection criteria with official release metadata and hashes of inspected
release sources. It covers maintenance/dependencies, password upgrades,
session/CSRF storage, OAuth/OIDC, tenant and entitlement boundaries, passkeys,
framework independence and contract/SDK/MCP integration. It selects narrow
tower-sessions, openidconnect/oauth2 and later webauthn-rs adapters while keeping
ForkLaunch IAM and policy ownership. Higher-level framework adoption is deferred.
The review records the tower-sessions/axum-login maintenance request, rs-auth
account-linking/verified-email and state-store concerns, and explicit integration
acceptance conditions. This is a source spot-review, not an audit certification
or completed provider/cookie integration. No production dependency changed.
All 24 non-registry source links returned HTTP 200; twelve registry metadata
links point to the successfully fetched official API (the HTML /versions route
returned 404). The evidence JSON parses and local document links resolve. Owned
downloaded review sources were removed after recording hashes.
Checklist: **643/682 (94.3%), 39 open**. The ten new completions are evaluation
tasks; the percentage measures checklist completion, not runtime feature weight.

### Optional server-side cookie sessions

`forklaunch-axum/cookie-session` integrates tower-sessions 0.15 behind an opt-in
feature. Applications supply the session store and live subject resolver; routes
explicitly declare their cookie name. Secure/HttpOnly/SameSite=Strict cookies,
random synchronizer tokens, exact HTTPS Origin checks, ID/token rotation and
logout are covered. Unsafe login/logout requires CSRF; bearer routes never use
cookie authentication as a fallback. OpenAPI uses distinct named cookie schemes.
MCP preserves transport browser-security headers over tool-argument overrides.

Validation: 97 affected all-feature tests, the additional feature-disabled
fail-closed test, strict all-target Clippy, 865 packaged source/template files,
and the three-lockfile RustSec gate pass with no new advisory exceptions.
Live loopback HTTP and MCP exercise valid/invalid cookies; tests also cover
fixation, old tokens, cross-origin requests, logout and current disabled-user
resolution. Logs: `/private/tmp/fl-cookie-final-tests.log`,
`fl-cookie-default.log`, `fl-cookie-clippy.log`, `fl-cookie-package.log`,
`fl-cookie-audit.log`.

This closes the tower-sessions adapter recommendation. Production store choice,
atomic application revocation, OAuth/OIDC providers and Better Auth client/ORM
parity remain separate work. Read-only access does not extend the thirty-minute
session expiry; session writes do. Checklist: **644/682 (94.4%), 38 open**.

### Document-local SDK schema anchors

The shared CLI/native HTTP SDK compiler now resolves static `$anchor` fragments
at schema positions, including OpenAPI components and guarded recursive models.
Anchor and pointer aliases reuse one graph node. A bounded index excludes
literal examples/defaults and nested resource scopes; malformed/duplicate or
missing anchors fail closed. Draft 4/6/7 do not gain newer anchor semantics.

The mutual-recursion fixture now uses named anchors. A freshly rebuilt CLI
created and relocated the application, generated both SDKs from contract and
OpenAPI artifacts, compiled both languages and ran live recursive round trips.
Both native generators passed the same consumers. HTTP/MCP startup, discovery,
invocation, Origin rejection, port/listener variants and shutdown passed.
Validation: 86 CLI SDK tests, 34 native SDK/codegen tests, strict codegen and
generated-client Clippy. Logs: `/private/tmp/fl-anchor-cli-tests.log`,
`fl-anchor-final-tests.log`, `fl-anchor-clippy.log`, `fl-anchor-runtime.log`.

The resource-ID/dynamic-reference checklist item remains open; this is a
verified part of it. Checklist remains **644/682 (94.4%), 38 open**.

### Bounded auth-store pagination

Better Auth transactions now expose insertion-order keyset pages with opaque
model-scoped cursors and 1..=4096 limits. PostgreSQL uses a new
(tenant_id, model, sequence) index and decrypts at most limit+1 records per page;
the memory backend maintains ordering and unique-field indexes. Replacement,
deletion and rollback keep those indexes consistent. Page Debug output omits
auth record contents. Stable traversal uses one existing serialized transaction.

Memory and live restricted-role encrypted PostgreSQL pass 4,100-record paging,
exact boundaries, invalid limits/cursor models, deleted cursor rows, replacement
ordering and rollback. The PostgreSQL test also retains encryption, tenant RLS,
rotation, ciphertext/index transplant rejection and atomic audit checks. The
all-feature crate suite passes 29 tests (one database test run separately), and
final memory-store regressions pass after replacing unique-field scans. Strict
all-target Clippy and packaging of 866 embedded sources/templates pass.
Logs: `/private/tmp/fl-auth-pagination-postgres.log`,
`fl-auth-pagination-all-tests.log`, `fl-auth-pagination-final-store.log`,
`fl-auth-pagination-clippy.log`, `fl-auth-pagination-package.log`.
The owned PostgreSQL container was removed after validation.

Account/session/member query migration and encrypted lookup backfill remain
open. The existing full-model list still rejects oversized results, preventing
silent authorization truncation. Checklist remains **644/682 (94.4%), 38 open**.

### Actual MCP transport/body conformance

`rust_mcp_transport_parity.mjs` bundles the repository generator and Zod validator
against pinned FastMCP 4.22.4/Zod 4.6.7/MCP SDK 1.32.1. The official client
initializes and discovers the actual FastMCP server over InMemoryTransport;
32 tool calls proxy to a live HTTP receiver. No validator or FastMCP substitute
is used. Multipart text/Unicode/newline/name handling and rejected non-string
shapes, file/binary bytes, form composites, JSON/text and custom media match the
native encoder. The existing generator-only fixture independently checks exact
multipart framing. Native MCP tests invoke the real multipart/form/file HTTP
extractors behind authentication guards.

All 32 actual transport cases and all 18 native MCP tests pass. CI installs the
shared pinned fixture dependencies and runs the new transport comparison.
Sessions, HTTP connections and temporary sources close/clean up on completion.
Logs: `/private/tmp/fl-mcp-transport-final.log`, `fl-mcp-transport-native.log`.
An initial fixture run exposed a retained FastMCP ping timer; retaining and
closing the session returned by `connect` fixes fixture shutdown. The clean
final run exits normally without force termination.

This closes the multipart/expanded-generator MCP item, not the separate
versioned-API or broad route/schema conformance gates. Checklist:
**645/682 (94.6%), 37 open**.

### Indexed opaque-session authentication

Better Auth exact token lookup now uses the memory unique-field index and
rotating PostgreSQL blind indexes. Fetch/refresh, single-token revocation and
sign-out no longer load the session model. Opening a PostgreSQL store rebuilds
legacy token claims under the tenant write lock in 256-record pages. Duplicate
or corrupt legacy records fail startup with transactional rollback. Coordinated
index-aware writer rollout is required; old binaries do not maintain claims.
Token uniqueness is a deliberate stricter constraint than the configured TS
entity's unindexed token field, preventing ambiguous credential ownership.

Memory and restricted-role encrypted PostgreSQL authenticate and revoke with
4,100 sessions while the legacy list still fails closed. The PostgreSQL fixture
removes token claims from the legacy dataset, reopens the store and validates
backfill beyond 4,096 rows. All 30 native tests and strict all-target Clippy pass;
the separate database test passes. Actual Better Auth comparison retains 63
checks/48 body/storage matches. A rebuilt CLI generates the latest migrations
and sources, and the relocated app passes both SDKs, actual frontend clients,
HTTP/MCP listener/startup/shutdown, restart and rename/delete.
Logs: `/private/tmp/fl-session-index-tests.log`, `fl-session-index-postgres.log`,
`fl-session-index-parity.log`, `fl-session-index-clippy.log`,
`fl-session-index-runtime.log`. The owned database and generated workspace were
removed after completion. Account/member indexes and session collection queries
remain open. Checklist remains **645/682 (94.6%), 37 open**.

### Callback/client checklist reconciliation

The callback exchange/client integration checkbox remained open after the
implemented popup-helper and actual shared/generated frontend-client work
recorded above. The fresh `fl-session-index-runtime.log` run revalidates both:
trusted signed callback exchange, JWT/session/email fragment fields and cookie
clearing, independently verified token/JWKS, and eight actual client namespace
scenarios (80 requests). HTTP/MCP startup, persistence, shutdown and module
rename/delete also pass. Mark the specific callback/client task complete;
optional plugins/hooks and unified mixed SDK behavior retain their own gates.
This is reconciliation of verified implementation, not a newly added runtime
feature. Checklist: **646/682 (94.7%), 36 open**.

### Named-version MCP reference regression

The real FastMCP fixture exposed an array-index lookup against the named version
map, plus missing response metadata on bodyless versioned routes. Select the
version from the same ordered values used to build input schemas and always
retain its response contract. Actual MCP calls now verify both named versions
of POST and GET, forwarded bodies/query strings, decoded responses and rejected
mismatched input. All 32 existing native/TypeScript transport comparisons also
pass (`/private/tmp/fl-mcp-version-after.log`). Rust versioned APIs remain open;
this repairs the TypeScript reference behavior. Checklist unchanged: 646/682.

### Session collections beyond the model scan bound

Single-session revocation now resolves the target token through its unique index.
Listing/revoke-others/revoke-all traverse 256-record keyset pages in one locked
transaction, retaining foreign sessions, insertion order and the configured
expired-session distinction. Memory and restricted encrypted PostgreSQL verify
4,099 owned sessions, deletion across page boundaries and foreign-user isolation.
The PostgreSQL fixture also retains unrelated credential-fixture sessions.
All 30 native tests, strict all-target Clippy and 63 actual Better Auth checks
(48 native response/storage comparisons) pass. Logs:
`/private/tmp/fl-session-collections-tests.log`, `fl-session-collections-memory.log`,
`fl-session-collections-postgres.log`, `fl-session-collections-clippy.log`,
`fl-session-collections-parity.log`. Full indexed relationship traversal remains
open; checklist unchanged at 646/682.

### Encrypted relationship indexes and scoped collection pages

Add transaction-scoped `find_page` with ordered memory indexes and forced-RLS
PostgreSQL relationship blind indexes. Retained-key queries deduplicate matches
and verify decrypted values/sequence before returning rows. Startup rebuilds
legacy claims in bounded pages under the tenant write lock; migration 0003
requires runtime table privileges and a coordinated index-aware writer rollout.
Startup backfill cost remains proportional to stored records.

Credential lookup, linked-account listing/removal, session collections and basic
organization membership lookup/listing use these indexes. More than 4,096 users,
accounts and sessions pass native and restricted-role PostgreSQL HTTP/MCP tests.
Fixtures cover cursor deletion, replacement/removal, rollback, exact Unicode/case
matching, invalid limits/fields, tenant isolation, legacy backfill, key rotation
and transplanted index rejection. All 30 native tests and strict all-target
Clippy pass. Actual TypeScript comparisons pass: 81 credential, 50 profile/account,
134 organization and 48 session response/storage cases (63 session checks).

The rebuilt CLI packages 867 Rust source/template files. Its relocated generated
app passes both SDKs, eight frontend-client scenarios, HTTP/MCP startup and tool
invocation, persistence/restart, source preservation, rename and delete. Logs:
`/private/tmp/fl-auth-relations-tests-final.log`, `fl-auth-relations-clippy-final.log`,
`fl-auth-relations-postgres-final.log`, `fl-auth-relations-accounts.log`,
`fl-auth-relations-profiles.log`, `fl-auth-relations-organizations.log`,
`fl-auth-relations-sessions.log`, `fl-auth-relations-package.log`,
`fl-auth-relations-runtime.log`. Broader account/organization/provider/compliance
callers still need migration; checklist remains **646/682 (94.7%), 36 open**.

### Indexed membership and permission queries

Organization members/full graphs/deletion, dynamic roles, membership guards and
application role/permission surfacing now query scoped relationship indexes.
Existing insertion ordering, filter evaluation and configured limits stay intact.
Legacy non-string organization values preserve JSON equality via bounded pages.
The large-tenant fixture adds 4,100 foreign members and malformed foreign dynamic
role records: own member listing, grants, full graphs, role reads, surfacing and
delete remain correct without reading or modifying the foreign scope.

All 31 native tests, strict Clippy and restricted encrypted PostgreSQL pass.
Twelve actual TypeScript suites pass 2,615 comparisons across membership, dynamic
roles, permissions, invitations and surfacing. The rebuilt generated app passes
both SDKs, eight frontend-client scenarios, HTTP/MCP startup/invocation, restart,
source preservation and rename/delete. Logs: `/private/tmp/fl-auth-member-index-`
`tests-final.log`, `large-final.log`, `postgres.log`, `clippy-final.log`,
`runtime.log` and the twelve named comparison logs. Team/provider/verification/
compliance query migration remains open. Checklist remains **646/682 (94.7%)**.

Cleanup removed seven previously validated Better Auth generated workspaces,
151 further generated runs with successful final log receipts and committed
fixture sources, and the two newly completed generated runs. Nested Git repos,
tracked files and active workspaces were excluded. Detailed old-run receipts:
`/private/tmp/fl-validated-workspace-cleanup.json` and
`/private/tmp/fl-completed-workspaces-removed.json`.

### Indexed teams/invitations and claim-owner writes

Team reads, member selection/mutations and invitation listing/creation now use
scoped indexes. Membership-key lookup retains precedence over legacy team/user
pairs. Migration 0004 indexes unique claims by record owner for replacement,
backfill and FK deletion. PostgreSQL EXPLAIN verifies the selective owner index
on the large dataset. The shared fixture now includes 4,100 records in each of
users, accounts, organization members/roles, teams/team memberships and invitations,
plus the existing 4,100-session scenario. Native/live HTTP/MCP behavior remains
correct beyond the old model scan bound.

All 31 native tests, strict Clippy, 1,550 actual TypeScript team/invitation
comparisons and restricted encrypted PostgreSQL pass. The expanded backfill
exceeded the owned fixture's 256 MiB temporary volume; a fresh 1 GiB fixture
passes the full test. CI PostgreSQL has no corresponding 256 MiB volume limit.
The CLI packages 868 source/template files; the generated relocated app passes
both SDKs, frontend clients, HTTP/MCP startup/invocation, restart and lifecycle
checks. The generated workspace was removed after completion. Logs:
`/private/tmp/fl-auth-team-index-tests-final.log`, `fl-auth-team-index-clippy-final.log`,
`fl-auth-team-index-postgres-final.log`, `fl-auth-team-index-runtime.log`,
`fl-auth-team-index-package.log` and seven named comparison logs. Another 296 MiB
of build outputs were removed only where dependency files traced them to already
deleted, validated generated workspaces (`fl-orphan-generated-artifacts.json`).
Provider/password, verification, keyring and compliance/retention traversal
remain open. Checklist stays **646/682 (94.7%), 36 open**.


### Provider, verification and compliance collection migration

Google account resolution, password updates/revocation, reset tokens and account
compliance now use relationship indexes. OAuth state consumption and reset
callback cleanup preserve global expiration behavior using 256-row keyset pages.
Memory and restricted PostgreSQL fixtures include 4,100 foreign verification
records, account ownership, one-time state/cookie correlation, token reset,
export redaction and scoped erasure. The PostgreSQL run passed in 258.90 seconds;
all 31 portable auth tests and strict all-feature/all-target Clippy passed.
Actual account, OAuth state, Google account/callback/HTTP/token and callback
comparisons passed. The password-reset fixture exposed an existing response
mismatch: invalid reset tokens returned 401 instead of 400. The route-specific
mapping now matches all 44 actual HTTP/storage comparisons; the live HTTP/MCP
regression also verifies replay rejection. All 14 encrypted-ORM/native compliance
comparisons passed. Evidence: `/private/tmp/fl-auth-provider-index-*.log`.
Keyring and retention scans remain open; checklist stays 646/682 (94.7%).

Generated provider-query validation also passed: both SDKs, eight actual frontend
client scenarios (80 HTTP actions), HTTP/MCP shared/separate/disabled listeners,
root/custom/URL namespaces, self-JWKS, persistence, shutdown and rename/delete.
All 868 embedded sources passed. The completed owned workspace `run.VztQJh` was
removed; runtime and package evidence use the same provider-index log prefix.


### Final auth traversal migration and configured JWKS window

All production Better Auth service callers now use indexed matches or keyset
pages. Keyring signing, verification and JWKS publication read the configured
adapter's first 100 inserted keys. An actual 101-record endpoint fixture caught
and now verifies the prior JWKS publication mismatch. Operator rotation pages
through all keys; it deliberately retains upstream's public first-100 window,
so operators must prune historical rows before new keys fall outside that window.
Retention uses 256-record pages and a bounded oldest-batch selection with stable
insertion ties. Dry runs count without collecting decrypted account rows.

All 31 portable auth tests passed; the restricted encrypted PostgreSQL fixture
passed in 290.36 seconds. Both stores exercise more than 4,096 account and key
records, retained-key behavior, final-page rotation, retention ordering/zero-size
batches/dry runs/replay, tenant isolation, audit and rollback. Strict all-feature,
all-target Clippy passed. Actual TS comparisons: key lifecycle 30, retention 41,
signing 189 API + 216 storage, verification 393 API + 309 storage, JWT/JWKS 26.
The rebuilt CLI packages all 868 sources. Evidence:
`/private/tmp/fl-auth-final-scans-{tests,clippy,postgres,keyring,retention,sign_jwt,verify_jwt,jwt,package}.log`.
Global expiration/retention scans remain linear and response arrays retain memory
proportional to their matches; startup index backfill requires coordinated writers.

The final rebuilt generated preset also passed both SDKs, all eight frontend
client scenarios, HTTP/MCP startup (shared/separate/disabled), custom/root/URL
namespaces, self-JWKS, persistence, shutdown and rename/delete
(`fl-auth-final-scans-runtime.log`). Indexed auth storage gate is complete:
**647/682 (94.9%), 35 open**. Broader Better Auth custom hooks/options and dynamic
namespace semantics remain separate, open work.


### Ecommerce classified export and erasure hooks

Added tenant-bound memory/PostgreSQL hooks for the actual Order shipping-address
classification. Default order-ID subject resolution matches the TypeScript
walker's fallback; explicit customer-ID mapping matches its constructor override.
The real encrypted ORM fixture revealed that classified values serialize as
opaque objects. Rust now preserves that behavior with a non-Debug protected
address and explicit audited `deanon` access; JSON export cannot leak addresses.
Whole-order erasure leaves unclassified records untouched and reports incomplete
operations. PostgreSQL decrypt/audit/delete work commits atomically, with foreign
tenant isolation and audit-failure rollback verified. All five native ecommerce
tests, restricted PostgreSQL, strict default/all-feature Clippy and 12 actual
MikroORM/native comparisons pass. The same-realm fixture mode preserves MikroORM's
`instanceof Function` handling of real lazy enum factories; default fixture
execution is unchanged. CI includes the new comparison. Evidence:
`/private/tmp/fl-ecommerce-compliance-{tests,postgres,clippy,clippy-default,actual}.log`.
Retention remains open; checklist remains 647/682 (94.9%).


### Ecommerce explicit retention policies

The ecommerce stores now implement the framework `RetentionStore` boundary for
`OrderCompliance`. No default retention duration is imposed. Trusted applications
resolve a calendar cutoff and opt into deletion or anonymization, with tenant
binding, bounded oldest-first batches, dry-run counts and incomplete-enforcement
reporting. The configured shipping address is non-nullable: the actual TS service
preserves it during anonymization and records completion; native behavior matches.
Deletion removes the whole Order. Migration `5_order_retention.sql` stores forced-
RLS completion markers with a cascading record FK, so ordinary DTO updates cannot
lose markers and reused deleted IDs cannot inherit them. Runtime grants include
the marker table. Memory stores bind retention via `for_tenant`.

Six portable native ecommerce tests and restricted PostgreSQL pass, including
competing passes, exact cutoff exclusion, replay, marker preservation across
updates/deletion/recreation, foreign-tenant rejection and audit-failure rollback.
Strict default/all-feature/all-target Clippy passes. Eight real encrypted ORM
retention comparisons and twelve classified export/erasure comparisons pass;
fixtures load the actual ecommerce entity metadata without unrelated auth registry
entries. The rebuilt CLI packages 879 sources, and the Rust security audit reports
no unexpected findings. Evidence: `/private/tmp/fl-ecommerce-retention-*.log`.
The earlier generated ecommerce run passed HTTP/MCP and checkout but lacked the
local Stripe fixture path; a fresh complete run supplies `STRIPE_SDK_PATH`.

The complete rebuilt generated ecommerce run passes HTTP/MCP startup/discovery,
both SDKs, actual TS checkout/webhook comparisons, cross-process checkout races,
provider verification/capture, encrypted persistence, durable inventory worker,
Compose ownership, source preservation, relocation and rename/sync/delete
(`fl-ecommerce-retention-runtime.log`). Both owned generated workspaces were
removed, including the superseded run missing its local Stripe dependency path.
Ecommerce compliance gate is complete: **648/682 (95.0%), 34 open**. Interrupted
provider capture/reconciliation remains separate work.


### Relay handoff concurrency prerequisite

The Rust/mixed relay audit reproduced a real TypeScript template race: two
independent request entity managers, synchronized after reading one ticket, both
redeemed it. The template now conditionally updates the unused, unexpired ticket
in one database statement before minting a session. The real MikroORM/PostgreSQL
fixture now permits exactly one redemption and checks nonce uniqueness, replay,
unknown and expired tickets. CI watches the relay template and runs this fixture.
Evidence: `/private/tmp/fl-relay-orm-before.log` (2 redemptions instead of 1) and
`fl-relay-orm-after.log` (pass). This fixes the reference behavior; native relay
service, HTTP/MCP and generated lifecycle work remain open. Checklist stays
648/682 (95.0%), 34 open.

Relay redirect audit also reproduced browser URL normalization turning
`/\t/foreign.test`, `/\n/foreign.test` and `/\r/foreign.test` into another-origin
URLs. The TS template now rejects ASCII controls before accepting root-relative
paths, with real template/URL-parser regression coverage. Evidence:
`fl-relay-redirect-before.log` fails and `fl-relay-redirect-after.log` passes.
The native relay primitive applies the same guard.


### Rust and mixed relay completion (2026-10-13)

The Better Auth adapter now implements relay nonce claiming, five-minute one-time
handoffs, application-owned token/owner and claim hooks, one-day session cookies,
and root-relative redirect validation. Nonce claims commit before hooks and
handoff claims commit before session minting. The new encrypted auth model shares
existing RLS, audit and unique-index enforcement; migration 0005 permits it.
HTTP/MCP ingest and claim contracts use route-scoped HMAC policies: instance keys
cannot authorize unrelated routes. Browser handoffs remain outside SDK/MCP.

`init module --language rust -m relay` extends the existing Better Auth service,
including mixed applications, and creates preserved application hook source.
Generated contracts include both relay operations. Dry-run, duplicate rejection,
relocation, sync/reinit preservation, both SDK builds, live TypeScript SDK calls,
HTTP/MCP startup and invocation, rename and deletion pass. Safe scaffold hooks
redirect without login and decline claims until applications supply their policy.

Validation receipts:
- `fl-relay-full-tests.log`: 35 native tests pass, PostgreSQL test separately run.
- `fl-relay-native-postgres.log`: full restricted PostgreSQL fixture passes,
  including concurrent relay claims/redemptions and cookie authentication.
- `fl-relay-route-keys-tests.log`: 41 framework unit + 2 integration tests pass.
- `fl-relay-http-tests.log`: live HMAC, cookie login, claim-purpose ordering,
  redirects, MCP invocation and graceful shutdown pass.
- `fl-relay-comparison.log`: actual TS ORM/native default-hook, nonce uniqueness,
  synchronized concurrent redemption, replay/unknown/expired and redirect cases.
- `fl-relay-generated.log` and `fl-relay-generated-mixed.log`: complete generated
  Rust/mixed lifecycle and HTTP/MCP checks; mixed fixture includes SDK calls.
- `fl-relay-final-clippy.log` and `fl-relay-route-keys-clippy.log`: strict checks pass.
- `fl-relay-package.log`: 886 embedded source/template files packaged.
- `fl-relay-cli-tests.log`: all 1,078 CLI tests pass.

Checklist: 649/682 (95.2%), 33 open. These are verified checklist counts, not an
estimate that all remaining tasks have equal effort. Broader provider options,
versioned routing, contract/schema conformance and deployment/publication gates
remain open.


### Workspace documentation completion (2026-10-13)

Both workspaces now have indexed guides covering every member's purpose and
feature/default selection, linked public API source, setup/composition, runtime
operations, persistence/auth boundaries, provider/worker semantics, SDKs and
remaining compatibility limits. Existing runnable generation and protocol guides
are linked rather than duplicated. The blueprint README no longer describes
completed indexed-query migration as outstanding, and documents ecommerce
protected compliance export and explicit retention policy.

Strict all-feature `cargo doc --workspace --no-deps` builds pass for all 19
framework and 30 distinct blueprint crates. The first strict framework build
caught broken HMAC and OTLP intra-doc links; fully qualified links now resolve.
CI runs both strict documentation builds. Every crate is in the guide catalog,
and all 81 local guide links/anchors resolve. Both workspace format checks pass,
including module ordering left by recent additions. Evidence:
`/private/tmp/fl-framework-docs.log`, `fl-blueprint-docs.log`,
`fl-framework-format.log`, `fl-blueprint-format.log`.

Checklist: 651/682 (95.5%), 31 open. Documentation describes current verified
capabilities and the remaining gaps; it does not close their implementation or
publication/deployment gates. Six completed relay scratch workspaces were removed
following commit `01686c344`; approximately 60 GiB remained free.


### Bundled static schema resources in HTTP SDK imports (2026-10-13)

The shared CLI/native HTTP SDK compiler now indexes bundled `$id` resources,
resolves relative references against lexical bases, scopes anchors per resource,
and preserves recursive identity across URI/pointer aliases. This follows the
[JSON Schema resource/base-URI model](https://json-schema.org/draft/2020-12/draft-bhutton-json-schema-01).
Generation never fetches external schemas. Indexing shares the existing depth,
node and byte bounds and excludes literal annotations. Duplicate/malformed or
missing resource identities fail explicitly. Source packaging includes the new
shared resolver module and both workspace lockfiles record the URI dependency.

The resource-enabled recursive fixture passes both CLI artifact sources
(contract/OpenAPI), both live client languages, both native framework generators,
HTTP/MCP discovery/invocation and shutdown. It includes self-recursive resource
roots and relative references between mutually recursive resources. The original
resource-free recursive fixture also passes. Unit checks cover scoped duplicate
anchor names, URN identities, literal preservation, missing/duplicate/dynamic
resources, OpenAPI component-local pointers, and expansion bounds.

Evidence: `/private/tmp/fl-resource-generated.log`,
`fl-resource-recursive-regression.log`, `fl-resource-native-tests.log` (27 shared
compiler + 5 Rust generator + 8 TS generator tests),
`fl-resource-native-clippy.log`, `fl-resource-cli-tests.log` (1,084 pass),
`fl-resource-package.log` (889 packaged source/template checks), and
`fl-resource-format.log`. CI includes the resource-enabled generated runtime.

The broad resource-reference gate stays open: draft-4 IDs, mixed resource
dialects, resource references into literal/schema-dictionary contexts,
nonempty ID fragments, dynamic/recursive references and WebSocket resource-aware
union relocation remain explicitly unsupported. Checklist remains 651/682
(95.5%), 31 open.


Schema resource follow-up adds lexical dialect tracking, draft-4 `id`, and
embedded resources using different drafts. The model walk now selects keyword
meaning from the current schema resource, including standalone OpenAPI components,
instead of applying the document root's draft everywhere. Tests confirm legacy
`const` and conditional annotations do not become modern validation/type rules.
Dialect strings and index copies consume the shared byte budget before copying.

The full generated matrix passes static, legacy and mixed-resource variants:
both client languages, both artifact imports, both native generators, strict
Rust client lint, HTTP/MCP startup, discovery, invocation and graceful shutdown.
Evidence: `fl-resource-dialects-generated.log` (three complete passes),
`fl-resource-dialects-native-tests.log` (29 shared + 5 Rust + 8 TS tests), and
`fl-resource-dialects-clippy.log`. All 1,086 CLI tests and workspace formatting
also pass (`fl-resource-dialects-cli-tests.log`, `fl-resource-dialects-format.log`).
Dynamic/recursive references, ID fragments,
resource/literal contexts and WebSocket relocation remain open; count unchanged.


### 2026-10-13 — HTTP SDK dynamic schema scope and native anchor correctness

The shared CLI/native graph compiler now supports explicit 2020-12
`$dynamicAnchor`/`$dynamicRef` resources. Each reference model is identified by
its target and inherited dynamic bindings, so the same base resource can safely
specialize under independent string/integer recursive roots. Static references
remain static; dynamic references whose initial target is a plain anchor or
JSON pointer retain static resolution. Binding copies use the existing byte
budget. Missing/older dialect declarations and WebSocket dynamic imports fail
explicitly instead of producing models that disagree with runtime validation.

The native validator's opt-in `forklaunch-static-anchor-resolution` correction
prevents ordinary `$ref` from dynamically rebinding and isolates named-reference
cache entries by actual resolved target. Pointer aliases retain their upstream
behavior. Provenance and CI cover the feature; all 1,993 feature-enabled and
1,992 default vendor validator tests pass, along with 150 referencing tests.
The one existing archive-missing Git-submodule test remains explicitly skipped.

`rust_schema_dynamic_parity.mjs` drives the actual private graph compiler and
native validator: 21 native/original and bundled-schema checks pass, including
recursive context specialization, static-vs-dynamic targets, plain anchors and
URI/pointer fallback. AJV 8.20 agrees on 14 checks. Three recorded results expose
AJV sibling-scope leakage / plain-anchor rebinding, and four URI-reference checks
hit AJV's explicit fragment-only limitation. These are documented differences,
not reported as AJV parity. Expected behavior follows
[JSON Schema 2020-12 §8.2.3.2](https://json-schema.org/draft/2020-12/draft-bhutton-json-schema-01#section-8.2.3.2).

Broader regression: 13,908 actual TypeScript/AJV/native codec checks pass;
30 shared compiler, 5 Rust generator, 8 TypeScript generator, 18 codec and
7 transport unit/integration tests pass; all 1,087 CLI tests, strict native
clippy and framework formatting pass. CLI packaging includes all 890 expected
source/template files. Generated dynamic schemas pass both SDK languages,
both contract/OpenAPI imports, both native generators, strict Rust client lint,
and real HTTP/MCP startup, discovery, calls and graceful shutdown.
Evidence: `fl-resource-dynamic-{ajv,ws-parity,native,final-unit,cli-tests,clippy,format,package,generated,vendor-tests,upstream,referencing}.log`.

Checklist remains **651/682 (95.5%), 31 open**. The broad resource gate still
includes 2019-09 recursive references, legacy ID fragments, resource/literal
contexts and WebSocket resource-aware relocation; this batch does not close it.

The combined generated resource suite also passes all four variants (static,
legacy, mixed and dynamic), recorded in `fl-resource-dynamic-regression.log`.
Strict all-feature framework API docs pass and the security audit reports no
unexpected advisories (`fl-resource-dynamic-docs.log`, `fl-resource-dynamic-audit.log`).


### 2026-10-13 — 2019-09 recursive SDK resource roots

The shared HTTP graph compiler now supports explicit 2019-09 resource-root
`$recursiveAnchor` booleans and `$recursiveRef: "#"`. Binding state is isolated
from named 2020-12 anchors, participates in model identity and consumes the same
expansion budget. Non-anchored resources preserve static root fallback; malformed
anchors, non-root anchors, other recursive URI values and unsupported dialects
fail before generation. This follows the
[2019-09 recursive-reference profile](https://json-schema.org/draft/2019-09/draft-handrews-json-schema-02#rfc.section.8.2.4.2).

The combined conformance fixture passes 36 native-original/compiled cases:
27 independent AJV agreements, five explicitly recorded AJV scope/anchor
result differences, and four unsupported AJV URI-reference cases. New tests
cover nested resource roots, false/absent recursive anchors, strict profile
boundaries and both static/dynamic forms. All 31 shared compiler, 5 native Rust,
8 native TypeScript and 1,088 CLI tests pass, with strict native lint, formatting
and 890 packaged source/template checks. The generated recursive-resource mode
passes both languages, both artifact sources, both native generators, strict
Rust consumer lint, and live HTTP/MCP startup, invocation and shutdown.
Evidence: `fl-resource-recursive-{ajv,unit,clippy,cli-tests,format,package,generated}.log`.

Five committed dynamic fixtures and the completed recursive fixture were cleaned
up (182 generated workspaces removed cumulatively). Checklist remains
**651/682 (95.5%), 31 open**; remaining resource work includes legacy fragment
IDs and resource-aware socket unions.


### 2026-10-13 — Resource-aware socket unions and raw AsyncAPI export

Socket plans now resolve each standalone resource graph before combining message
schemas. Two messages can reuse the same `$id` and anchor names without sharing
recursive bindings. The public `bundle_schema_resources` operation is shared
with raw AsyncAPI export, which resolves resource scopes before document-pointer
relocation. Resource-free schemas retain their original annotation/pointer
layout. Both workspace locks record the AsyncAPI → SDK-codegen dependency.

`rust_ws_resources_runtime.mjs` generates strict TypeScript and Rust consumers
for static, Draft-4, 2020-12 dynamic and 2019-09 recursive unions. Both clients
complete 16 live roundtrips; malformed recursive cross-variant values and
constraint-invalid outgoing values fail. The emitted AsyncAPI document passes
the official schema with format validation. CI runs the fixture. Runtime
bundling explicitly requires one dialect per standalone message: model-only
HTTP imports can preserve mixed embedded dialects, but runtime translation of
mixed-dialect constraints remains open.

Validation: 27 AsyncAPI, 32 shared compiler, 5 Rust generator, 8 TypeScript
generator and 1,088 CLI tests pass; strict native clippy, framework formatting,
all-feature API docs, 890 packaged sources and security audit pass with no
unexpected advisories. Existing metadata checks retain 34 actual TypeScript/native
comparisons, 63 parser documents with 11 known limitations, and six dictionary
alias comparisons. Generated TypeScript regression retains both module formats,
13,908 native/TypeScript codec comparisons and full live transport controls.

A rebuilt CLI passes relocated generation/check/watch, invalid edit recovery,
staging, ownership/containment, retirement, strict consumers, actual TypeScript
socket peer roundtrips, service rename/delete, and generated HTTP/MCP startup,
discovery, invocation and shutdown before/after rename. Two stale readiness
assertions were updated to the watch banner already merged from main. The first
live-client invocation omitted the parser environment variable; validation
resumed at that step with the fixture dependency set and all remaining lifecycle
checks passed. Evidence: `fl-socket-resource-{runtime,tests,clippy,cli-tests,metadata,ts-regression,cli-runtime,cli-resume,docs,format,package,audit}.log`.

Checklist remains **651/682 (95.5%), 31 open**. The resource gate still covers
legacy ID fragments and mixed runtime/import constraints. Completed socket
fixtures are cleaned after committing the source change.


### 2026-10-13 — Legacy fragment anchors and AJV-compatible ID siblings

The shared resource index resolves Draft-4 `id` and Draft-6/7 `$id` fragment-only
anchors without replacing their enclosing resource root. Recursive aliases,
colon-bearing legacy names and identical anchor names in different resources
remain distinct. Static anchor grammar now follows the declared 2019-09 or
2020-12 dialect. Malformed names and modern fragment IDs fail explicitly.
Legacy IDs combining a URI/path and nonempty fragment remain outside this
profile; this batch implements the defined fragment-only form from
[Draft 7 §8.2.3](https://json-schema.org/draft-07/draft-handrews-json-schema-01#rfc.section.8.2.3).

An additional relative-base fixture found that the native resolver ignored IDs
beside legacy `$ref` while AJV applied them. The existing opt-in
`forklaunch-ajv-ref-siblings` feature now forwards to referencing and retains
those ID scopes. Default upstream behavior is unchanged. All six new sibling
scope cases agree with actual AJV. The combined fixture passes 57 native/original
and compiled cases: 48 independent AJV agreements, the five previously recorded
AJV scope/anchor differences and four unsupported AJV URI cases.

Validation: all eight generated resource variants (static, legacy, mixed,
dynamic, recursive, fragment4/6/7) pass both SDK languages, both artifact
formats, both native generators, strict Rust consumer lint and live HTTP/MCP
startup, discovery, calls and shutdown. Seven socket variants pass both typed
clients and 28 live roundtrips; raw AsyncAPI passes its official schema. The
broader actual TypeScript/native codec fixture still passes 13,908 cases.
All 34 shared compiler, 27 AsyncAPI, 5 Rust SDK, 8 TypeScript SDK, 18 codec,
7 transport and 1,090 CLI tests pass. Strict native lint, formatting, API docs,
890 packaged sources and security audit pass with no unexpected advisories.

Vendor validation: 151 referencing compatibility tests and all 1,992 available
default validator tests pass. The combined compatibility suite passes 1,986
checks; seven default-profile assertions explicitly require ignored legacy
siblings and are skipped only for that opt-in profile. They still run in the
default suite. Both profiles omit the previously documented test whose Git
submodule fixture is absent from the published archive. CI/provenance record
the exact exclusions; the unfiltered compatibility differences are preserved in
`fl-legacy-fragment-vendor-profile-differences.log`.
Evidence: `fl-legacy-fragment-{ajv,socket,codec,unit,final-unit,clippy,cli-tests,all-generated,referencing,vendor,vendor-default,docs,format,package,audit}.log`.

Checklist remains **651/682 (95.5%), 31 open**, with the remaining resource
boundaries now stated directly on the checklist. Eleven owned generated
fixtures from this batch are removed after commit (195 cumulative).


### 2026-10-13 — External AsyncAPI component/payload resource imports

`bundle_document_schema_resources` now extracts resource-aware schemas from
OpenAPI/AsyncAPI documents using the shared bounded graph. The resource index
recognizes standalone payloads, Multi Format schema wrappers and component
schemas. Bundled URI aliases resolve without retrieval. Resource-free documents
retain the existing contextual/literal bundler, preserving its broader legacy
annotation handling. The external AsyncAPI importer uses the new entry point.

Runtime dialect checking follows each message's reachable graph, so separate
messages can use different drafts without conflating constraints. A message
whose reference chain crosses incompatible dialects fails explicitly, including
alias roots whose `$schema` would otherwise overwrite the target dialect.
Duplicate IDs and missing remote resources also fail before output.

The expanded socket fixture compiles fourteen clients per language across
seven raw/fragment resource variants and their external-import counterparts.
Both languages pass 42 live roundtrips, strict models, constraint rejection,
official AsyncAPI schema checks, unchanged external source documents and three
negative resource-boundary cases. All five existing imported dialect clients
still pass live peers and 20 invalid-bound checks. Native tests pass: 27 AsyncAPI,
35 shared compiler, 5 Rust SDK and 8 TypeScript SDK; all 1,091 CLI tests pass.
Strict clippy, formatting, all-feature docs and 890 packaged sources pass.

New `rust_asyncapi_resources_runtime.sh` runs the relocated CLI import lifecycle
with component IDs: generation/check/watch, invalid-input recovery, strict
consumers, live peers, input ownership, rename/delete and HTTP/MCP startup,
discovery, calls and shutdown before/after rename. It passes and is included in
CI beside the resource-free variant. Evidence:
`fl-asyncapi-resources-{runtime,dialects,unit,clippy,cli-tests,generated,docs,format,package}.log`.
The committed generated fixture is cleaned (196 cumulative). Four build
artifacts proven to reference previously deleted owned fixtures were also removed.
Checklist remains **651/682 (95.5%), 31 open**; mixed runtime dialect translation
and combined URI/path-plus-fragment legacy IDs remain on the resource gate.

### 2026-10-13 — Complete schema applicator graph traversal

The shared graph now resolves references beneath schema dependencies, tuple
items, additional items, unevaluated properties/items and content schemas.
Every graph phase (indexing, alias checks, dependency analysis, expansion and
model lowering) follows those positions. Required-property dependency arrays
remain literal strings. Same-instance cycles through conditionals, negation
and schema dependencies fail explicitly; structurally guarded recursion remains
supported. This prevents dangling references after bundled definitions move.

Dialect-inactive keywords are excluded from graph/index processing and retain
literal reference locations during contract relocation. Generated TypeScript
codecs sanitize only their private compilation copy, preserving caller data and
explicit references into annotation storage. Standalone socket model compilation
now explicitly applies its Draft 7 default without changing implicit HTTP model
schemas or resource-free runtime payload annotations.

Validation: 75 original/bundled native cases, 66 independent AJV agreements,
five previously recorded AJV differences and four unsupported URI cases;
13,908 generated TypeScript/native and actual TypeScript validator comparisons;
strict ESM/CommonJS annotation regressions; 42 live roundtrips across both
resource-aware socket SDKs and external imports; 34 actual TypeScript metadata
comparisons and 63 parser documents (11 known limitations). Native affected
suites pass (20 contract, 37 shared compiler, 27 AsyncAPI, eight TypeScript SDK),
as do 1,093 CLI tests, strict clippy/docs and all 890 packaged sources.
The relocated dynamic-resource service passes HTTP/MCP startup, initialization,
discovery, invocation, Origin rejection and graceful shutdown, plus both SDKs
from contract/OpenAPI artifacts and native generators.
Evidence: `fl-schema-applicators-{unit,contract,ajv,typescript,annotations,sockets,metadata,cli,clippy,docs,package,generated}.log`.
Checklist remains **651/682 (95.5%), 31 open**: this advances the broader schema
constraint/resource gates without prematurely closing their remaining work.

### 2026-10-13 — TypeScript literal, tuple and intersection models

The shared native/CLI emitter now represents boolean false schemas as `never`,
object/array constants and enum members as literal types, legacy/2020 tuple
positions with required/optional slots and constrained tails, and `allOf` as
intersections including constraints on the containing schema. Empty object
constants reject primitive/array values. Runtime validators remain responsible
for constraints that TypeScript cannot express, including numeric bounds and
arbitrary array lengths; Rust heterogeneous arrays retain lossless JSON values.

New CI fixture `rust_schema_types_parity.mjs` compiles actual emitter output:
12 schemas, 20 accepted values and 32 statically rejected values. Each expected
result also matches native validation of the original and bundled schemas.
The existing 75-case resource fixture, 13,908 actual TypeScript/native codec
comparisons, live generated client suite, native SDK unit suites and all 1,093
CLI tests pass. Strict clippy/docs, formatting and 890 packaged sources pass.
Evidence: `fl-schema-types-{static,resources,runtime,unit,cli,clippy,docs,package}.log`.
The prior completed generated service workspace was removed (197 cumulative).
Checklist remains **651/682 (95.5%), 31 open**; broader constraint/emitter parity
still includes reference siblings and remaining model/runtime boundaries.

### 2026-10-13 — Validation siblings of schema references

The shared graph now imports validation siblings of a single static, dynamic or
recursive reference. It expresses the target as an `allOf` member while keeping
sibling constraints on the containing schema, preserving evaluated-property
annotations for `unevaluatedProperties` and existing intersection members.
Legacy dialect behavior follows the explicit ForkLaunch AJV compatibility
profile. OpenAPI Reference Objects outside schema positions retain their
separate annotation-only rules. Multiple reference applicators on the same
schema remain explicitly unsupported.

TypeScript intersections preserve the target and sibling model types. Rust
keeps a concrete referenced/typed intersection constituent when the surrounding
schema only adds validation constraints, with every constraint still enforced
by the runtime codec. Recursive typed HTTP responses remain usable.

The expanded resource fixture passes 108 original/bundled native cases and
99 independent AJV agreements, with the same five recorded AJV differences and
four unsupported URI cases. Strict TypeScript models pass 21 accepted and 35
rejected values across 13 fixtures. Sixteen generated clients per language,
including external AsyncAPI imports, pass 48 live socket roundtrips. Native SDK
and AsyncAPI suites, 1,093 CLI tests, strict clippy/docs, formatting and 890
packaged sources pass. A new `siblings` mode in the generated resource matrix
passes relocated HTTP/MCP startup, discovery, invocation, Origin rejection and
shutdown, plus typed Rust/TypeScript recursive clients from both artifact
formats and both native generators. Evidence:
`fl-schema-siblings-{parity,types,sockets,unit,models,cli,clippy,docs,package,generated}.log`.
Checklist remains **651/682 (95.5%), 31 open**; the schema gates still require
mixed runtime dialects, remaining reference profiles and emitter conformance.

### 2026-10-13 — Composed static/dynamic/recursive references

The graph evaluates every applicable reference keyword on a schema and retains
all targets in the containing intersection. Static plus 2020-12 dynamic or
2019-09 recursive references now preserve both constraints and recursive scope.
This follows the independent-applicator semantics in JSON Schema Core 2020-12
sections 7.2 and 8.2.3:
https://json-schema.org/draft/2020-12/json-schema-core#section-8.2.3.

The expanded fixture passes 120 original/bundled native cases, 107 independent
AJV default-mode agreements and 12 additional AJV allErrors-mode comparisons.
Four new recorded default-mode AJV differences arise from its generated dynamic
reference validity variable being shadowed: it skips the adjacent static
constraint, while allErrors mode and native evaluation correctly enforce both.
Together with prior cases, nine AJV behavior differences and four unsupported
URI cases remain explicit. Bundled generated schemas enforce both constraints
with either AJV mode.

Twenty generated clients per language, including external imports, pass 60 live
socket roundtrips and reject payloads that violate only the static sibling.
The generated `composed` HTTP service mode passes typed recursive clients through
both artifact formats/native generators plus HTTP/MCP startup, discovery,
invocation, Origin rejection and graceful shutdown. Native suites, 1,093 CLI
tests, strict clippy/docs, formatting and 890 packaged sources pass. Evidence:
`fl-schema-composed-{parity,sockets,unit,cli,clippy,docs,package,generated}.log`.
The first generated attempt exposed a fixture assertion that assumed one
reference keyword; the assertion now checks both and the complete rerun passes.
Checklist remains **651/682 (95.5%), 31 open**. Multiple-reference composition is
no longer a remaining schema-profile boundary.

### 2026-10-13 — Legacy URI/path plus named-fragment IDs

Draft-4/6/7 IDs such as `nodes#Word` now resolve as exact offline aliases in the
shared graph and native codec. Distinct aliases on one path retain independent
cached targets. Recursive aliases and nested fragment-only IDs work through
borrowed and owned registries. An alias does not invent the bare `nodes`
resource or a JSON-pointer base; those missing targets still fail explicitly.
Modern nonempty ID fragments and duplicate aliases remain rejected.

Native support uses the opt-in `forklaunch-legacy-fragment-ids` vendor feature,
forwarded through jsonschema and enabled by the codec. Registry retrieval skips
already indexed exact aliases, and validator cache identity preserves their full
URIs. Upstream defaults remain unchanged. CI and both vendor provenance files
record the feature and dedicated regressions.

Validation: 144 original/bundled native cases, 131 independent default-mode AJV
agreements, the same nine recorded AJV behavior differences/four unsupported URI
cases, and 12 additional allErrors-mode comparisons; 152 resolver compatibility
tests, 1,987 validator compatibility tests, 1,992 default validator tests and the
isolated legacy-feature regression. Async retrieval feature compilation passes.
The generated matrix covers 26 clients per language and 78 live socket
roundtrips, including external AsyncAPI imports. All three new generated HTTP
modes (`uri4`, `uri6`, `uri7`) pass typed recursive SDKs from both artifact
formats/native generators plus HTTP/MCP startup, discovery, invocation, Origin
rejection and shutdown. The 13,908 actual TypeScript/native codec comparisons,
native affected suites (27 AsyncAPI, 38 shared compiler, five Rust SDK, eight
TypeScript SDK, 18 codecs and seven transports), 1,094 CLI tests, strict
clippy/docs, formatting and 890 packaged sources pass.
Evidence: `fl-schema-uri-fragment-{parity,referencing,vendor,default,isolated,async,sockets,typescript,unit,cli,clippy,docs,package,generated-uri4,generated-uri6,generated-uri7}.log`.

Checklist remains **651/682 (95.5%), 31 open**. The resource gate now identifies
mixed runtime dialects and AJV-permissive legacy fragment spellings as remaining
work: an additional direct AJV probe accepts pointer-shaped, digit-leading and
underscore-leading legacy IDs that the shared compiler still rejects. This
receipt covers named fragments within the existing profile; it does not claim
those broader spellings are implemented.

### 2026-10-13 — Legacy fragment spellings and URI normalization

Legacy IDs now support pointer-shaped fragments, digit/underscore names and
percent-encoded names through the shared compiler/native codec. Exact registered
IDs take precedence over document pointers. URI normalization decodes unreserved
characters and normalizes escape case across paths, queries and fragments while
preserving encoded delimiters: `#%2Falias` and `#/alias` remain separate targets.
Normalized duplicate aliases fail explicitly. Encoded-slash references without
an explicit matching alias still obey the checked JSON-pointer boundary.

Registry object analysis now applies the existing AJV sibling-ID policy during
indexing as well as ID extraction/compilation, so a legacy ID beside `$ref` can
be referenced elsewhere. Both borrowed and owned registry paths are covered.
The opt-in vendor features retain default upstream behavior; malformed URI
syntax is outside this compatibility profile.

Validation: 240 original/bundled native cases, 227 default-mode AJV agreements,
the same nine recorded AJV differences/four unsupported URI cases, and 12
additional allErrors-mode comparisons. The expanded matrix compiles 32 clients
per language and passes 96 live socket roundtrips, including external imports.
Three generated `pointer4`/`pointer6`/`pointer7` service variants pass HTTP/MCP
startup, discovery, invocation, Origin rejection and shutdown, typed recursive
SDKs through both artifact formats and both native generators. All 13,908 actual
TypeScript/native codec comparisons and the generated live client suite pass.
Resolver suites pass 153 compatibility/150 default tests; validator suites pass
1,987 compatibility/1,992 default tests. Affected native suites, all 1,095 CLI
tests, strict clippy/docs, formatting and 890 packaged sources pass. Evidence:
`fl-schema-permissive-{parity,sockets,typescript,referencing,referencing-default,vendor,default,unit,cli,clippy,docs,package,generated-pointer4,generated-pointer6,generated-pointer7}.log`.
Checklist remains **651/682 (95.5%), 31 open**. Mixed-dialect runtime translation
is the remaining identified resource-import boundary.

### 2026-10-13 — Mixed runtime resource drafts and socket unions

The runtime graph now collects reachable dialects and translates mixed Draft
4/6/7/2019-09/2020-12 resources to 2020-12. References bind against the original
resource index before conversion, preserving tuple-pointer identities, lexical
IDs and dynamic/recursive bindings. Draft 4 boolean exclusive bounds become
numeric bounds; legacy tuple items and additionalItems become prefixItems and
items. Existing inactive-keyword filtering preserves older draft behavior.
Only reachable resources trigger conversion, unsupported reachable dialects
fail explicitly, and caller-owned schemas remain unchanged. Message unions
also convert differing standalone drafts before relocating their references.

The new CI fixture checks 201 original/translated native cases, including all
20 ordered draft pairs, recursion, bounds, tuple tails, property dependencies,
inactive annotations and evaluated locations. Independent per-resource AJV
engines verify original constraints; AJV 2020 agrees with 198 translated cases.
Three cases explicitly record AJV's pre-existing behavior of marking every
array item evaluated after contains, versus native matching-index tracking.
This difference is reproducible without translation and remains part of the
separate schema-constraint/emitter gate; resource support does not imply all
validator implementations agree.

The expanded socket fixture compiles 36 clients per language and passes 108
live roundtrips, including mixed resources, mixed message unions and external
AsyncAPI imports. The generated mixed-schema service passes both artifact
formats, both native SDK generators, typed Rust/TypeScript consumers and live
HTTP/MCP startup, discovery, invocation, Origin rejection and shutdown.
The existing 240 resource cases and 12 composed-reference allErrors checks,
13 TypeScript model fixtures (21 accepted/35 rejected values), 13,908 actual
TypeScript/native codec comparisons, 40 shared compiler/5 Rust SDK/8 TypeScript
SDK unit tests, 1,096 CLI tests, strict clippy and 890 packaged sources pass.
Evidence: `fl-schema-mixed-{sockets,http,types,codecs,units,cli-final,clippy-final,package}.log`.

The resource-import gate is complete for the documented offline profiles.
Checklist: **652/682 (95.6%), 30 open**. Broader validation/emitter, provider,
module, deployment and publishing gates remain open.

### 2026-10-13 — Ordered HTTP version selection and MCP projections

Rust version declarations now retain insertion order and JavaScript's canonical
integer-key enumeration order. `RouteContract::for_version` projects request
and response metadata while preserving common params, path and auth settings.
Local and live Axum routers compile reusable version plans at construction,
choose the last matching request and validate the selected response status.
Handlers receive the selected version and every matching name. Embedded JSON
schemas use the bounded/offline validator moved from the socket codec into the
optional contract schema-validation module; socket diagnostics remain compatible.
The metadata-only Axum adapter also selects versions and remains compatible with
callers mutating its public application registry.

MCP exports one tool per version with independent input/output schemas and
version-qualified names for multiple declarations. Transport bindings project
versions alongside tool definitions, preserving route authentication and regular
HTTP matching. A call through an earlier tool can select a later overlapping
HTTP version, as in TypeScript.

Validation: 35 actual TypeScript-parser/native selection cases cover overlaps,
rejection, declaration order and canonical/noncanonical integer keys. A live
loopback service passes HTTP requests, MCP initialization, discovery and both
versioned calls, then graceful shutdown. All 134 affected native library tests
(55 Axum, 20 contract, 22 HTTP, 19 MCP, 18 socket), 1,096 CLI tests and strict
clippy pass; the blueprint sample compiles. Evidence:
`fl-version-{actual-parity,live-tests,units,cli,clippy,blueprint}.log`.
All 13,908 generated/actual TypeScript/native codec comparisons and the generated
live TypeScript socket suite pass after sharing the validator. Strict docs and
894 embedded package sources pass (`fl-version-{codecs,docs,package}.log`).

Checklist stays **652/682 (95.6%), 30 open**. This is the runtime/MCP portion of
versioning; OpenAPI export, SDK projections and non-JSON media/coercion coverage
still prevent closing the versioned-API gate.

### 2026-10-13 — Versioned OpenAPI exports and typed SDK methods

Standard OpenAPI operations now project the last declared version and retain all
request/response contracts in `x-forklaunch-version-contracts`, alongside the
existing version-name list. `generate_versioned_openapi_json` exports independent
named documents with unversioned routes under the empty key. Components and
security metadata are generated per document, so repeated schema names across
versions retain their own meanings.

CLI imports preserve full version metadata from standard OpenAPI and import all
named documents with reference resolution isolated to each document. Contract
and OpenAPI imports share bounded version expansion with native SDK method
naming: `write` plus `v1`/`v2` becomes `writeVersionV1`/`writeVersionV2` and the
corresponding Rust snake-case methods. Every method retains its request schema,
response schema/status and common params. Colliding normalized aliases fail
explicitly, following the existing generated-identifier profile. Shared metadata
clones obey the graph byte/node budget; a repeated-large-params regression proves
version expansion cannot bypass the cap. Native projections also avoid cloning
all sibling versions for every projected route.

A fresh relocated generated service passes typed Rust and strict TypeScript
consumers through contract, standard OpenAPI and named OpenAPI imports, followed
by both native generators. Each of the four passes starts HTTP/MCP, initializes
and discovers versioned tools, invokes both versions, rejects a mismatched body,
checks distinct tool schemas and shuts down gracefully. The TypeScript consumers
also reject wrong-version body types at compile time. The E2E workflow includes
this fixture. Earlier fixture-only method-name mismatches were corrected before
the complete fresh run.

Validation: 1,099 CLI tests; 20 contract/22 HTTP/7 OpenAPI/43 shared compiler tests;
strict clippy/docs, formatting, and 898 embedded package sources. Evidence:
`fl-version-generated-complete.log`,
`fl-version-export-{cli-final,units,clippy-final,docs,package-final}.log`.
Checklist remains **652/682 (95.6%), 30 open**. Non-JSON media and
request-transformation conformance remain on the versioned-API gate.

### 2026-10-13 — Versioned text bodies and shared parser registration

Versioned routes now derive their wire parser from the first declared version,
including an absent first body, instead of the unversioned base contract. Local
and metadata adapters normalize empty text and preserve binary bytes before
selection. Registration rejects incompatible parser kinds; file/binary share a
parser, and JSON/unknown share a parser. Encoded schemas across a route's versions
have a combined 32 MiB construction budget before JSON decoding.

Validation: 40 actual TypeScript request-parser/native selection comparisons and
six actual TypeScript registration/native parser-compatibility checks; 56 Axum
and 23 HTTP unit tests; strict all-feature/all-target Clippy for both crates; and
898 embedded package sources. A fresh relocated generated service passed empty,
short and Unicode text HTTP/MCP calls, JSON version calls, startup/discovery,
rejection and graceful shutdown. Both typed SDKs passed all three artifact
sources (contract, standard OpenAPI, named OpenAPI) and both native generators.
The initial generated probe assumed a tool name rather than using discovery;
the corrected complete run is recorded in
`/private/tmp/fl-version-media-generated-final.log` (`run.5wqn6V`). Other receipts:
`fl-version-media-actual.log`, `fl-version-media-units.log`,
`fl-version-media-clippy.log`, and `fl-version-media-package.log`.

Checklist stays **652/682 (95.6%), 30 open**. Binary/form/multipart schema matching,
full middleware coercion/transform behavior and the broader versioning gate
remain open. These parser tests compare actual TypeScript request validation;
they do not claim all Express wire-parser behavior is identical.

### 2026-10-13 — Versioned opaque files and native byte constraints

Version matching no longer decodes file/binary bytes as UTF-8. The standard
opaque OpenAPI binary-string shape (with descriptive annotations) accepts and
preserves every byte. Native array and boolean schemas validate the byte-array
representation. Unsupported assertions on opaque string schemas fail at route
construction instead of silently losing their constraints. JSON/unknown parser
aliases also consistently use JSON during matching.

Validation: 45 actual TypeScript/native request-selection comparisons (including
five actual file-validator Buffer inputs) and six parser registration checks;
57 Axum and 24 HTTP tests; strict Clippy; and 898 embedded package sources.
A fresh relocated generated service (`run.ZkUoe6`) passed raw HTTP file calls
with empty/non-UTF-8/Unicode bytes and MCP file calls with representable strings,
plus JSON/text calls, both typed SDKs through all three artifact sources and both
native generators, startup/discovery and graceful shutdown. Receipts:
`/private/tmp/fl-version-file-{actual,units,clippy,generated,package}.log`.

Checklist remains **652/682 (95.6%), 30 open**. This closes the opaque file
matching defect, not all media semantics: arbitrary transformations, the separate
TypeScript base64-string binary validator, multipart/form matching and full
wire-parser conformance remain tracked. MCP string-file calls do not establish
arbitrary binary MCP transport parity.

### 2026-10-13 — Versioned flat URL-encoded forms and shared body decoding

Version selection parses URL-encoded flat fields and repeated string arrays into
schema values, preserving original handler bytes. It follows qs repeated-key
grouping, canonical integer-key ordering and malformed percent/UTF-8 escape
fallback. Bracket-array names are supported; nested/indexed paths are rejected
explicitly. Parsing has a 1,000-parameter bound. JSON/text/byte-array body decoding
now happens once per request, shared by all candidate versions.

Validation: 29 comparisons against the repository's actual Express content
middleware, including raw-body capture, repeated keys, bracket arrays, Unicode,
malformed escapes and prototype names; the existing 45 request-selector and six
registration comparisons; 57 Axum/25 HTTP tests; strict Clippy; 899 embedded
package files. Generated `run.l5FVJF` passed HTTP and MCP form version calls,
invalid-form rejection, all previous media cases, both typed SDKs through three
artifact sources and both native generators, startup and graceful shutdown.
The wire-parser fixture is included in Rust parity CI. Receipts:
`/private/tmp/fl-version-form-{actual-final,regression,units,clippy,generated,package}.log`.

Checklist remains **652/682 (95.6%), 30 open**. Nested/indexed forms, multipart
version selection, configurable parser options and broader transformations are
still incomplete. MCP form arrays retain TypeScript's existing JSON-composite
serialization; this batch does not change that transport contract.

### 2026-10-13 — Nested URL-encoded version selection

The form matcher now handles nested bracket paths, sparse indexed arrays,
array/object/scalar collisions, array overflow objects, balanced and unterminated
bracket names, prototype keys and equals signs inside bracket keys. Arrays compact
only after merging, matching qs. Input depth is bounded at 32 bracket levels and
merged output at 64 levels; the 1,000-parameter limit remains. Numeric indices use
the exactly representable JavaScript integer range.

Validation: **1,060 actual TypeScript content-middleware/native comparisons**,
including every triple from ten collision/overflow/prototype fragments; 57 Axum
and 25 HTTP tests; strict Clippy; and 900 embedded package sources. Generated
`run.cjSiEE` adds a third form version requiring a nested array and verifies both
bracket-array and sparse-index HTTP inputs select it while preserving wire bytes.
The full generated HTTP/MCP matrix, both typed SDKs through three artifact forms,
both native generators, startup/discovery and graceful shutdown pass. Receipts:
`/private/tmp/fl-version-nested-{actual-final,units,clippy,generated,package}.log`.

Checklist remains **652/682 (95.6%), 30 open**. Nested forms no longer block the
standard UTF-8 selection profile. Multipart matching, configurable parser options,
non-UTF-8 charsets, parser-limit error-status equivalence and arbitrary request
transformations remain incomplete; this is not a claim of every Express option.

### 2026-10-13 — Buffered multipart version matching and generated SDK lint fix

Version matching now parses buffered multipart text fields and native byte-array
file fields once per request. The matcher shares `MultipartLimits` with the
existing typed Axum extractor and honors extension overrides; overwritten parts
count toward all limits. Parsing polls immediately ready memory without entering
an executor or waiting for I/O. Handler wire bytes are preserved. HTTP now depends
on the same multer parser already used by Axum; both workspace locks are updated.

Validation: seven actual TypeScript multipart parser/native version-selection
comparisons and the existing seven typed-extractor comparisons; 58 Axum and 26
HTTP tests (including malformed bodies and custom limits); five Rust SDK tests;
strict Clippy and blueprint dependency checks; 901 embedded package files. The
new generated multipart routes exposed nested-condition lint errors in the
shared Rust SDK template's empty-form delimiter handling. The template now uses
an Option filter, retaining compatibility without an edition-specific let chain.
Fresh generated `run.0xntgf` passes multipart HTTP/MCP calls, raw attachment byte
preservation, the full prior media matrix, both SDKs through three artifact forms
and both native generators, startup/discovery and graceful shutdown. Receipts:
`/private/tmp/fl-version-multipart-{actual-final,units,clippy,blueprint,sdk-tests,generated-final,package-final}.log`.

Checklist remains **652/682 (95.6%), 30 open**. Native multipart byte-array schema
matching is complete for this profile; nested OpenAPI opaque-file annotations,
parser-limit error-status equivalence, charset/configurable parser behavior and
arbitrary transformations remain open. Mixed text/file duplicate-name ordering
retains the already documented nonportable TypeScript behavior.

### 2026-10-13 — Parser failure statuses and disabled-validation behavior

Body parsing now distinguishes malformed input (400), form/multipart limits
(413), and invalid multipart configuration (500). Failures remain fatal when
schema validation is None/Warning and when a body has only a named schema.
Local, metadata and live Axum adapters preserve these distinctions. Streamed
Axum body-limit errors are identified through their typed error chain and return
413; unrelated read errors remain 400 and do not expose I/O details. Public error
classification adds body parser/size/configuration locations and PayloadTooLarge.
Both workspace locks include Axum's direct http-body-util dependency.

Validation: **1,062 actual TypeScript form-parser/native comparisons**, including
parameter-limit 413 and nesting-limit 400; 59 Axum and 27 HTTP tests; strict
all-feature/all-target Clippy; blueprint checks; and 901 embedded package files.
Fresh generated `run.ctLYC7` verifies error statuses with the full HTTP/MCP media
matrix and both SDKs through three artifact forms/native generators, startup,
discovery and graceful shutdown. Receipts:
`/private/tmp/fl-version-parser-status-{actual,units,clippy,blueprint,generated,package}.log`.

Checklist remains **652/682 (95.6%), 30 open**. The standard parser-limit status
gap is resolved. The versioning gate still includes nested opaque-file schema
annotations, configurable parser/charset behavior and request transformations;
this receipt does not claim every TypeScript error payload or middleware hook.

### 2026-10-13 — Multipart opaque-file annotations and legacy File artifacts

Unscoped multipart file schemas now validate the parser's byte-array or text
values, matching the actual TypeScript file validator. Lowering reuses the
bounded contract bundler and schema-location walker, including references stored
inside literal metadata; exported schemas and const/default/example data remain
unchanged. It recognizes the actual legacy File artifact's exact instance-guard
oneOf marker. Other string assertions and differing oneOf shapes are rejected
instead of discarded. Resource-scoped schemas retain their previous validator
path; opaque-file lowering in that profile remains open.

Validation: **127 actual TypeScript multipart parser/file-validator/native
comparisons**, covering five drafts, direct properties, definition references,
literal-metadata references and text-file inputs; seven existing typed-extractor
comparisons; 59 Axum and 29 HTTP tests; strict Clippy; and 902 embedded package
files. Generated `run.Paq2su` now declares its attachment as an opaque binary file
and passes the full live HTTP/MCP media matrix, raw binary attachment preservation,
both SDKs through three artifact forms and native generators, startup/discovery
and graceful shutdown. Receipts:
`/private/tmp/fl-version-multipart-schema-{actual,units,clippy,generated,package}.log`.

Checklist is still **652/682 (95.6%), 30 open**, verified from the checklist file.
Remaining versioning work is narrowed to scoped opaque-file schemas, configurable
parser/charset behavior and request transformations.


### 2026-10-13 — Resource-scoped multipart file assertions

Multipart opaque-file assertions now compile at their original resource scope,
with IDs, anchors, literal-metadata references and mixed resource drafts intact.
The opt-in validator hook preserves the source registry and exported schemas;
evaluated-item/property tracking sees the adapted assertions. Default JSON and
socket validation remain unchanged. Rewritten nodes stay owned through compile
cache teardown, and source-only optimizations are disabled for this profile.

Validation: **271 actual TypeScript parser/file-validator/native comparisons**
across five drafts and scoped/unscoped reference layouts, plus seven typed
multipart extractor comparisons; 24 contract, 27 HTTP and 59 Axum unit tests;
strict Clippy; 1,995 default and 1,990 compatibility vendor tests; 13,908 actual
TypeScript/AJV/native codec comparisons; 201 mixed-draft cases (198 AJV agreements,
three recorded existing contains/unevaluatedItems differences); 902 package files.
Generated `run.8VOTWn` uses a resource-scoped anchored attachment and passes HTTP
and MCP startup, discovery, calls, raw byte preservation, both typed SDKs through
all three artifacts and native generators, and graceful shutdown. Receipts:
`/private/tmp/fl-scoped-{actual,native,clippy,vendor-default,vendor-compat,codec,mixed,generated,package}.log`.

Checklist remains **652/682 (95.6%), 30 open**. The scoped-file gap is closed;
configurable parsers/charsets and request transformations still keep the broader
versioning gate open.


### 2026-10-13 — Default JSON/text wire parsing in version selection

Version matching now uses the TypeScript defaults for strict JSON roots,
empty JSON bodies, one leading UTF-8 BOM and malformed UTF-8 replacement. Parser
failures remain fatal for unconstrained schemas. Wire bytes are preserved;
standard Axum String/Json extractors still use their own decoding rules, so this
receipt does not claim parsed-value handler delivery or arbitrary transformations.

Validation: 23 actual TypeScript content-middleware/native wire comparisons,
45 existing version cases and six registration checks; 59 Axum and 28 HTTP tests;
strict Clippy; 902 package files. Fresh generated `run.Ux0dcU` verifies BOM-aware
selection with intact handler bytes, the full HTTP/MCP startup/call/shutdown
matrix, and both SDKs through three artifact forms and native generators. The
new comparison runs in CI. Receipts:
`/private/tmp/fl-wire-{actual,versions,native,clippy,generated,package}.log`.
Checklist remains **652/682 (95.6%), 30 open**; parser configuration/charsets and
request transformation delivery remain open.


### 2026-10-13 — Parsed version bodies reach typed handlers

`VersionedBody<T>` deserializes the value already parsed and validated during
version selection, without consuming or rewriting request bytes. Local and
metadata handlers receive the shared value on `SelectedVersion.parsed_body`.
The live adapter keeps selection metadata on both framework requests and Axum
extensions; empty JSON parsed as `{}` no longer fails a later missing-body check.
The extractor supports JSON, text, file byte arrays, nested forms and multipart.
Missing selection/body metadata and incompatible handler types reject explicitly
without exposing payloads in errors. Custom schema transforms and unversioned
parsed-body delivery remain outside this change.

Validation: 61 Axum and 29 HTTP tests, including every parser with simultaneous
parsed/raw access and extractor rejection cases; strict Clippy; 23 actual wire
comparisons; 1,062 actual URL-encoded comparisons; 903 package files. Fresh
`run.qzu84p` verifies BOM-prefixed JSON and malformed UTF-8 text through generated
typed handlers, HTTP/MCP startup/discovery/calls/shutdown, and both SDKs through
three artifacts and native generators. Receipts:
`/private/tmp/fl-parsed-{native,clippy,actual,form,generated,package}.log`.
Checklist remains **652/682 (95.6%), 30 open**; configurable parser/charset behavior
and arbitrary request transformations remain on the versioning gate.


### 2026-10-13 — Request charset decoding and unsupported-encoding status

Versioned JSON/text/form matching and parsed handler values honor Content-Type
charsets. JSON requires `utf-`; forms accept UTF-8 and ISO-8859-1 with Latin-1
percent decoding; text supports Unicode endian/autodetection variants and the
encoding_rs legacy tables. Unknown/disallowed encodings return 415 as fatal parser
errors. Original wire bytes remain intact. Multibyte decoding retries the byte
after a failed lead; UTF-32 trailing bytes produce the TypeScript replacement
character. UTF-8 forms now follow body-parser replacement/BOM behavior before qs
percent decoding. Both workspace locks include the direct decoder/MIME edges.

Validation: **128 actual TypeScript content-middleware/native parsed-value and
status comparisons**, including UTF-8/16/32, endian detection, quoted charset
labels, Latin-1, ASCII, Windows-1252, Shift-JIS, GB18030, Big5 and KOI8-R; 1,062
existing form comparisons; 61 Axum and 29 HTTP tests; strict Clippy; blueprint
check; 904 package files. Fresh `run.SUIJG0` passes live decoded UTF-16 JSON/text,
Latin-1 text/forms, 415 rejection and the full HTTP/MCP/SDK matrix across three
artifacts and native generators. Receipts:
`/private/tmp/fl-charset-{actual,form,native,clippy,blueprint,generated,package}.log`.

Checklist remains **652/682 (95.6%), 30 open**. Every iconv alias/table, unpaired
surrogate behavior, custom charset defaults, form charset-sentinel options and
arbitrary transformations are not claimed complete.


### 2026-10-13 — Configurable versioned JSON/text/form parsers

`RouteOptions.body_parser` now configures JSON strict mode, JSON/text/form default
charsets, form extended mode, depth and parameter limits across every version.
Explicit Content-Type charsets override defaults. Flat parsing retains bracketed
keys; configured depth overflow is 400 and parameter overflow is 413. Invalid
default charsets and unsupported limits fail registration (depth <=32, parameters
1..=100,000). Non-default settings are exported in contract artifacts; defaults
preserve the prior shape.

Validation: **170 actual TypeScript content-middleware/native value/status
comparisons**, 1,062 existing form comparisons, 24 contract/30 HTTP/61 Axum unit
tests, strict Clippy and 905 embedded package files. Fresh generated `run.dbD25l`
includes a non-strict scalar JSON route, verifies custom/default artifact metadata,
HTTP and MCP scalar calls, every previous media/charset check, both SDKs through
three artifact formats and native generators, startup/discovery and shutdown.
Receipts: `/private/tmp/fl-parser-options-{actual,form,native,clippy,generated,package}.log`.

Checklist remains **652/682 (95.6%), 30 open**. Compression, per-parser byte limits,
verify/reviver hooks, form sentinel/entity options, complete legacy codec coverage
and arbitrary schema transformations remain open; this configuration currently
applies to the versioned pipeline.


### 2026-10-13 — Bounded compression and per-parser byte limits

Versioned JSON/text/form/raw parsers now configure byte limits and inflation
independently. Defaults match body-parser's 100 KiB decoded limit with inflation
enabled. Identity, gzip (including concatenated members), zlib deflate and Brotli
are decoded with at most limit+1 output bytes. Unknown/disabled encodings return
415, malformed streams return 400 and decoded-limit failures return 413. Runtime
configuration caps limits at 32 MiB; multipart keeps its own limits and transport
limits still bound wire bytes. Raw request bytes remain original, while selected
parsed values contain decompressed data (TS verify buffers are decompressed).

Validation: **267 actual TypeScript parser/native value/status comparisons**,
24 contract/30 HTTP/61 Axum tests, strict Clippy, blueprint check, no new unreviewed
advisories across all three locks, and 906 packaged files. Fresh `run.ttVFp4`
passes compressed JSON/text/file HTTP calls for every encoding, expansion-limit
rejection, configured text limits and disabled scalar inflation, all HTTP/MCP
startup/discovery/call/shutdown checks, and both SDKs through three artifact forms
and native generators. Receipts:
`/private/tmp/fl-wire-limits-{actual,native,config,clippy,blueprint,audit,generated,package}.log`.

Checklist remains **652/682 (95.6%), 30 open**. Custom verify/reviver hooks, form
sentinel/entity options, complete legacy codec coverage and schema transformations
remain on the broader parser/versioning work.


### 2026-10-13 — Form charset sentinels and numeric entities

Versioned form options now support charsetSentinel and interpretNumericEntities.
The first literal `utf8=` part is removed and, for a recognized sentinel, selects
UTF-8 or Latin-1 percent decoding. It still counts against parameter limits.
Numeric entities affect Latin-1 values only, preserving keys, Number rounding,
UTF-16 code-unit wrapping and paired surrogates. Both options default to false;
unpaired-surrogate representation remains the previously documented limitation.

Validation: **404 actual TypeScript parser/native comparisons**, including both
sentinel orders, unknown/lowercase sentinels, all option combinations, repeats,
large/overflowing entities, pairs, and sentinel-inclusive limits; 24 contract,
30 HTTP and 61 Axum tests; strict Clippy; 906 package files. Fresh `run.TaomOs`
proves sentinel/entity decoding changes the selected HTTP version while preserving
wire data, and passes all HTTP/MCP/media/compression checks, both SDKs across three
artifacts and native generators, startup/discovery and graceful shutdown.
Receipts: `/private/tmp/fl-form-options-{actual,native,clippy,generated,package}.log`.
Checklist remains **652/682 (95.6%), 30 open**. Verify/reviver hooks, schema/request
transformations and complete legacy codec/invalid-string coverage remain open.

### Native per-version body hooks and adapter ownership (2026-10-13)

Native callbacks now preprocess each version's original parsed input before schema
checks and transform successful candidates afterward. Rejections are isolated to
the candidate; a later failed transform preserves the earlier successful version.
Direct/local/metadata adapters expose the selected transformed value and preserve
raw bytes. Nested live routers retain hooks. Local handlers served through Axum
own selection/response validation, so callbacks run once; transport authentication
and byte limits still run before local dispatch. Live/metadata registries reject
missing or ambiguous method/operation registrations.

Validation: 45 actual TypeScript version comparisons, six parser registration
checks and **18 actual Zod preprocessing/transformation comparisons**; 404 parser
wire comparisons; 24 contract, 31 HTTP and 63 Axum tests plus three integration
tests; strict Clippy; 907 package files. Fresh `run.iFWY4d` passes generated HTTP
and MCP transformation/fallback/rejection, both typed SDK transformation calls
across contract/OpenAPI/named OpenAPI and native generators, startup/discovery,
Origin rejection and graceful shutdown. The multi-route native exporter fixture
now preserves distinct schema names rather than accidentally sharing components.
Receipts: `/private/tmp/fl-body-hooks-{actual,wire,native,clippy,package}.log` and
`/private/tmp/fl-body-hooks-generated-final.log`.

Runtime callbacks are explicit native code, not serialized JavaScript. Parser
verify/reviver, missing-body defaults, non-body transformations and the documented
codec/string representation boundaries remain open. Checklist **652/682 (95.6%),
30 open**; this completes a portion of the broad versioning gate.

### Shared body-parser verification callbacks (2026-10-13)

`BodyParserHooks::with_verify` and `insert_parser` provide native verification of
bounded, decompressed bytes before character decoding/parsing. Callbacks receive
request metadata and the charset (absent for raw/file). They run once across all
versions, including empty bodies; size/encoding errors take precedence. Failure
returns a fixed 403 under Error/Warning/None validation modes without leaking
callback details. Multipart/SSE registrations are rejected. Runtime hooks remain
native/read-only and are not serialized as JavaScript functions.

Validation: **484 actual TypeScript/native parser comparisons** (80 new verify
cases), including all four parsers, gzip/deflate/Brotli, UTF-16, empty bodies,
strict/invalid JSON and bounded-byte precedence. 24 contract, 31 HTTP, 64 Axum
and three integration tests pass, including once-only direct/local adapter calls
under every validation mode. Strict Clippy and 908 package files pass. Fresh
`run.9eT6Uy` passes generated HTTP/MCP verification, compressed success/rejection,
private-error omission, both typed SDKs across three artifact forms/native
generators, startup/discovery/Origin rejection and graceful shutdown.
Receipts: `/private/tmp/fl-parser-hooks-{actual,native,clippy,package,generated}.log`.
Checklist remains **652/682 (95.6%), 30 open**. JSON reviver and non-body/missing-body
transformation semantics remain in the broad versioning gate.

### Native JSON reviver traversal (2026-10-13)

`BodyParserHooks::with_json_reviver` applies child-first transformations once,
before version-specific callbacks. It preserves JavaScript numeric-key ordering,
other source-key order and duplicate-key last-value/original-position behavior
through an ordered parser, without changing global serde_json map features.
Object properties and the root can be removed; empty decoded bodies bypass the
reviver. Callback failures return a fixed 400. Traversal is depth-bounded.

Validation: **513 actual TypeScript/native parser comparisons**, including 29 new
reviver fixtures with ordered callback effects, duplicate keys, nested arrays,
property removal, compression, primitive roots, empty/BOM input and errors.
24 contract, 34 HTTP, 64 Axum and three integration tests pass, plus strict
Clippy, blueprint-core compilation and 909 source-package files. Fresh `run.NtsKUh`
passes HTTP/MCP reviver transformation/rejection, private-error omission, all prior
media/parser/verification checks and both SDKs across three artifact forms and
native generators, including service startup/discovery and graceful shutdown.
Receipts: `/private/tmp/fl-reviver-{actual,native,clippy,blueprint,package,generated}.log`.

Sparse array deletion explicitly returns 400: native JSON values cannot represent
JavaScript holes. Mutable reviver holders/source context, broader undefined/default
validation and non-body transformations remain open. Root deletion is an absent
parsed value and requires a compatible contract/handler; the typed required-body
extractor requires a present value. Checklist **652/682 (95.6%), 30 open**.


### Mixed-app live TypeScript SDK imports (2026-10-13)

Live SDK mode now accepts a mixed workspace containing registered TypeScript
projects, removes only their declaration entry points, and leaves native package
metadata and generated SDKs intact. Generated mode restores declaration resolution
without parsing Tokio as a JavaScript runtime. Shared root tsconfig/editor tasks
remain application-owned in hybrid workspaces. Pure Rust live mode fails before
mutation. Legacy TypeScript-only Node/Bun behavior is covered by a regression test.

Fresh relocated `rust-mixed-sdk-runtime/run.77wpkisb` proves strict TypeScript
package imports observe source edits immediately, generated/live round trips,
dry-run preservation, unchanged native artifacts and shared configuration. The
same fixture passes SDK watch recovery, service lifecycle preservation, native
HTTP/MCP startup/endpoint modes/Origin rejection/graceful shutdown, and both SDKs
calling real native and tsx HTTP services. All **1,100 CLI tests pass**. Receipts:
`/private/tmp/fl-live-sdk-{generated,tests,cli-all}.log`.

This closes the direct live TypeScript import gate: **653/682 (95.7%), 29 open**.
It does not make Rust source directly importable from TypeScript; native services
continue using their generated contract clients.


Better Auth rate-limit customization now supports positive default windows/budgets
and exact auth-relative path overrides, including endpoint disablement. The
standard `rust_better_auth_rate_limits.mjs` entry point runs 153 default and 174
configured HTTP comparisons against actual Better Auth 1.7.6. Sensitive built-in
rules override defaults; explicit endpoint settings override both. Retry headers,
rolling boundaries, path normalization, parser/guard ordering and atomic request
counting are preserved. Configuration rejects zero budgets/windows and unsupported
wildcard/query/encoded paths. Dynamic rule callbacks, wildcard rules and external
storage remain open; this narrows the existing customization gate without changing
the broad checklist completion count.

Validation receipt: 36 portable auth tests pass (the dedicated PostgreSQL unit
test remains explicitly ignored in that run), strict all-feature/all-target auth
clippy passes, and fresh generated run `cK6KDS` passes against restricted-role
PostgreSQL. Eight actual frontend client scenarios, both generated SDKs,
HTTP/MCP shared/separate/disabled listeners, Origin rejection, restart/shutdown,
relocation, preservation, rename and deletion all pass.


Better Auth rate-limit wildcard rules now match upstream first-match insertion
order, including wildcard-before-exact and exact-before-wildcard precedence,
endpoint disablement, `*` segment boundaries, whole-segment `**`, and `?` within
wildcard patterns. Replacing a configured pattern preserves its original order.
The shared trusted-origin wildcard compiler is reused and compiled once per
configured rule. The standard rate-limit fixture now passes 969 actual HTTP
comparisons across default, exact and three wildcard profiles; all 2,822 actual
trusted-origin/callback/CSRF comparisons remain green. Dynamic callbacks and
external storage remain open under the existing customization gate.

Wildcard validation receipt: 37 portable native auth tests and strict all-feature,
all-target clippy pass, including live HTTP/MCP startup/shutdown and concurrent
rate limiting. The PostgreSQL-specific unit test remains explicitly ignored.


Native Better Auth custom rate-limit callbacks now await asynchronous decisions
from method, original URI, normalized auth-relative path and headers. Each callback
receives the inherited sensitive/default rule and can retain it, disable the limit,
or select a positive window/budget. Only the first matching rule executes; counter
locks are acquired after the callback completes, and default/static rules avoid
cloning request metadata. Callback failures fail closed without exposing private
messages. The standard fixture passes 1,485 actual Better Auth HTTP comparisons
across default, exact, wildcard and two asynchronous-rule profiles. Bodies remain
untouched for downstream parsing. Body-reading callbacks, plugin rate rules and
external counter stores remain open under the broader customization gate.

Callback validation receipt: the existing 39 auth tests pass, and all seven focused
rate-limit regressions pass after correcting the new privacy test's router mount.
Strict all-feature/all-target clippy and the 1,485-case HTTP fixture pass. The
PostgreSQL-specific unit test remains explicitly ignored in the portable suite.


Native plugin rate rules now accept ordered path predicates and positive limits.
The first matching plugin rule overrides built-in sensitive/default rules; the
first matching custom rule can override it, and asynchronous custom callbacks
receive the effective plugin window/budget. The expanded fixture passes 1,947
actual Better Auth HTTP comparisons, including reversed plugin ordering, custom
disablement and callback inheritance. Body-reading callbacks and external counter
stores remain open; this does not claim complete Better Auth plugin support.

Plugin-rule validation receipt: all 41 portable native auth tests pass, with the
PostgreSQL-specific unit test explicitly ignored. The final eight focused rate
regressions, strict all-feature/all-target clippy, and 1,947 HTTP comparisons pass;
plugin predicate call traces also match actual Better Auth, including static
custom overrides. Fresh generated run `IxtPZO` passes restricted-role PostgreSQL,
eight blueprint/generated frontend-client scenarios, both SDKs, HTTP/MCP
shared/separate/disabled listeners, Origin rejection, restart/shutdown,
relocation/preservation, rename and deletion. The final predicate-order adjustment
was checked with native regressions and actual-library traces after that generated
run; the checkout CLI was rebuilt with the final sources.


Customer persistence now has typed native create/read/update/delete/list operations
over the existing durable billing store. PostgreSQL migration 3 adds the customer
kind; email/name use tenant-, record- and field-bound encrypted envelopes and
atomic actor audits. Shared memory/restricted-role PostgreSQL tests cover durable
CRUD, concurrent disjoint updates, duplicate-ID races, nullable-name clearing,
empty-ID filtering and cache rejection. PostgreSQL also verifies tenant isolation,
audit-failure rollback, restart/reopening, retained-key rotation, ciphertext
substitution rejection and absence of customer plaintext in storage. This fills
the planned native customer persistence gap; the five configured TypeScript
billing services remain unchanged and all 168 actual service comparisons pass.

Stripe customer create/retrieve/update/list/delete now use the existing direct
and managed client helpers alongside checkout lifecycle operations. The official
Stripe 22.6.2 fixture passes 26 direct cases; actual TypeScript managed transport
passes 28 cases, including customer nested address/metadata encoding, expansions,
pagination parameters, null updates and deletion. Provider/local-write atomicity,
reconciliation and configured mapper review remain separately open.


Billing completion receipt: 11 portable billing tests, the dedicated restricted-role
PostgreSQL integration, strict all-target/all-feature billing clippy, 168 base-service
comparisons and 26 direct/28 managed Stripe cases pass. Generated run `kpWfCb`
passes mixed billing/Stripe modules, both SDKs from contract and OpenAPI imports,
HTTP/MCP startup, encrypted persistence and lifecycle. The first run `4fppO5`
exposed repeated whole-document SDK indexing; commit `299933772` reuses one
borrow-bound document index while preserving cumulative expansion limits. Its
80-operation/300-component regression, all 109 CLI SDK tests, 43 shared codegen
tests and strict codegen clippy pass. Customer persistence and customer/checkout
helper gates are closed; provider/local-write reconciliation remains open.


Better Auth custom rate-limit storage receipt (2026-10-13):
`AuthRateLimitOptions::with_custom_storage` accepts an async application-owned
atomic check/increment backend. Effective plugin/custom rules and normalized
`IP|auth-path` keys reach the backend; disabled limits bypass it. Retry omission
uses the effective window and explicit zero survives. Errors return a generic
failure without private details or process-local fallback. Applications own
backend atomicity and expiry; built-in Redis/PostgreSQL counter backends and
body-reading rule callbacks remain open.

All 2,409 actual Better Auth HTTP comparisons pass, including exact backend and
plugin call traces. Native validation passes 43 tests (one PostgreSQL-specific
unit test ignored), including concurrent custom consumption and fail-closed
errors, plus strict all-feature/all-target clippy. Fresh generated run `93F5fo`
passes restricted-role PostgreSQL, eight actual frontend-client scenarios, both
SDKs, HTTP/MCP shared/separate/disabled listeners, Origin rejection,
restart/shutdown, relocation/preservation, rename and deletion. Logs are in
`/private/tmp/fl-auth-storage-{parity,tests,clippy,generated}.log`. This advances
the broader customization gate without marking it complete.


AsyncAPI trait import receipt (2026-10-13): message and operation traits now
resolve bounded local references and merge in declaration order, with target
fields applied last. This follows the [AsyncAPI 3 merge mechanism](https://www.asyncapi.com/docs/reference/specification/v3.0.0#traits-merge-mechanism).
Arrays replace earlier arrays and nested null members remove inherited values.
Payload scopes remain anchored to the original message; traits cannot introduce
payloads. Inherited content types and bindings receive the existing wire checks;
unsupported application headers, security and bindings still fail explicitly.
Cycles, external references, invalid trait targets and more than 64 traits per
object fail before generation; expansion shares the import byte budget.

All 29 native AsyncAPI tests and strict all-feature/all-target clippy pass.
The official AsyncAPI parser agrees on local references, merge ordering, target
precedence, arrays and nested null removal. Fresh relocated run `xjjTgI` uses
trait-inherited message content types and operation metadata, compiles both SDKs,
runs live WebSocket peers, checks watch recovery and input ownership, and passes
service rename/deletion and the full generated HTTP/MCP startup/shutdown matrix.
The source-package fixture includes all 913 current embedded assets. Logs:
`/private/tmp/fl-asyncapi-traits-{tests,parity,clippy,generated,package}.log`.
The broader event/protocol, upgrade authentication/parameters, replies and richer
message matching import gate remains open.


Optional version-body hooks receipt (2026-10-13): native preprocessing can now
receive absence separately from explicit JSON null and supply defaults before
per-version schema validation. Optional transforms can produce an absent result
after successful validation. Existing required-value hooks still skip absent
inputs; registering either callback form replaces its counterpart.
`OptionalVersionedBody<T>` extracts a selected optional result without consuming
raw bytes and rejects missing version selection or incompatible present values.

The actual repository TypeScript parser/Zod comparison adds 12 cases for absent
input/defaults, explicit null, absent transformed output, failed transformations,
independent version candidates and fallback. All 45 prior version-selection,
six parser-registration and 18 transformation comparisons also pass. Native
validation passes 34 HTTP tests, 65 Axum tests and three integration tests, plus
strict all-feature/all-target clippy. Fresh generated run `uzzJd8` passes both
SDK languages through contracts, full/named OpenAPI and native generators;
HTTP/MCP root deletion/defaults and absent transformed results; all prior
media/parser/verify/reviver cases; listener startup and graceful shutdown.
Logs: `/private/tmp/fl-optional-body-{parity,tests,clippy,generated}.log`.
Sparse arrays, mutable reviver holders/source context and broader non-body
transformations remain open. Checklist remains 655/682 complete, 27 open.


TypeScript verified-session authorization receipt (2026-10-13):
`forklaunch_auth::typescript::TypeScriptRbacPolicy` provides explicit compatibility
with the actual middleware's omitted/empty rule sets, permission-before-role
precedence, access-dependent checks, exact scope membership, and subscription
before feature checks. Native RBAC retains its existing semantics. The new
`RouterConfig::with_authorizer` callback replaces native RBAC only when explicitly
configured, receives an already verified/enriched session, and cannot run after
failed credential verification. Async grant resolution remains in the verifier.
The policy does not replace credential parsing, session-schema validation,
missing surfacing callbacks, public route dispatch or HMAC's sessionless path.

All 1,495 actual repository middleware session-policy comparisons pass. The
fixture substitutes credential verification and telemetry only; the actual
middleware and its authorization guards evaluate every policy. Native validation
passes 16 auth tests, 66 Axum tests and three integration tests, plus strict
all-feature/all-target clippy. Fresh generated `WQrswi` verifies signed JWTs and
runs permissions, exact/unknown scopes, subscriptions/features, missing policy
and rejected credentials over HTTP/MCP, compiles both SDKs and passes the full
listener/startup/shutdown matrix. All 915 current embedded source assets are
packaged. CI runs both the actual-source comparison and generated runtime.
Logs: `/private/tmp/fl-ts-rbac-{parity,tests,clippy,generated,package}.log`.
Broader auth transport/session and error-envelope parity remain open.


OAuth/OIDC library adapter receipt (2026-10-13): optional `forklaunch-auth/oidc`
now uses [oauth2 5](https://docs.rs/oauth2/5.0.0/oauth2/) and
[openidconnect 4](https://docs.rs/openidconnect/latest/openidconnect/) for reusable
authorization-code provider integration. Discovery is pinned to operator-owned
issuer/client/callback configuration; transport permits HTTPS or loopback HTTP,
refuses redirects/URL credentials/fragments and bounds responses/time. Pending
state is non-cloneable, expires in ten minutes and binds issuer/client/callback;
applications retain it in the initiating browser's server-side session. Public
and HTTP Basic clients use S256 PKCE. RS256 verification checks issuer, audience,
expiry, nonce, present azp, nonempty subject, future iat and present at_hash.
Provider errors and Debug omit secrets.

All 22 auth tests and strict all-feature/all-target clippy pass. Local provider
coverage includes real discovery/JWKS/token HTTP, PKCE wire bytes, public and
confidential clients, state/config/expiry failures before exchange, invalid
metadata/scopes, nonce/signature/issuer/audience/azp/expiry failures, substituted
access tokens, redirects and declared/chunked response bounds. The all-lockfile
security gate and its tests pass with no unreviewed findings. The new framework
RSA advisory is retained with a public-verification-only source review; it is
not claimed fixed. No existing locked package version was removed/upgraded.
Fresh generated `Z2FzoB` enables the optional adapter, compiles both SDKs and
passes signed-JWT authorization and the HTTP/MCP listener/startup/shutdown matrix.
All 918 embedded assets are packaged. Logs:
`/private/tmp/fl-oidc-{tests,clippy,security,security-tests,generated,package}.log`.

This closes the explicit OAuth/OIDC library-integration requirement. The generic
adapter does not provision accounts/sessions or implement refresh grants or
automatic key refresh; the configured Better Auth Google flow remains separate.
Checklist: 656/682 complete (96.2%), 26 open.


OIDC refresh receipt (2026-10-13): the optional generic adapter now supports
refresh grants using trusted original login claims and encrypted server-owned
tokens. Newly returned ID tokens retain issuer, subject and audience, cannot
backdate issuance before the login, and preserve authentication time and nonce
when supplied, following [OIDC refresh rules](https://openid.net/specs/openid-connect-core-1_0.html#RefreshTokenResponse).
An omitted ID token produces absent claims; an omitted replacement refresh token
retains the supplied token. Rotated tokens are returned for application-owned
atomic persistence. Empty/non-Bearer token responses and mismatched original
provider/client claims fail closed. This supersedes the earlier refresh limitation.

All 25 auth tests and strict all-feature/all-target clippy pass, including real
local-provider refresh wire requests, identity/nonce/time/hash failures, token
rotation and optional tokens. Generated run `YAAZD4` compiles both SDKs and passes
signed-JWT authorization plus the HTTP/MCP listener/startup/shutdown matrix.
All 919 embedded assets are packaged. Logs:
`/private/tmp/fl-oidc-refresh-{tests,clippy,generated,package}.log`.
Automatic key refresh and account/session provisioning remain open; the broader
provider gate stays open. Checklist remains 656/682 (96.2%), 26 open.


Reference billing pagination regression (2026-10-13): clamp numeric customer
cursors to the stored collection length before slicing, matching the existing
reference IAM pagination behavior. Empty collections, terminal/beyond-end cursors,
maximum integer cursors/limits and normal multi-page traversal are covered.
The three billing unit tests and portable persistence test pass; the separately
configured PostgreSQL test remains ignored in this run. Strict all-feature/
all-target billing clippy passes. Logs:
`/private/tmp/fl-billing-cursor-{tests,clippy}.log`. This does not close the broader
billing reconciliation gate.


All-module lifecycle receipt (2026-10-13): `rust_module_lifecycle.sh` now checks
all eight built-in variants together in a relocated mixed app with a TypeScript
library: iam-base, iam-better-auth, cac-base, messaging-base, messaging-twilio,
ecommerce-stripe, billing-base and billing-stripe. Run `smv1a8ho` passes repeated
initialization, sync, preserved user source/package metadata, rename, variant
ownership, OpenAPI component naming, Rust/TypeScript compilation from both
OpenAPI and native contracts, SDK drift checks and deletion of contracts/SDKs/
registrations. The TypeScript library survives byte-for-byte and the final empty
Cargo workspace remains valid. The fixture is included in E2E CI and documented
in the blueprint workspace guide. Earlier family runtime receipts cover live
HTTP/MCP, encrypted storage and providers; relay/storefront have their own
checked lifecycle fixtures. This closes the combined module lifecycle gate,
not the remaining deployment or provider behavior gates.

The fresh source-archive installer first exposed invalid Better Auth response
component names (`Surface User Roles`/`Surface User Permissions`). Native module
contracts now use `SurfaceUserRoles`/`SurfaceUserPermissions`, preserving actual
TS operation names and response shapes. Five actual TS contracts/nine response
schemas and strict all-feature Better Auth clippy pass. The component-name audit
also replaces explicit `Vec<String>` query schema names in IAM/ecommerce/billing
with `StringArray`, preserving wire arrays. Six interface tests and strict
all-feature/all-target clippy pass. Component keys follow the
[OpenAPI 3.1 naming rule](https://spec.openapis.org/oas/v3.1.0.html#components-object).

Archive run `A9tzbZ` includes the Better Auth correction and passes installation,
portable module/worker compilation, both SDK import paths/native generators,
HTTP/MCP startup/listener/shutdown checks and all five managed capability
callbacks. Later query-name changes pass the final all-module matrix and the
919-asset source-package check. Logs:
`/private/tmp/fl-parity-packaged-install-final.log`,
`/private/tmp/fl-module-matrix-final.log`,
`/private/tmp/fl-module-interface-{tests,clippy}.log`,
`/private/tmp/fl-auth-component-{parity,clippy}.log`,
`/private/tmp/fl-module-matrix-package.log`.
Checklist: 657/682 complete (96.3%), 25 open. Cleanup removed 17.5 GiB of inactive
port-owned test executables older than 24 hours; active builds and other worktrees
were preserved.


General OpenAPI component-name receipt (2026-10-13): the exporter now maps
arbitrary contract schema names to valid, collision-safe OpenAPI component keys.
Ordinary names remain unchanged; empty names, characters outside the OpenAPI
component alphabet and the reserved `_fl_encoded_` prefix use UTF-8 hexadecimal
encoding. The reserved-prefix case prevents a user name from colliding with an
encoded key. Both route references and relocated recursive schema references
use the mapped key; schema titles and literal annotations remain unchanged.

All eight OpenAPI tests and strict all-feature/all-target clippy pass, covering
spaces, generic syntax, Unicode, slash/tilde/percent, empty names, literal-prefix
collisions, recursive roots and definitions. Generated run `XjdxFB` exercises
unusual names through both CLI artifact imports, both native SDK generators,
strict Rust/TypeScript consumers, live recursive requests/responses and HTTP/MCP
startup/discovery/invocation/shutdown. The fixture is in E2E CI. All 920 embedded
assets are packaged. Logs:
`/private/tmp/fl-component-names-{tests,clippy,generated,package}.log`.
The broader schema/emitter conformance gate remains open; checklist stays
657/682 (96.3%), 25 open.


Versioned field transformation receipt (2026-10-13): native runtime hooks now
preprocess/default and transform declared parameters, query fields and headers
around each version's schema checks. Missing and null remain distinct; absent
post-transform values are accepted. Last-successful selection, unknown-field
stripping and TypeScript's truthy replacement of existing header keys are covered
by 320 comparisons against the actual repository parser/Zod source. Typed Axum
extractors expose selected results while preserving raw query/header values.
Authentication remains before callbacks; credential-transform compatibility is
not claimed. Registration rejects unknown fields/versions and ambiguous routes.

Generated run `j0VNl1` passes both SDK compilers from contract and OpenAPI imports,
36 live HTTP/MCP transformation cases, invalid-input rejection and the full
HTTP/MCP startup/listener/shutdown matrix. The first run `HwpggS` exposed raw
percent-encoded path captures. Shared matching now decodes captures once after
matching the complete route; encoded slashes do not split routes, double encoding
is preserved after one decode, and malformed escapes/non-UTF-8 captures return
400. Unit and generated live tests cover these cases, including validation-off
local dispatch and unchanged raw paths. The new fixtures are registered in CI.

39 HTTP tests, 67 Axum tests and three integration tests pass, as do strict
all-feature/all-target clippy and the no-default-feature Axum check. Logs:
`/private/tmp/fl-version-fields-{tests,clippy,parity,generated,disabled,package}.log`.
The percent-encoding dependency was already locked; only the HTTP dependency edge
was added. Sparse values, mutable holder/context behavior, cross-field side-effect
ordering and remaining codec boundaries stay open. Checklist remains 657/682
(96.3%), 25 open.


Billing webhook recovery receipt (2026-10-13): migration 4 adds tenant-isolated,
metadata-only attempts committed before callback effects and an operator resolution
audit. Failed, cancelled or interrupted callbacks remain blocked across replicas
and restarts. This intentionally tightens native retry behavior: ambiguous effects
are not blindly re-executed. Successful receipt insertion and attempt removal are
atomic. Bounded pending inspection and exact-attempt resolution support Applied
(suppress replay) or RetryAllowed (operator-verified safe retry); this API does not
query or repair provider state automatically. Active callbacks retain their lock,
and stale attempts cannot resolve newer retries. Actor/correlation context and
atomic audit insertion are required. Runtime/operator grants are documented.

Three base unit tests, memory/restricted-role PostgreSQL integration and the native
HTTP/MCP integration pass. Tests include cancellation after a committed local write,
store reopening without replay, tenant separation, active callback contention,
stale resolution, anonymous resolution rejection, audit-write rollback, successful
deduplication and a two-connection pool. Strict base/Axum all-feature/all-target
clippy passes. Generated run `f21N1N` passes signed direct/platform blocked replay,
restart, encrypted storage, both live SDKs/import paths, managed gateway callbacks,
HTTP/MCP startup/shutdown and rename/delete lifecycle. The earlier `Wj5b8x` run
found a malformed platform test envelope, corrected before the passing run.
All 925 embedded assets are packaged. Logs:
`/private/tmp/fl-billing-reconciliation-tests.log`,
`/private/tmp/fl-billing-reconciliation-clippy.log`,
`/private/tmp/fl-billing-reconciliation-generated-final.log`,
`/private/tmp/fl-billing-reconciliation-package.log`.
Outbound provider/local-write recovery and remaining mapper decisions stay open;
checklist remains 657/682 (96.3%), 25 open.





Verified PayPal capture recovery receipt (2026-10-13): `PaypalOrderReader` and
`PaypalClient::retrieve_order_details` add authenticated bounded order retrieval.
`PaypalPaymentService::reconcile_captured_payment` verifies completed/final capture
state, exact amount/currency and order reference before a local repair. One purchase
unit/one capture matches this client's creation format. Pending/refunded, multiple,
substituted and mismatched evidence is rejected; no capture/refund/delivery request
is issued. Ordinary TypeScript provisional-claim and failure semantics remain,
with 22 actual client cases and 19 wrapper operations passing unchanged.

`PaymentReconciliationStore` adds audited snapshot-checked repair for memory and
PostgreSQL. Migration 6 adds tenant-RLS audit metadata. Runtime roles need SELECT
and operators need INSERT; generated documentation and fixture grants are updated.
Audit failure rolls back a changed payment, stale snapshots and anonymous actors
are rejected, and a delayed legacy claim release cannot reset verified completion.
Seven base business-rule tests, two recovery tests (including restricted PostgreSQL
and reopening), the PayPal live provider test and both native Axum tests pass.
Strict all-feature/all-target clippy passes for base/PayPal/Axum. Provider cases
reject amount/currency/reference/status mismatches, overflow/invalid money,
multiple captures/units, refund/authorization metadata and substituted order IDs.

Generated run `oDyeRU` compiles both SDKs from both artifacts and passes live
HTTP/MCP, checkout/webhook comparisons, worker delivery, encrypted persistence,
restart and CLI/Compose lifecycle. Its runtime phase was resumed in the same
compiled workspace after supplying the required Stripe fixture dependency path.
All 930 embedded assets are packaged. Logs:
`/private/tmp/fl-paypal-reconciliation-{tests,clippy,parity,package}.log`,
`/private/tmp/fl-paypal-reconciliation-generated.log`,
`/private/tmp/fl-paypal-reconciliation-generated-final.log`.
A disk-full build and stopped retained PostgreSQL fixture were resolved by removing
1.46 GiB of inactive port-owned executables and starting a fresh owned database.
No source/unrelated workspaces were removed. Uncaptured/ambiguous claims and
cancelled-order provider references remain open; checklist remains 657/682
(96.3%), 25 open.


Verified Stripe capture recovery receipt (2026-10-13): Stripe client/service
helpers retrieve expanded charge evidence and use the audited payment-repair
boundary introduced for PayPal. Full intent/charge amounts, currencies, provider
reference and order metadata must match; only succeeded, paid, fully captured,
unrefunded and undisputed evidence is accepted. Connect account headers are retained.
Provider reads do not create/capture/refund charges or deliver orders; raw client
secrets/provider responses never enter the audit. Twenty invalid provider variants
are rejected by live HTTP tests without another mutation or audit entry.

Both Stripe native tests and strict all-feature/all-target clippy pass; fifteen
actual TypeScript comparisons confirm ordinary provider/Connect/resume/lifecycle
semantics remain unchanged. Generated run `wg2LRs` passes both SDKs/import formats,
HTTP/MCP startup, checkout/webhook comparisons, cross-process checkout admission,
worker/inventory persistence, restart and CLI/Compose lifecycle. All 931 embedded
assets are packaged. Logs:
`/private/tmp/fl-stripe-reconciliation-{tests,clippy,parity,generated,package}.log`.
Incomplete captures and cancelled-order references still require separate recovery;
checklist remains 657/682 (96.3%), 25 open.


Trusted custom JWT signer receipt (2026-10-13): `JwtSigner` and
`KeyRing::sign_jwt_with_signer` accept a typed asynchronous server callback,
preserve Better Auth custom-sign claim/default semantics, and bypass local key
selection, JOSE claim setters and storage. Validation precedes callback invocation;
callback failures are sanitized. Panic-on-access storage and pending-callback tests
prove no database access and caller-controlled cancellation. Non-finite JavaScript
claims are rejected because the public boundary uses finite JSON values.

All 45 native auth tests pass (one database opt-in test ignored); strict all-feature,
all-target Clippy passes. Actual Better Auth comparisons cover 62 custom-sign cases,
plus 189 existing signing API and 216 persisted key-state regression cases.
Generated run `8smf2l` validates both SDKs, eight actual client/namespace scenarios,
HTTP/MCP shared/separate/disabled startup, encrypted tenant sessions, independent
JWT verification, restart, relocation/preservation, rename and delete. All 932
embedded assets are packaged. Logs:
`/private/tmp/fl-custom-jwt-{tests,clippy,parity,local-regression,generated,package}.log`.
This helper does not configure session-token HTTP routes or remote JWKS; those
remain open within broader auth coverage. Checklist remains 657/682 (96.3%), 25 open.


External HTTP JWT signer receipt (2026-10-13): trusted `ExternalJwtSigner`
configuration wires the typed callback into session-authenticated `/token`,
`get-session` JWT headers and popup callback exchange. Explicit asymmetric
algorithm/public-key URL metadata is required. Local `/jwks` returns 404 and
signing creates no local key rows; key publication, rotation and verifier
configuration remain application-owned, as does callback algorithm enforcement.

Nine actual Better Auth HTTP cases compare configured claims, headers, anonymous
and forged sessions, and remote-JWKS suppression. Live TCP tests independently
verify signatures, deny revoked sessions without invoking the signer, sanitize
provider failures, check popup exchange and confirm empty local key storage.
The complete native suite passes 47 tests (one database opt-in test ignored),
updated focused tests and strict all-target Clippy pass, and 161 existing actual
HTTP/session comparisons remain unchanged. Generated run `gRhDXs` passes both
SDKs, all eight client/namespace scenarios, encrypted persistence, HTTP/MCP
startup/discovery/invocation, restart and rename/delete. All 934 embedded assets
are packaged. Logs:
`/private/tmp/fl-external-jwt-{tests,all-tests,clippy,parity,http-regression,generated,package}.log`.
Checklist remains 657/682 (96.3%), 25 open; broader auth adapters/options remain.


Common naming/hash symbol receipt (2026-10-13): the following public TypeScript
exports map directly to `forklaunch_common` string-input APIs. One actual-source
fixture executes 591 inputs across all twelve exports (7,092 comparisons), covering
separator versus stripped punctuation, numeric prefixes, original-input fallback,
Unicode case conversion, UTF-16 leading-surrogate behavior and signed hash overflow.

| TypeScript export | Rust export |
| --- | --- |
| `removeLeadingSeparators` | `remove_leading_separators` |
| `splitBySeparators` | `split_by_separators` |
| `removeNonAlphanumeric` | `remove_non_alphanumeric` |
| `removeLeadingNumbers` | `remove_leading_numbers` |
| `capitalize` | `capitalize` |
| `uncapitalize` | `uncapitalize` |
| `toCamelCaseIdentifier` | `to_camel_case_identifier` |
| `toPrettyCamelCase` | `to_pretty_camel_case` |
| `toValidIdentifier` | `to_valid_identifier` |
| `isValidIdentifier` | `is_valid_identifier` |
| `getTransformationInfo` | `get_transformation_info` / `TransformationInfo` |
| `hashString` | `hash_string` |

Operation names now use the actual TS transformation: `foo!bar` becomes `foobar`
instead of `fooBar`, leading numbers are removed, and empty conversions retain the
original string. Clients using affected names need regeneration. Rust string inputs
do not represent isolated JavaScript surrogate code units or non-string coercions.
The hash is non-cryptographic and adds no security primitive.

All 98 affected common/contract/macro/OpenAPI/SDK tests and strict all-target Clippy
pass. Generated run `ftABml` validates both SDKs from both artifact formats against
Rust and TypeScript HTTP services, HTTP/MCP startup/discovery/invocation/shutdown
and both native multipart SDKs. Packaging caught and fixed a missing common-example
template link; all 936 embedded assets are included. Logs:
`/private/tmp/fl-common-naming-{parity,tests,clippy,generated,package}.log`.
Remaining utility/symbol and transport boundaries stay open; checklist is still
657/682 (96.3%), 25 open.


Common slash helper receipt (2026-10-13): `removeTrailingSlash`,
`removeDoubleLeadingSlash` and `sanitizePathSlashes` now map to their exact
single-step Rust counterparts, including empty/root inputs, relative paths,
repeated slashes and trailing line separators. Native contract/request/router
normalization moved to explicit `normalize_route_path`, preserving its existing
leading-slash insertion, boundary collapse and root behavior. Callers needing the
old normalization should use that function rather than the TypeScript helpers.

The common actual-source matrix now checks 697 inputs across fifteen exports
(10,455 comparisons). All 159 affected common/contract/HTTP/Axum/MCP tests and
strict all-feature/all-target Clippy pass. Generated run `ecWquM` validates
versioned field transforms/defaults, parameter decoding/raw wire preservation,
both SDKs from both artifacts and HTTP/MCP listener/startup/shutdown profiles.
All 936 embedded assets remain packaged. Logs:
`/private/tmp/fl-common-path-{parity,tests,clippy,generated,package}.log`.
OpenAPI parameter-string conversion and the rest of the public utility/symbol
audit remain open; checklist stays 657/682 (96.3%), 25 open.


Native platform deployment receipt (2026-10-13): CLI release components now carry
`runtime: "tokio"` and their actual Cargo executable name, including custom crate
names and both worker components. JavaScript components retain their existing
manifest shape. The companion checkout `../forklaunch-platform-rust-parity`,
branch `rohin/rust-runtime-parity`, commits `fe0974937` and `050b1ef99`, preserves
these fields through platform schemas and starts native containers with direct
argv and a cleared image entrypoint. Runtime-aware build groups, CodeBuild and
image gates skip JavaScript installation for native-only images, use independent
mixed-image contexts and reject conflicting Node/Bun workspace installers.
Multiple image builds now emit independently scoped helpers and compile together.

Native tasks default to port 8000, network-accessible service hosts and enabled
worker health listeners. Native DATABASE_URL values survive worker processing,
fall back to the final DB_URL unless explicitly unset, use secret task references
and secret stack outputs. Explicit connections remain authoritative. This does
not grant database privileges, synthesize tenants or run owner migrations.

All 1,101 CLI tests pass. Real optimized native and mixed Cargo/PNPM/Node release
builds retain component metadata, generate both SDK targets and reuse prebuilt
artifacts without publishing (`/private/tmp/fl-native-generated-release.log`).
Platform validation covers 117 focused deployment/build
checks plus five manifest-schema checks, generated mixed-program compilation,
legacy JavaScript snapshot equality and full deployment-worker type checking
against rebuilt local core/platform-management declarations. Generated relocated
Linux container run `tBwUZM` passes HTTP/OpenAPI/MCP startup, discovery/invocation,
health checks and graceful SIGTERM shutdown. Logs:
`/private/tmp/fl-platform-native-{build,install,listeners,database,schema}-tests.log`,
`/private/tmp/fl-platform-native-{worker,database}-types.log`,
`/private/tmp/fl-platform-native-container.log`,
`/private/tmp/fl-native-release-all-cli-tests.log`.

No AWS deployment was performed. Native migration jobs, database role/tenant
provisioning, actual ECS/load-balancer behavior and worker role/scaling semantics
remain open. Both generated native worker components currently execute a combined
consumer/control process; distinct HTTP-only/consumer-only roles are not implied.
The companion deployment guide records these limits. The checklist remains
657/682 (96.3%), 25 open.


Basic credential compatibility receipt (2026-10-13): opt-in
`forklaunch_auth::typescript::basic::verify_basic` matches 815 actual TypeScript
middleware cases for missing/empty headers, exact/custom prefixes, first-space
selection, Node's two base64 alphabets/ignored bytes/padding/trailing bits,
UTF-16 low-byte input, replacement UTF-8, empty/extra colon fields and accepted or
rejected synchronous login callbacks. No principal is returned before successful
verification. Strict native `parse_basic` remains unchanged.

`RouterConfig::with_response_authenticator` carries safe application-selected
credential errors through native HTTP/MCP, followed by ordinary authorization.
The existing verifier API and cookie verifier behavior remain available. Native
errors retain structured JSON; this does not close the TS text-error envelope gate.
All 96 affected auth/Axum tests plus the new live HTTP/MCP Basic test pass,
as do strict all-feature/all-target Clippy and the 1,495 session-policy comparisons.
Generated relocated run `Fud7KV` validates Basic/JWT/policy cases over HTTP and MCP,
both SDK compilers and the complete shared/separate listener startup/shutdown
matrix. All 938 embedded assets are packaged. Logs:
`/private/tmp/fl-typescript-basic-{parity,adapter-tests,sockets,clippy,generated,package,rbac-regression}.log`.
Async/custom Basic resource decoders, session-schema validation and remaining
credential/error transports remain open; checklist stays 657/682 (96.3%), 25 open.


OpenAPI path utility receipt (2026-10-13): `open_api_compliant_path` now implements
the exact public TypeScript global ASCII-word replacement, including literal
prefixes, file suffixes, adjacent parameters and non-ASCII boundaries. Native
contract construction and route decoration use explicit `route_open_api_path`,
preserving their existing whole-segment conversion/matching. This does not claim
native routing support for partial-segment parameters.

The actual-source common matrix covers 711 inputs across sixteen exports (11,376
comparisons). All 130 affected common/contract/OpenAPI/Axum/MCP tests and strict
all-feature/all-target Clippy pass. Generated relocated run `0tl7w8` passes versioned
field transforms/defaults, raw wire preservation, both SDK compilers and the full
HTTP/MCP startup/shutdown matrix; all 938 embedded assets are packaged. Logs:
`/private/tmp/fl-common-openapi-{parity,tests,clippy,generated,package}.log`.
Native embedded-parameter route matching and the remaining public-symbol audit
stay open; checklist remains 657/682 (96.3%), 25 open.


Same-channel AsyncAPI reply receipt (2026-10-13): server-view WebSocket imports
now accept fixed same-channel operation replies, inline or via bounded local
reply references. Replies populate the opposite message direction and retain
existing schema/disjointness/media validation. Explicit nonempty message subsets
must belong to the same root channel. Dynamic addresses, cross-channel routing,
external/cyclic/sibling references, missing/foreign messages, overlapping schemas
and oversized subsets fail before either SDK is changed. Stream clients expose
send/receive; applications own correlation, ordering and reply deadlines.

All 18 AsyncAPI tests pass, including eight JSON/binary/action/reference variants
and twelve rejection mutations, as do strict all-target Clippy and all 1,101 CLI
tests (`/private/tmp/fl-asyncapi-reply-cli-tests.log`). Generated
relocated run `hsJ5Me` validates the document with the official parser, compiles
strict Rust/TypeScript SDKs and exchanges real request/reply messages with a live
peer. It checks invalid-reply output preservation, watch recovery, relocation,
rename/delete ownership and both services' complete HTTP/MCP startup/shutdown
matrices. All 938 embedded assets remain packaged. Logs:
`/private/tmp/fl-asyncapi-reply-{tests,clippy,generated,package}.log`.
The [AsyncAPI 3 reply model](https://www.asyncapi.com/docs/reference/specification/v3.0.0#operationReplyObject)
remains broader than this fixed WebSocket profile: dynamic/cross-channel replies,
automatic correlation, upgrade security/parameters, other protocols and event
projects stay open. Checklist remains 657/682 (96.3%), 25 open.


OIDC signing-key rotation receipt (2026-10-13): login and refresh responses with
ID tokens now fetch fresh JWKS from the discovery-pinned URL before verification.
This supersedes the automatic-key-refresh limitation in the earlier receipts.
Keys are fetched once per verification without credential headers or token-grant
retries, using existing no-redirect, 1 MiB and 15-second transport bounds. Failed,
revoked, malformed and oversized key sets never fall back to discovery-time keys.
Refresh responses without ID tokens skip the key request. The deliberate absence
of a cache adds provider latency/load and makes key availability a login dependency;
endpoint metadata changes still require explicit rediscovery.

All 28 auth tests and strict all-feature/all-target Clippy pass. A local provider
rotates actual RSA key material between discovery, login and refresh, asserting
identity binding, exact grant/key request counts and credential-free key fetches.
Generated relocated run `z9iUdx` executes the vendored OIDC wire tests, compiles
both SDKs and passes Basic/JWT authorization and the complete HTTP/MCP listener,
discovery, invocation and shutdown matrix. All 939 embedded assets are packaged.
Logs: `/private/tmp/fl-oidc-key-refresh-{tests,clippy,generated,package}.log`.
Account/session provisioning and broader provider/options coverage remain open.
Checklist remains 657/682 (96.3%), 25 open.


Basic async resource decoder receipt (2026-10-13): the opt-in
`typescript::basic::verify_basic_resource` verifies through a trusted async
callback instead of synchronous username/password login. Exact prefix and first
space-token behavior precede the callback; its token is passed unchanged. Null
claims return the TypeScript 403, while other JSON claims are preserved for
application-owned session validation and principal binding. Callback errors are
explicit public responses. Cancellation drops the callback without retries.

All 30 auth tests and strict all-feature/all-target Clippy pass. The actual-source
Basic comparison fixture now checks 1,067 cases, including 252 async resource
cases covering prefix errors, empty/opaque tokens, decoder precedence and JSON
claim shapes. Generated relocated run `6W7xXG` compiles both SDKs, passes vendored
OIDC wire tests and verifies Basic resource login, invalid sessions, role denials
and trusted claim propagation over both HTTP and MCP. Its complete listener and
startup/shutdown matrix passes. The wire fixture accounts for Fetch trimming a
trailing Authorization space; direct middleware comparisons retain empty-token
coverage. All 939 embedded assets remain packaged. Logs:
`/private/tmp/fl-basic-resource-{tests,parity,clippy,generated,runtime,startup,package}.log`.
Session schemas, broader callbacks and error envelopes remain open; checklist is
657/682 (96.3%), 25 open.


JWT custom resource verifier receipt (2026-10-13): the opt-in
`typescript::jwt::verify_jwt_resource` awaits a trusted verifier/decoder after
exact Bearer/custom prefix and first-token checks. Falsy JSON results reject
verification; callback errors become the TypeScript generic 403 without exposing
provider details. The application must verify credentials in the callback and
validate/bind the returned session before authorization. This adds no implicit
payload-only decoder and leaves strict built-in JWT verification unchanged.

All 32 auth tests and strict all-feature/all-target Clippy pass. The shared
actual-source credential fixture now checks 1,931 cases (864 new JWT callback
cases), covering error redaction, falsy/structured claims and header precedence.
Generated relocated run `TNYJ4G` compiles both SDKs, runs vendored OIDC tests, and
passes real signed/expired/invalid JWT callbacks, role checks and trusted claims
over HTTP/MCP alongside the Basic/RBAC cases and complete listener/startup/shutdown
matrix. All 940 embedded assets are packaged. Logs:
`/private/tmp/fl-jwt-resource-{tests,parity,clippy,generated,package}.log`.
Broader session-schema/callback and error-envelope work remains open; checklist
remains 657/682 (96.3%), 25 open.


Native worker role receipt (2026-10-13): generated PostgreSQL, Redis, BullMQ and
Kafka executables now accept `--control-service` before opening job storage.
This serves management HTTP without consumers, leases or retention. Tokenless
mode exposes health/readiness only; authenticated controls require a scoped
Redis publisher. The process does not subscribe, stays running after remote drain,
and reports local readiness rather than remote queue health. Subscriber counts
remain delivery fanout counts, not acknowledgements. Management routes expose no
MCP/application SDK methods.

Scaffolding/sync advertise `workerControlService` only when installed sources
support the entry point. New releases carry explicit control/consumer roles;
legacy/custom sources retain their old command. A source-preservation regression
withdraws unsupported metadata and restores it after re-adoption. Companion
platform commit `5774b2bbc` preserves roles through schemas, validates compatible
runtime/component combinations and selects direct argv with `--control-service`
only for the control component.

All 1,101 CLI tests and six worker HTTP tests pass, with strict worker Clippy.
Generated Redis run `sQqS08` verifies real two-consumer fanout, storage-independent
control startup, no extra subscriber, post-drain availability, normal consumer
lease/retry/isolation behavior and relocation/rename/backend-change/delete.
Generated matrix `rw_v_b0r` builds all four backends, starts each control role with
unavailable job storage, checks configuration rejection and graceful SIGTERM,
compiles both SDK targets, and passes the companion service's complete HTTP/MCP
startup/discovery/invocation/shutdown matrix. All 940 embedded assets are packaged.
Platform validation passes 22 native-command/generated-program tests, six schema
tests and the full deployment-worker type check. Logs:
`/private/tmp/fl-worker-control-{tests,clippy,redis-generated,matrix-build,matrix-runtime,matrix-mcp,matrix-sdk,source-capability,package}.log`,
`/private/tmp/fl-worker-role-cli-tests.log`, and
`/private/tmp/fl-platform-worker-role-{generated-tests,schema,worker-types}.log`.
Actual AWS tasks/load balancers/scaling, migration jobs and tenant/runtime-role
provisioning remain unverified. Checklist remains 657/682 (96.3%), 25 open.


HMAC header/verifier-dispatch receipt (2026-10-13):
`typescript::hmac::verify_hmac` adds opt-in TypeScript error ordering, exact prefix,
first-four-component parsing, embedded-equals preservation, ignored suffixes,
configured key lookup and trusted async verification. Missing keys reach the
verifier as absent and must reject there. Successful verification creates no
session or principal. Timestamp text is raw; JavaScript Date conversion, signing,
freshness and replay policy are explicitly outside this dispatch adapter.

The actual middleware dispatch fixture checks 1,320 header/config/verifier cases;
16 existing actual createHmacToken/header-generator comparisons also pass. All 34
auth tests and strict all-feature/all-target Clippy pass. Generated relocated auth
run `inFisa` compiles both SDKs and verifies real HMAC signatures and 60-second skew,
missing keys, malformed credentials and invalid/expired signatures on HTTP/MCP,
alongside Basic/JWT/RBAC and the complete startup/discovery/invocation/shutdown
matrix. All 942 embedded assets are packaged. The dispatch fixture runs in CI.
Logs: `/private/tmp/fl-typescript-hmac-{parity,signatures,tests,clippy,generated,package}.log`.
Remaining session/callback/date and error-envelope boundaries remain open;
checklist remains 657/682 (96.3%), 25 open.


Crate release pipeline receipt (2026-10-13): all 49 public framework, blueprint
and protocol-fork crates now package with versioned internal dependencies. Three
sample crates explicitly disable publication. `scripts/rust_release.py` validates
the complete publication graph and packages the crates plus the pinned literal-key
Serde JSON patch as a separate checksummed artifact. A loopback sparse registry
stages copies with owned dependency registry metadata rewritten; original source
files and release archives remain unchanged. External dependencies use crates.io.

A separate consumer installs all 49 crates from that registry, applies only the
extracted parser artifact as a root path patch, and compiles defaults plus Axum MCP
(protocol-fork defaults disabled). Literal JSON marker-key roundtrips and the full
HTTP/MCP listener/startup/discovery/invocation/Origin/shutdown matrix pass. Nine
regression tests cover graph order, private/unpublishable dependencies, source and
archive integrity, live sparse index metadata and clean commit/tag/receipt/upload
boundaries. The manual publishing workflow defaults to validation only; uploads
require an explicit input, matching versioned tag, clean checkout, successful
receipt and configured credentials. Required parser assets are preserved on an
existing public GitHub release before sequential Cargo uploads.

Logs: `/private/tmp/fl-rust-{registry-prepare,registry-verify,registry-startup,release-tests,release-package}.log`.
All 1,101 CLI tests pass, and all 942 embedded CLI assets remain packaged.
CLI log: `/private/tmp/fl-rust-release-cli-tests.log`. Local validation ran on macOS; the
new Linux workflow has not been executed here. No public crate or GitHub release
was uploaded. Optional adapter runtime checks, public publication/installation and
AWS deployment remain separate gates. Checklist: **658/682 (96.5%), 24 open**.


Ordered session callback receipt (2026-10-13):
`TypeScriptRbacPolicy::authorize_session` accepts trusted request-scoped
`TypeScriptSessionResolver` implementations, validates original verified claims,
then awaits scopes, permissions or roles, subscription and features in middleware
order. Rejections stop subsequent lookups; authenticated/internal access skips
permission/role callbacks. Missing required callbacks return the exact TypeScript
500 messages. Missing scope callbacks retain the upstream skip behavior. Parsed
defaults/transforms never replace the original session. Applications still own
credential verification, identity binding, schema validation and public callback
error formatting.

All 2,335 actual middleware callback-order comparisons pass, including controlled
validator outcomes, original claims, missing callbacks, empty grants, access,
precedence and async failures. All 36 auth tests and strict all-feature/all-target
Clippy pass; native regressions include cancellation without later callbacks.
Generated relocated run `r2lhuP` compiles both SDKs, executes vendored OIDC tests,
and validates real signed-JWT session validation/ordered grants over HTTP/MCP,
alongside existing Basic/JWT/HMAC/RBAC cases and the complete listener/startup/
shutdown matrix. The comparison fixture runs in CI. Logs:
`/private/tmp/fl-typescript-session-{parity,tests,clippy,generated,package}.log`.
Schema-validator equivalence, remaining callback/date boundaries and error
envelopes stay open. Checklist remains **658/682 (96.5%), 24 open**.


Auth rejection wire-format receipt (2026-10-13):
`RouterConfig::auth_error_format` explicitly selects native JSON (the default) or
TypeScript plain-text authentication/authorization errors. `TypeScriptText` sends
the public message with `text/plain; charset=utf-8`; credential/session policies
still determine statuses/messages. Successful responses and request-validation/
handler errors retain their existing formats. JSON retains correlation IDs and
HEAD suppresses bodies in both modes. MCP preserves the rejected HTTP body.

The actual TypeScript/native canonical services pass 46 HTTP outcomes, including
exact missing/invalid/expired/forged JWT rejection bodies, content types and header
parsing. Both OpenAPI snapshots pass with ten explicit document differences still
retained. All 70 Axum unit tests, two MCP integrations, one WebSocket integration
and strict all-feature/all-target Clippy pass. Generated relocated run `szxAHb`
builds both SDKs, executes 11 vendored OIDC tests, verifies Basic/JWT/HMAC/session
policies in both JSON/text error modes and passes the complete HTTP/MCP startup
and shutdown matrix. An initial MCP assertion omitted the standard HTTP reason
phrase; the corrected exact-body assertion passes in the runtime receipt. All
944 embedded sources/templates remain packaged. Logs:
`/private/tmp/fl-auth-text-{canonical-expanded,tests,clippy,generated,runtime,startup,package}.log`.
Broader schema/errors/OpenAPI conformance remains open. Checklist remains
**658/682 (96.5%), 24 open**.


Terminal Stripe cancellation receipt (2026-10-13):
`StripePaymentService::reconcile_canceled_payment` reads authenticated provider
evidence in the configured Connect account. Only canceled intents with positive
cancellation time, zero received/capturable funds, explicit null latest charge
and matching intent/order/amount/currency qualify. It marks the local payment
failed, preserves its reference and atomically audits the operator. It never
mutates provider state or transitions/delivers an order.

`PaymentCancellationStore` implements memory/PostgreSQL snapshot checks and
atomic audit/update. Migration 7 uses forced tenant RLS; runtime roles need SELECT
and operators additionally need INSERT on the new audit table. Confirmation/claim
mutations reject recorded terminal cancellation. Capture and cancellation repairs
reject contradictory prior verified evidence. Existing ordinary failed-payment
confirmation behavior remains unchanged when no verified cancellation exists.

Provider tests reject 16 insufficient/mismatched cancellation responses and verify
read-only requests, Connect binding and secret exclusion. Restricted PostgreSQL
tests pass snapshot rejection, audit-write rollback, tenant isolation, persisted
audit fields, restart and blocked delayed confirmations. Base/Stripe suites and
strict all-feature/all-target Clippy pass. Generated relocated run `Nacuku` executes
the vendored provider/reconciliation tests, builds both SDK targets from both
artifact formats, and passes real HTTP/MCP, tenant/encryption, checkout/provider
webhook, inventory worker and restart checks. Eight actual checkout cases, eleven
actual webhook cases and four native replay/concurrency scenarios pass. All 947
embedded source/template files are packaged. Logs:
`/private/tmp/fl-terminal-payment-{tests,postgres,clippy,generated,package}.log`.

Pending/ambiguous claims, canceled intents with prior charges, PayPal cancellation
and canceled-order references remain open; this narrow repair does not close the
umbrella recovery gate. Checklist remains **658/682 (96.5%), 24 open**.

### 2026-10-13 — Public API source inventory and parse-error formatting

The checked source graph now covers all 44 tracked package manifests, 608 resolved
public type entry points and 1,662 declaration/re-export records. It retains
public method signatures, DTO type aliases, local/external re-export edges,
private module source lists and executable entry points. CI checks drift using
TypeScript 5.9.3 without loading application code. Regression checks cover
wildcard/root-directory resolution, explicitly exported validator test helpers,
packages without root exports, service members, DTOs and private module separation.
This is syntactic coverage; semantic Rust/fixture mappings remain open.

`forklaunch-validation` now exports the string-path `ParseError` DTO and
`pretty_print_parse_errors`. Formatting matches **918 actual TypeScript cases**
for absent/null/empty errors and prefixes, root/nested/empty path segments,
Unicode, control characters, multiline messages, ordering and three-digit error
numbers. It does not claim Zod/TypeBox/Garde validator equivalence.

Six native validation tests and strict all-target/all-feature Clippy pass. Fresh
relocated generated service `WQudHl` builds both SDKs and passes eleven vendored
OIDC tests. Structured session errors pass HTTP and MCP with exact decoded message
checks in JSON and text modes, plus the complete listener/startup/shutdown matrix.
The original fixture compared escaped JSON with literal newlines; that assertion
was corrected and both runtime modes rerun successfully. All **949** embedded Rust
source/template files are packaged, including the new formatter example. Logs:
`/private/tmp/fl-parse-errors-{parity,tests,clippy,generated,runtime,startup,package}.log`.
Checklist remains **658/682 (96.5%), 24 open**.


### 2026-10-13 — Testing environment and token compatibility

`forklaunch-testing` now maps `DatabaseType`, `TestEnvConfig`,
`setupTestEnvironment` and `TEST_TOKENS` to native typed configuration,
`setup_test_environment`, `TestService::with_test_environment` and the shared
mock token constants. Nine database spellings normalize to five families; SQLite
needs no container. Container endpoints expose explicit host/port mappings for
databases, Redis, Kafka and S3. Test-only defaults, custom overrides (including
empty strings and literal prototype-like keys), absent infrastructure and retained
baseline variables match **970 actual TypeScript source comparisons**. Missing
required mappings return an error. Native commands receive an owned environment
map; a real child-process check proves propagation without parent mutation.

Two existing native testing tests, strict all-feature/all-target Clippy, nine
release graph/packaging boundary tests and all **951** embedded source/template
files pass. No dependency was added to the default runtime; example serialization
uses dev dependencies. This does not implement Docker ownership/cleanup, ORM
setup, database clearing or the full blueprint harness. Logs:
`/private/tmp/fl-testing-env-{parity,tests,clippy,package,release-tests}.log`.
Checklist remains **658/682 (96.5%), 24 open**.

### 2026-10-13 — Typed Redis inspection and actual safe-value interop

`JsonCache` now exposes single/batch existence checks and batch deletion without
reading, decoding or decrypting values. Redis existence batches use MULTI/EXEC,
retain order/duplicates/misses, and support lists and corrupt ciphertext. Keys are
validated before sending batch commands. Literal-prefix scans remain separate
from explicit Redis glob scans, with sorted unique SCAN output. Prefix existence
results retain key/result pairs across concurrent disappearance. Native backend,
configuration and raw connection access map the TS configuration/client helpers.

The previous JSON interop fixture substituted JSON.parse/JSON.stringify for
TypeScript's safeParse/safeStringify, hiding top-level string differences. It now
loads both actual common helpers. `JsonRecordCodec::with_typescript_safe_values`
adds an explicit compatibility profile: strings are written verbatim, valid JSON
text parses to its JSON value, and non-JSON text falls back to a string. Strict
JSON remains default. Invalid UTF-8, missing/wrong tenant, bad ciphertext and Rust
type mismatches remain errors. Both envelope formats pass actual Redis tests for
ordinary/JSON-looking/malformed strings, scalar values, arrays/objects, rotation,
expiry, persisted keys, empty/duplicate batches, corrupt/list existence checks,
atomic invalid-key rejection, raw client access and glob/literal-prefix scans.
JavaScript special values/number rounding and falsy queue batch filtering remain.

Three codec unit tests, seven Redis tests including the real backend, strict
all-feature/all-target core/Redis Clippy, nine release checks and **951** packaged
Rust source/template files pass. Added an explicit serde derive dev dependency
for existing core tests that previously relied on workspace feature unification.
Fresh relocated generated infrastructure run `Dq9Tv6` passes ownership/lifecycle,
Redis/S3 binary and encrypted JSON round-trips, typed inspection and safe-string
checks, both SDKs and the full HTTP/MCP startup/shutdown matrix. Its production
codec/Redis sources match the final source. A separate fresh application from the
final rebuilt CLI also verifies the updated core dev-dependency manifest and
exact runtime sources. Logs:
`/private/tmp/fl-json-peek-{parity,clippy,codec-tests,tests,generated,package,release-tests}.log`.
Checklist remains **658/682 (96.5%), 24 open**.

### 2026-10-13 — Explicit TypeScript queue batch semantics

`JsonQueue::peek_records_typescript` and `dequeue_batch_records_typescript` now
apply TS truthiness filtering after authenticated decoding. Null, false, zero and
empty strings are omitted, while empty arrays/objects and literal private-protocol
marker keys survive. Counts apply before filtering. Zero-count dequeues remove
nothing; zero-count peeks use one LRANGE over the full current list. Ordinary
native methods preserve falsy values and retain empty zero-count behavior.
Authentication/type/parse failures propagate and failed peeks leave items intact.
Dequeues do not requeue removed records after a decoding failure. Supported counts
are nonnegative integers; negative/fractional JavaScript inputs are not emulated.

Actual TS Redis source, including its real safeParse/safeStringify functions,
passes both envelope formats, five count boundaries, mixed truthy/falsy values,
FIFO order, remaining lengths, wrong-tenant checks and corrupted-peek retention.
The fixture now builds the native example once and executes it for each case.
Seven native Redis tests including the backend and strict core/Redis Clippy pass.
Fresh relocated run `L0EhpR` validates the compatibility queue methods, encrypted
Redis/S3 factories, both SDKs, CLI ownership/lifecycle and the full HTTP/MCP
startup/shutdown matrix. All **951** embedded source/template files pass packaging.
Logs: `/private/tmp/fl-json-batch-{parity,clippy,tests,generated,package}.log`.
Checklist remains **658/682 (96.5%), 24 open**.


### 2026-10-13 — Optional signed base IAM sessions

Base IAM now supports explicitly configured RS256 bearer issuance via the
`jwt-sessions` feature. Generated modules accept a complete private-key/key-ID/
issuer/audience configuration and reject incomplete or invalid configuration
before listening. Tokens bind tenant/subject, server-resolved roles/permissions,
unique jti, issuer/audience and exact database-clock one-hour expiry. Full-token
SHA-256 storage preserves the local revocation, disabled-user, password-change
and tenant isolation checks. Opaque issuance remains the default. Public-key
hosting, directory identity integration, cookie/CSRF and OAuth/OIDC flows remain
open; downstream stateless JWT consumers require their own current-grant,
revocation and tenant policy.

Two native signer tests, restricted-role PostgreSQL signed and opaque session
suites and strict all-feature Clippy pass. Fresh relocated generated run `Jd8inE`
passes independent Node RSA signature/claim verification, unique issuance,
rejection of valid-but-unstored JWTs, opaque fallback, invalid-config startup
rejection, all directory HTTP/MCP operations, both SDKs, compliance, tenant
isolation, restart/logout, graceful shutdown and rename/delete lifecycle. The
expired directory-JWT fixture now expects the established TypeScript/Rust 403
policy; stored-session failures remain 401. CLI tests pass **1,101**, embedded
source packaging checks **953** assets and release-tool tests pass **9** cases.
No registry upload or external deployment occurred.
Logs: `/private/tmp/fl-iam-jwt-{unit,postgres,opaque,clippy,cli-tests,package,release-tests,generated-final}.log`.
Checklist remains **658/682 (96.5%), 24 open**.


### 2026-10-13 — Base IAM browser cookie/CSRF adapter

Optional `forklaunch-iam-axum/browser-sessions` adds BrowserIam using the existing
shared CookieSessions implementation. Explicit application-owned storage holds
the server-side credential; browser responses contain only the CSRF proof or
current subject/grants/expiry. Login rotates cookie and CSRF IDs and revokes the
prior credential. Logout revokes the IAM session before clearing the cookie.
Current persistent sessions, user state and grants govern each authentication.
The internal `/auth/browser` mount preserves shared HTTP middleware and keeps
browser endpoints out of OpenAPI/SDK/MCP operations. Existing bearer endpoints
retain their separate header requirement. All browser responses are no-store.

The restricted-role PostgreSQL suite passes browser fixation/CSRF/Origin checks,
current-grant changes, disabled users, tenant isolation, cookie replacement,
logout and password-change revocation, alongside the existing bearer suite.
Shared cookie tests pass, including authenticated HTTP/MCP requests. Fixture
schema-permission setup is serialized to avoid PostgreSQL catalog ACL races,
and successful fixtures remove their tenant rows before dropping owned roles.
Fresh relocated run `5EzzDH` passes mounted browser HTTP checks, unchanged 40-tool
MCP startup/discovery, signed/opaque sessions, both SDKs, directory/compliance,
restart/logout and CLI rename/delete. Packaging includes **955** source/template
assets; release-tool tests pass **9** cases and the source API inventory is current.

Tests explicitly use MemoryStore. Reusing it across router instances is not a
process-restart durability claim. Production durable-store generation and OAuth/
OIDC flows remain open; the broader checklist stays **658/682 (96.5%), 24 open**.
Logs: `/private/tmp/fl-iam-browser-{postgres-final,clippy-final,shared,generated,package,release-final,inventory}.log`.


### 2026-10-13 — Generated durable encrypted browser sessions

`PostgresCookieStore` now implements the shared session-store interface using
hashed cookie IDs, encrypted server-side records, tenant RLS and a server-owned
namespace. Sealed payloads bind record ID, namespace and indexed expiry. Collision
retries cannot overwrite an existing session, and updates cannot recreate deleted
or expired cookies. Retained encryption keys support rotation; expiry cleanup is
explicit and tenant/namespace scoped. Debug/errors omit keys and record contents,
and records are bounded to 64 KiB. Migration 3 adds the browser session table.

Generated IAM enables this store only when SESSION_COOKIE_ORIGIN is a valid exact
HTTPS origin. It shares the existing encryption key ring and uses the service name
as namespace; service rename requires browser re-login. Cookie persistence and IAM
writes are separate transactions, so failed cleanup can leave an unreachable
credential until its normal expiry. Operators grant runtime CRUD on the new table
and schedule expiry cleanup. No production MemoryStore fallback is introduced.

Three restricted-role PostgreSQL suites pass bearer/browser lifecycle plus durable
storage isolation, RLS, ciphertext/namespace/expiry swapping, retained-key rotation,
ID collisions, expiry cleanup, size limits and stale-write rejection. Strict IAM/
sample Clippy and workspace formatting pass. Fresh relocated run `2qSPQB` uses the
actual generated PostgreSQL wiring and proves a browser cookie survives process
shutdown/restart, rejects the other tenant and is invalid after logout. It also
passes invalid-origin startup rejection, signed/opaque sessions, both SDKs,
directory/compliance HTTP/MCP operations and CLI rename/delete. Packaging checks
**958** embedded source/template assets; CLI tests pass **1,101**, release-tool
tests pass **9** cases and
the source inventory remains current. Public-key hosting, identity/provider
integration and OAuth/OIDC still have their documented boundaries. No registry
upload or deployment occurred. Checklist remains **658/682 (96.5%), 24 open**.
Logs: `/private/tmp/fl-iam-cookie-store-{postgres,clippy-final,format,generated,package,release,inventory,cli-tests}.log`.


### 2026-10-13 — Stripe canceled intents with verified failed charges

Cancellation reconciliation now accepts an expanded failed charge only when its
intent reference, amount and currency match, paid/captured/refunded/disputed are
explicitly false, and captured/refunded amounts are zero. The parent intent must
still be terminal canceled with a positive cancellation timestamp, zero received/
capturable funds and matching merchant-scoped order identity. Null latest charges
remain supported; unexpanded references, missing evidence, pending/successful,
authorized or refunded charges remain rejected. Repair remains read-only at the
provider and atomically audited locally.

Two provider fixtures pass, including 28 new insufficient-evidence cases, successful
repair, delayed claim release and blocked later confirmation. The restricted-role
PostgreSQL audit/rollback/tenant/restart fixture and strict all-feature Clippy pass.
Evidence: `/private/tmp/fl-stripe-failed-charge-{tests,postgres,clippy}.log`.
Pending/ambiguous claims, paid/authorized prior charges, PayPal cancellation and
canceled-order reference reconciliation remain open. Checklist stays
**658/682 (96.5%), 24 open**.


### 2026-10-13 — ARM64 container and offline ECS smoke artifacts

A fresh generated dependency-free accounts service built using the generated
Dockerfile for Linux ARM64, matching the companion platform's Fargate architecture.
The native process passed the platform's curl health settings, HTTP health/OpenAPI,
MCP initialization/discovery/health invocation and foreign-Origin rejection. SIGTERM
exited cleanly with code zero; its owned container was removed. Image
`forklaunch-rust-ecs-parity:local-validation` and generated workspace
`/private/tmp/fl-rust-ecs-parity-yoqpv_u7` are retained for the pending cloud target.
Logs: `/private/tmp/fl-rust-ecs-image-arm64-build.log` and
`/private/tmp/fl-rust-ecs-local-runtime.log`. This is local container evidence.

`scripts/rust_ecs_smoke_plan.py` now creates reviewable task-definition and service
request JSON offline, with explicit target configuration, digest-pinned ECR image,
ARM64/Fargate/awsvpc, direct native argv, cleared entrypoint and platform health
settings. It requires an exact registered task revision before emitting a usable
service request. Eight tests cover account/region/revision constraints, malformed
network identifiers, unsafe inputs, collisions and actual CLI output. CI runs them.
Resource existence, VPC membership, permissions and connectivity require target
inspection; the tool neither contacts AWS nor provisions resources.

The available AWS account was inspected read-only; its existing unrelated app
cluster was left untouched. A dedicated test target is still awaiting the user's
selection. No image push or AWS mutation occurred. Actual ECS/load balancer,
provisioning, migration and scaling gates remain open; checklist stays
**658/682 (96.5%), 24 open**.


### 2026-10-13 — Explicit OIDC identities and local session issuance

Base IAM migration 4 adds tenant-scoped, case-sensitive issuer/subject bindings
to local users, forced RLS and cascading user deletion. Administrative bind/unbind
APIs require caller authorization, preserve existing ownership and never infer
identity from email. Unbinding prevents new issuance; existing sessions require
separate revocation. OIDC-enabled runtime roles need CRUD on the new identity table.

Optional `oidc-sessions` performs the actual framework provider exchange before
resolving the binding and issuing a persistent opaque or RS256 local credential.
Provider roles/emails grant no local privileges and provider tokens are discarded.
The identity/user lock serializes issuance with unlink, disable and deletion.
Returned sessions reuse the existing local expiry, tenant, current-grant and
revocation policy. The callback caller must consume pending state bound to the
initiating browser; HTTP callback and durable pending-state integration remain open.

Three provider tests pass, including a loopback provider's real discovery/JWKS/
nonce-bound RS256 ID-token exchange, a locally issued RS256 session, wrong-nonce
rejection, absent/same-email bindings, issuer/case/tenant isolation, restart,
revocation, disabled/deleted users, ownership conflicts and RLS. Strict IAM
all-feature/all-target Clippy and the minimal postgres-feature build pass. CI
explicitly executes the ignored PostgreSQL provider fixtures. Evidence:
`/private/tmp/fl-iam-provider-{tests,clippy}.log`. Broader provider/browser and
directory integration remain open; checklist stays **658/682 (96.5%), 24 open**.


### 2026-10-13 — Auth error schemas and safe handler-error wire compatibility

Opt-in TypeScript auth-error formatting now projects missing 401/403 plain-text
OpenAPI schemas on authenticated routes. Inline primitive schemas avoid application
component-name collisions; explicit contracts retain precedence and the runtime
registry is unchanged. Native default documents remain unchanged. Actual-source
canonical OpenAPI differences shrink from ten to six. The six remaining 400/404/
500/null-schema differences remain checked rather than being normalized away.

`HandlerErrorFormat::TypeScriptText` separately formats errors returned by
LocalRouter handlers as the actual TypeScript generic exception text with the
request correlation ID, without returning internal error details. It preserves
the default native JSON format and does not catch panics or rewrite arbitrary
Axum handler responses. The actual Express/native fixture now compares **52 live
HTTP outcomes**, including six thrown-handler outcomes with exact status, media,
text, correlation and secret-redaction checks.

Nine OpenAPI tests, four focused Axum tests, strict all-target OpenAPI/Axum Clippy,
and the live canonical comparison pass. Snapshot verification was repeated
without update mode. Evidence: `/private/tmp/fl-auth-openapi-{tests,clippy,canonical-check}.log`
and `/private/tmp/fl-handler-errors-{tests,clippy,canonical}.log`. Broad validator/
error/document parity remains open: **658/682 (96.5%), 24 open**.


### 2026-10-13 — Narrow PayPal terminal-void repair

`PaypalPaymentService::reconcile_voided_payment` reads authenticated order details
and requires VOIDED/CAPTURE, exactly one matching purchase unit, exact order
reference/amount/currency and explicitly empty captures/authorizations/refunds.
Missing histories, nonterminal states and previous monetary effects still require
review. The method records local failure through existing atomic actor audit,
stale-snapshot and tenant safeguards; it neither mutates provider funds nor
transitions the local order.

`VerifiedCancellation.canceled_at` is now optional because PayPal supplies no
verified cancellation timestamp. Stripe keeps its real timestamp; PayPal records
None. Migration 8 removes only NOT NULL, preserving existing timestamps and the
positive-value check. The separate reconciled_at column records observation time.
Callers using this public DTO must handle optional cancellation time.

Ten nonignored PayPal/base/Stripe tests, two restricted PostgreSQL repair/migration
tests and strict all-feature/all-target Clippy pass. Provider denial fixtures cover
missing/mismatched/ambiguous evidence, and the migration fixture exercises the
actual migration 7→8 while retaining existing Stripe values. Fixtures follow the
[PayPal Orders schema](https://developer.paypal.com/api/orders/v2/schema.json);
these are local provider fixtures, not PayPal sandbox observations. Evidence:
`/private/tmp/fl-paypal-void-{tests,postgres,clippy}.log`. Broad recovery remains
open: **658/682 (96.5%), 24 open**.


### 2026-10-13 — Request error envelopes and explicit empty-response documentation

`typescript_request_validation_error_response` formats caller-provided public
ParseErrors with TypeScript's Request prefix, paths/messages and correlation ID.
The final default Express 400 response uses plain text: although its parse
middleware initially selects JSON media, the enriched send applies the default
string response contract. Live testing caught this distinction before completion.
Explicit response declarations can override media. Native validator diagnostics
and default structured errors remain separate; Zod message equivalence is not
claimed by this formatter.

The TypeScript OpenAPI emitter now omits content for explicit undefined/void
responses. Null/nullish retain JSON body documentation and no-body responses
retain declared headers. Thirty actual Zod/TypeBox emitter cases pass, and the
existing live empty-404 case confirms the corrected document matches the wire.
The canonical document comparison now retains **five** real differences.

Validation passes **918** formatting cases, **25** actual request middleware
body/status comparisons, six native tests, strict validation Clippy, and **60**
live HTTP outcomes including eight request-validation outcomes. Snapshot checking
passes with update mode disabled. Evidence:
`/private/tmp/fl-request-errors-{parity-final,clippy-final,tests,canonical-check}.log`
and `/private/tmp/fl-openapi-no-body.log`. Checklist remains
**658/682 (96.5%), 24 open**.


### 2026-10-13 — Cancelled-order payment-reference review

`review_cancelled_order_payments` provides a tenant-bound read-only report with
1–100-entry keyset pages. It requires a cancelled order and distinguishes missing
references, unverified pending/claim-or-capture/failed states, recorded capture
or cancellation evidence, and contradictory evidence. A local Succeeded value
alone is not treated as proof of external capture. Every record/reference is
preserved; the report authorizes no refund, reference reuse or order reopening.

Memory holds record/audit locks together. PostgreSQL uses one MVCC statement per
page, selecting order status and payment fields without decrypting order PII.
Historical evidence must match payment ID/reference/amount/currency, and conflicting
provider names or capture/cancellation/status evidence is flagged. Existing audit
schemas cannot bind a historical merchant account or distinguish delete/recreate
with an identical tuple, so results remain historical evidence, not fresh provider
truth. Each page is independent; concurrent insertions before a cursor require
restarting the scan.

The shared memory/restricted PostgreSQL fixtures cover all classifications, empty/
missing/noncancelled orders, tenant isolation, pagination, stale replacement
references, amount mismatch and no PII output. One memory test, two PostgreSQL
tests (including timestamp migration regression) and strict all-feature/all-target
Clippy pass. Evidence: `/private/tmp/fl-cancelled-order-review-{tests,postgres,clippy}.log`.
Gate875 remains open for unresolved provider effects and reference reconciliation;
checklist stays **658/682 (96.5%), 24 open**.


### 2026-10-13 — Completed configured base IAM browser OIDC flow

The generated base IAM service now supports an explicitly configured standard
OIDC authorization-code/S256 PKCE flow. SESSION_OIDC_ISSUER/CLIENT_ID and optional
CLIENT_SECRET require the existing exact HTTPS cookie origin. Discovery and
configuration validation precede listening. Explicit tenant/issuer/subject bindings
resolve existing local identities; provider emails and roles cannot grant access.

Migration 5 adds encrypted tenant/namespace/browser-bound pending state. It has
absolute ten-minute expiry, one current flow per browser and atomic consumption
before provider exchange. Cookie deletion cascades pending rows. Callbacks survive
process restarts, rotate cookie/CSRF IDs and use existing local session revocation.
A static CSP-hashed callback page removes code/state from the URL, makes same-origin
CSRF-protected requests and navigates only to `/`. Callback GET never creates or
replaces a cookie. Public raw endpoints remain outside SDK/MCP operation metadata.

Validation passes four native PostgreSQL HTTP suites, four provider/pending tests
(including a claim race on independent restricted connections), twelve framework
OIDC tests, strict IAM/framework Clippy, browser-only feature compilation and VM
relay checks. Real Chromium separately proves cross-site Strict-cookie exclusion,
same-origin cookie binding, CSP execution, HttpOnly behavior, no Referer leakage
and rejection without the initiating cookie.

Fresh generated run `5zxkME` passes real discovery/JWKS/PKCE/code exchange through
the generated executable, pending-state survival across a process restart,
server-resolved grants, cookie/CSRF rotation, replay denial, logout and HTTP/MCP
startup. Existing password/opaque/RS256, tenant/encryption, directory/compliance,
both SDKs and source-preserving rename/delete lifecycle checks pass as well.
Packaging checks 974 embedded sources/templates; nine release-tool tests pass.
The owned generated workspace is removed after this receipt. Evidence:
`/private/tmp/fl-iam-browser-oidc-*.log`,
`/private/tmp/fl-iam-oidc-{generated,browser-relay,package,release}.log`.

This closes the configured base IAM provider-flow gate: **659/682 (96.6%), 23 open**.
It does not close directory credential integration, broader Better Auth social/
linking/adapter work, deployment or public registry gates. Base login discards
provider access/refresh tokens; downstream provider APIs need their own token
lifecycle. Generic provider refresh remains available in the framework.


### 2026-10-13 — Final Express parser error profile after main merge

`ParserErrorFormat::TypeScriptText` is opt-in for typed parser failures and
transport body limits. It matches main's final Express response: safe 4xx/5xx
text, UTF-8 media type and correlation ID, without raw parser input or private
verifier details. Native JSON defaults and ordinary schema diagnostics remain
unchanged. Both LocalRouter and Axum versioned routes are covered; Content-Length
and streamed body limits remain bounded. Arbitrary adapter errors and untyped
handler parser policies are outside this profile.

Three Axum matrix tests, seven HTTP tests and strict all-target Clippy pass.
The actual Express/Rust canonical fixture passes **88 live HTTP outcomes**,
including 28 new parser comparisons: malformed/strict-null JSON, parser and
transport limits, unsupported charset, verifier failure, correlation and no
handler execution after rejection. Checked OpenAPI snapshots remain enabled
without update mode; the same **five** real document differences remain.
Evidence: `/private/tmp/fl-parser-errors-{tests-final,clippy-final,canonical}.log`.
Gate806 stays open; checklist remains **659/682 (96.6%), 23 open**.


### 2026-10-13 — Verified interrupted checkout completion

The Stripe billing crate's opt-in `postgres-reconciliation` feature adds
`reconcile_checkout_webhook` for interrupted checkout completed/expired events.
Two authenticated GETs bind the pending event ID/type to the current subscription
checkout session, customer, live/test mode, terminal state and already-applied
SUCCESS/FAILED metadata. Connected-account/context events are rejected. The API
never repeats a provider mutation or treats checkout completion as payment
settlement.

Local repair requires an actor and existing encrypted local ownership matching
the verified provider session/customer. An opaque snapshot binds tenant, exact
pending attempt, encrypted cache/backup contents and row versions. Under the
event lock, compare-and-swap rejects stale state; backup status changes, inspected
cache removal, reconciliation audit, receipt and attempt removal commit together.
Absent ownership, unavailable provider events, missing required backup, inconsistent
statuses and other subscription/plan/payment-link/portal effects stay unresolved.

Validation passes two provider evidence/wire tests, 32 provider-evidence denials,
the restricted PostgreSQL suite, all 12 billing regression tests and strict
all-feature/all-target Clippy. PostgreSQL checks cover failures after local writes
and receipt insertion, rollback, same-content row replacement, stale attempts,
tenant/customer mismatch, concurrent one-winner completion, reopened receipt
visibility, backup-disabled expiry and partially completed backup updates.
Evidence: `/private/tmp/fl-billing-checkout-repair-{provider,postgres,clippy,regression}.log`.
Gate858 remains open; checklist stays **659/682 (96.6%), 23 open**.


### 2026-10-13 — Main integration and exact-session Better Auth authorization

Main is merged through `c5a4248d1` in merge commit `ac1f18e2a`, preserving the
existing adapter integration and main's auth encryption policy. All **1,120 CLI
tests** pass. The regenerated API inventory and regression check pass at
44 packages, 608 entry points and 1,662 declaration/re-export records.

Native application authorization now uses validated user/organization/session
scope from verified claims, checks that exact unexpired session and current
membership, and never selects a different session. Authenticated issuance adds
the persisted `sessionId`, including external signing. Old low-level helpers
without session scope confer no scoped privileges. Two new HMAC routes sign
all three scope components; legacy user-only routes return empty grants. Builtin
roles supply configured platform permissions; custom role values remain opaque.
Seven application contracts are exposed alongside the 70 raw auth routes.

Session primary/foreign-key comparisons follow PostgreSQL UUID case semantics;
organization/member text links retain exact matching. Actual PostgreSQL tests
confirm uppercase session IDs work, uppercase organization/user text membership
links do not broaden access, and wrong sessions fail. Updated CI pins the actual
MikroORM adapter at 0.5.11. Service-bound account/key encryption is verified with
context restoration, legacy marker rejection, encrypted columns, transaction
rollback and restart. Decoded real ORM records are consumed by native auth;
this does not assert interchangeable encrypted storage schemas.

Validation passes **52** actual TypeScript authorization comparisons, **7**
application contracts / **13** response schemas, **26** JWT/JWKS comparisons,
**49** configured protocol checks, **41** real PostgreSQL/ORM/native checks, two
native scope tests, 20 native HTTP/MCP tests and the final five-mount application
fixture. Strict all-feature/all-target Clippy passes. Rebuilt CLI and packaging
checks cover 977 embedded sources/templates. The generated scoped-route/runtime
rerun is in progress and will have its own receipt. Evidence:
`/private/tmp/fl-better-auth-scoped-{unit,http,application-final,conformance,contracts,jwt,reference,clippy}.log`,
`/private/tmp/fl-rust-scoped-orm.log`, `/private/tmp/fl-rust-main-cli-tests.log`,
`/private/tmp/fl-rust-main-package.log`. Broader Better Auth options/hooks and
other parity gates remain open: **659/682 (96.6%), 23 open**.


### 2026-10-13 — Generated scoped auth validation and adapter gate reconciliation

Generated Better Auth run `AO2PeS` passes scoped HMAC roles/permissions through
HTTP, the typed SDK and MCP; changed scope invalidates the signature, wrong
sessions return no grants and legacy routes remain empty with active ownership.
Persisted session IDs appear in signed JWTs. All eight actual frontend-client
profiles, shared/separate/disabled MCP, custom/root/URL-overridden auth namespaces,
self-JWKS, encrypted tenant isolation, restart and shutdown pass. Both SDKs build;
source-preserving rename, artifact checking, workspace compilation and deletion
pass. A stale six-tool assertion was corrected to eight; runtime/lifecycle checks
were rerun in the same isolated generated workspace with a fresh database role.
Evidence: `/private/tmp/fl-rust-scoped-generated{,-resume}.log`.

Review of the auth evaluation's selected adapter scope also closes its stale
integration recommendation. Both reviewed adapters now have optional features,
locked dependency review and the generated session/OIDC evidence above. Broader
framework adoption was deliberately deferred by the evaluation; passkeys/custom
provider options remain in broader IAM parity work. No extra framework is adopted
merely to close a checkbox. Checklist: **660/682 (96.8%), 22 open**.
