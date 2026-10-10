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
| [`@forklaunch/core`](../framework/core/package.json) | forklaunch-core, forklaunch-http, forklaunch-auth, forklaunch-contract, forklaunch-mcp | HTTP/contracts/config/DI/cache/object store/telemetry/encryption exist; SQL entity/compliance/retention services and full auth/streaming conformance remain. |
| [`@forklaunch/e2e-tests`](../framework/e2e-tests/package.json) (generated/private) | Rust wire/runtime suites | Actual TypeScript framework service conformance remains beyond the plain Node wire fixtures. |
| [`@forklaunch/express`](../framework/express/package.json) | forklaunch-axum | Equivalent HTTP behavior is the target; multipart/SSE and full validation/auth/error fixtures remain. |
| [`@forklaunch/hyper-express`](../framework/hyper-express/package.json) | forklaunch-axum | All 27 exported HTTP methods dispatch through shared validation/auth and nested contracts; both SDK artifact sources and actual compiled TypeScript router token forwarding are covered. Runtime-specific adapter internals need no clone. |
| [`@forklaunch/infrastructure-s3`](../framework/infrastructure/S3/package.json) | forklaunch-infrastructure-s3 | Async store, bounded streaming/multipart transfers, codecs, signed downloads and browser POST policies are implemented with real MinIO fixtures. POST size/type/key/expiry enforcement, AWS signature vectors, SDK credential cache/rotation and actual TypeScript policy fixtures are covered; ordinary SigV4 bucket endpoints only. |
| [`@forklaunch/infrastructure-redis`](../framework/infrastructure/redis/package.json) | forklaunch-infrastructure-redis | Async cache/queue/pubsub and real fixtures exist; audit remaining provider-specific options and typed codecs. |
| [`@forklaunch/internal`](../framework/internal/package.json) | contract macros, CLI embedded templates and Rust tooling | Map remaining exported build/runtime helpers; JavaScript compiler utilities remain in the TypeScript path. |
| [`@forklaunch/testing`](../framework/testing/package.json) | forklaunch-testing and runtime fixtures | Local calls and response helpers exist; actual cross-language conformance harness remains. |
| [`@forklaunch/universal-sdk`](../framework/universal-sdk/package.json) | forklaunch-sdk-rust, forklaunch-sdk-typescript, forklaunch-sdk-codegen and CLI SDKs | HTTP targets exist; multipart/SSE, recursive schemas, typed realtime and one mixed SDK surface remain. |
| [`@forklaunch/validator`](../framework/validator/package.json) | forklaunch-validation and schema contracts | JSON Schema validation/coercion exists; audit Zod/TypeBox exported behaviors against shared fixtures. |
| [`@forklaunch/ws`](../framework/ws/package.json) | forklaunch-ws, forklaunch-asyncapi | Envelope/AsyncAPI foundations exist; typed server/client wrappers and generated clients remain. |
| [`@forklaunch/blueprint-billing-base`](../blueprint/billing-base/package.json) (generated/private) | Missing generated module | Wire persistent base services/routes/contracts, compliance and CLI lifecycle. |
| [`@forklaunch/blueprint-billing-stripe`](../blueprint/billing-stripe/package.json) (generated/private) | Missing generated module | Wire Stripe provider/base persistence/routes/webhooks and CLI lifecycle. |
| [`@forklaunch/blueprint-cac-base`](../blueprint/cac-base/package.json) (generated/private) | forklaunch-cac-interface/base/axum and Rust init module cac-base | Generated module mounts HMAC lookup and JWKS/system-role compliance; no entities are registered, so authorized export/erase return upstream 404s. Uses verified claims or a trusted role resolver in place of the TS admin-only stub. Mixed coexistence, both SDKs, upgrade, rename/sync/delete and installed source archives pass. |
| [`@forklaunch/blueprint-client-sdk`](../blueprint/client-sdk/package.json) (generated/private) | forklaunch-blueprint-client-sdk and CLI SDK generation | Native SDK prototypes and generated HTTP packages exist; all added modules/mixed surfaces remain. |
| [`@forklaunch/blueprint-core`](../blueprint/core/package.json) (generated/private) | forklaunch-blueprint-core and framework crates | Core DTOs/contracts exist; compliance/service registrations and shared role/provider behavior remain. |
| [`@forklaunch/blueprint-ecommerce-stripe`](../blueprint/ecommerce-stripe/package.json) (generated/private) | Missing generated module | Catalog/cart/order/payment/subscription routes, provider wiring, compliance and CLI lifecycle remain. |
| [`@forklaunch/blueprint-iam-base`](../blueprint/iam-base/package.json) (generated/private) | Rust init module iam-base | Generated persistent session module exists; additional TS IAM routes and compliance remain. |
| [`@forklaunch/blueprint-iam-better-auth`](../blueprint/iam-better-auth/package.json) (generated/private) | Missing enabled-provider parity | Audit actual Better Auth configuration, JWT/cookies/CSRF, social/OIDC/passkey/plugins and generated SDKs. |
| [`@forklaunch/implementation-billing-base`](../blueprint/implementations/billing/base/package.json) | forklaunch-billing-base | Reference in-memory service only; persistent customer/plan/subscription/invoice/usage services remain. |
| [`@forklaunch/implementation-billing-stripe`](../blueprint/implementations/billing/stripe/package.json) | forklaunch-billing-stripe | Signature verification/reference service exists; actual Stripe client, checkout and subscription sync remain. |
| [`@forklaunch/implementation-cac-base`](../blueprint/implementations/cac/base/package.json) | forklaunch-cac-base | Mock codes and provider descriptor/lookup match actual TS fixtures; authenticated generated module, both SDKs and CLI lifecycle now pass. |
| [`@forklaunch/implementation-ecommerce-base`](../blueprint/implementations/ecommerce/base/package.json) | Missing | Product, variant, inventory, cart, order and payment services/DTOs/persistence/concurrency remain. |
| [`@forklaunch/implementation-ecommerce-paypal`](../blueprint/implementations/ecommerce/paypal/package.json) | Missing | PayPal payment/provider behavior and fixtures remain. |
| [`@forklaunch/implementation-ecommerce-stripe`](../blueprint/implementations/ecommerce/stripe/package.json) | Missing | Stripe payment resume/Connect fee/confirmation/failure/webhook behavior remains. |
| [`@forklaunch/implementation-iam-base`](../blueprint/implementations/iam/base/package.json) | forklaunch-iam-base, forklaunch-iam-axum | Persistent scoped users/organizations/permissions/sessions exist; full TS user/role/permission CRUD and provider flows remain. |
| [`@forklaunch/implementation-messaging-base`](../blueprint/implementations/messaging/base/package.json) | forklaunch-messaging-base | Hooks-only sent behavior, encrypted tenant Postgres persistence, audited reads, subject export/erasure and key rotation; actual TS service fixtures pass. |
| [`@forklaunch/implementation-messaging-twilio`](../blueprint/implementations/messaging/twilio/package.json) | forklaunch-messaging-twilio | Actual TS REST/status/callback/error conformance; bounded single-attempt dispatch, signed form callbacks and generated SDK/runtime fixtures. |
| [`@forklaunch/implementation-worker-bullmq`](../blueprint/implementations/worker/bullmq/package.json) | Native Redis worker covers some behavior; wire compatibility missing | Audit actual options/retry/retention/waiting+active inspection/tenant ACL and decide explicit interop protocol. |
| [`@forklaunch/implementation-worker-database`](../blueprint/implementations/worker/database/package.json) | forklaunch-worker-database | Persistent lease/runner/retry/dead-letter/retention/control suites exist; public option and cross-language DB-wire audit remains. |
| [`@forklaunch/implementation-worker-kafka`](../blueprint/implementations/worker/kafka/package.json) | forklaunch-worker-kafka | Transactional native producer/consumer/admin and generated/Linux suites exist; TS Kafka event-array envelope interoperability remains. |
| [`@forklaunch/implementation-worker-redis`](../blueprint/implementations/worker/redis/package.json) | forklaunch-worker-redis | Persistent lease/runner/admin/control suites exist; native protocol differs from TS, so interoperability remains explicit. |
| [`@forklaunch/interfaces-billing`](../blueprint/interfaces/billing/package.json) | forklaunch-billing-interface | Core billing DTOs exist; all TS CRUD/update/provider fields remain to map. |
| [`@forklaunch/interfaces-cac`](../blueprint/interfaces/cac/package.json) | forklaunch-cac-interface | Provider interface/DTOs, code-set and compliance contracts, authenticated runtime, generated module and both SDKs are validated. |
| [`@forklaunch/interfaces-ecommerce`](../blueprint/interfaces/ecommerce/package.json) | Missing | All catalog/inventory/cart/order/payment DTOs and interfaces remain. |
| [`@forklaunch/interfaces-iam`](../blueprint/interfaces/iam/package.json) | forklaunch-iam-interface | Typed core/session DTOs exist; full TS interface method/DTO mapping remains. |
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
