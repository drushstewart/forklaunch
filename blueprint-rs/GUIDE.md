# blueprint-rs guide

Blueprint development and operations reference for the current source workspace.

## Crate catalog

The source links expose the public API. Build the searchable Rust API reference
with the commands below. Feature names come from each crate's `Cargo.toml`; enabling
a feature makes its API available but does not configure credentials or start a service.

| Crate | Responsibility | Optional features and defaults |
| --- | --- | --- |
| [forklaunch-blueprint-core](crates/forklaunch-blueprint-core/src/lib.rs) | Shared identifiers, pagination, audit/error types and managed provider clients. | `instance-gateway`, `stripe-webhook`, `provider-http`; default: none |
| [forklaunch-iam-interface](crates/interfaces/forklaunch-iam-interface/src/lib.rs) | IAM identities, sessions, roles, permissions and service DTOs/traits. | —; default: none |
| [forklaunch-cac-interface](crates/interfaces/forklaunch-cac-interface/src/lib.rs) | Code-set/category contracts and service types. | —; default: none |
| [forklaunch-messaging-interface](crates/interfaces/forklaunch-messaging-interface/src/lib.rs) | Messaging contracts and service/provider types. | —; default: none |
| [forklaunch-ecommerce-interface](crates/interfaces/forklaunch-ecommerce-interface/src/lib.rs) | Product, variant, inventory, cart, order and payment contracts/types. | —; default: none |
| [forklaunch-billing-interface](crates/interfaces/forklaunch-billing-interface/src/lib.rs) | Customer, plan, subscription, checkout and billing contracts/types. | —; default: none |
| [forklaunch-worker-interface](crates/interfaces/forklaunch-worker-interface/src/lib.rs) | Queue/job, lease, administration and handler types. | —; default: none |
| [forklaunch-monitoring-interface](crates/interfaces/forklaunch-monitoring-interface/src/lib.rs) | Monitoring contracts/types; does not scaffold a standalone module. | —; default: none |
| [forklaunch-iam-base](crates/implementations/iam/forklaunch-iam-base/src/lib.rs) | Directory/auth service implementations and optional PostgreSQL persistence. | `postgres`; default: none |
| [forklaunch-iam-better-auth](crates/implementations/iam/forklaunch-iam-better-auth/src/lib.rs) | Native configured Better Auth behavior, encrypted store, HTTP and relay. | `http`, `postgres`, `application-http`; default: none |
| [forklaunch-cac-base](crates/implementations/cac/forklaunch-cac-base/src/lib.rs) | Code-set service implementations. | —; default: none |
| [forklaunch-messaging-base](crates/implementations/messaging/forklaunch-messaging-base/src/lib.rs) | Persistent messaging lifecycle, callbacks and store seams. | `postgres`; default: none |
| [forklaunch-ecommerce-base](crates/implementations/ecommerce/forklaunch-ecommerce-base/src/lib.rs) | Atomic local commerce storage/business rules, compliance and retention. | `worker`, `postgres`; default: none |
| [forklaunch-ecommerce-axum](crates/implementations/ecommerce/forklaunch-ecommerce-axum/src/lib.rs) | Commerce HTTP/MCP contracts and request mapping. | —; default: none |
| [forklaunch-ecommerce-stripe](crates/implementations/ecommerce/forklaunch-ecommerce-stripe/src/lib.rs) | Stripe checkout/capture/webhook integration. | —; default: none |
| [forklaunch-ecommerce-paypal](crates/implementations/ecommerce/forklaunch-ecommerce-paypal/src/lib.rs) | PayPal checkout/capture integration. | —; default: none |
| [forklaunch-messaging-twilio](crates/implementations/messaging/forklaunch-messaging-twilio/src/lib.rs) | Twilio transport and callback integration. | —; default: none |
| [forklaunch-messaging-axum](crates/implementations/messaging/forklaunch-messaging-axum/src/lib.rs) | Messaging HTTP/MCP contracts and request mapping. | —; default: none |
| [forklaunch-cac-axum](crates/implementations/cac/forklaunch-cac-axum/src/lib.rs) | CAC HTTP/MCP contracts and request mapping. | —; default: none |
| [forklaunch-iam-axum](crates/implementations/iam/forklaunch-iam-axum/src/lib.rs) | Directory IAM HTTP/MCP contracts and request mapping. | —; default: none |
| [forklaunch-billing-base](crates/implementations/billing/forklaunch-billing-base/src/lib.rs) | Billing services, store seams and optional PostgreSQL persistence. | `postgres`; default: none |
| [forklaunch-billing-stripe](crates/implementations/billing/forklaunch-billing-stripe/src/lib.rs) | Stripe provider and optional remote client. | `stripe-client`; default: none |
| [forklaunch-billing-axum](crates/implementations/billing/forklaunch-billing-axum/src/lib.rs) | Billing HTTP/MCP contracts and request mapping. | —; default: none |
| [forklaunch-worker-database](crates/implementations/worker/forklaunch-worker-database/src/lib.rs) | Database queue, shared runtime/control/retention and PostgreSQL adapter. | `http-control`, `runtime`, `postgres`; default: none |
| [forklaunch-worker-redis](crates/implementations/worker/forklaunch-worker-redis/src/lib.rs) | Native leased Redis queue and optional official BullMQ protocol adapter. | `bullmq`, `redis-client`; default: none |
| [forklaunch-worker-kafka](crates/implementations/worker/forklaunch-worker-kafka/src/lib.rs) | Kafka worker interfaces and optional broker adapter. | `kafka-client`; default: none |
| [forklaunch-blueprint-client-sdk](crates/client-sdk/src/lib.rs) | SDK generation from blueprint contract registries. | —; default: none |
| [forklaunch-blueprint-sample-app](crates/sample-app/src/lib.rs) | Reference application composition. | —; default: none |
| [forklaunch-blueprint-sample-service](crates/sample-service/src/lib.rs) | Reference service and conformance examples. | `postgres`; default: none |
| [forklaunch-blueprint-sample-worker](crates/sample-worker/src/lib.rs) | Reference worker composition. | —; default: none |

## Choose a generated module

The [generation guide](../docs/adding-projects/rust.md) covers application init,
adding modules, migrations, development, SDKs and mixed-language lifecycle.
Available presets are `iam-base`, `iam-better-auth`, `cac-base`, `messaging-base`,
`messaging-twilio`, `ecommerce-stripe`, `billing-base` and `billing-stripe`.
Relay extends an existing Better Auth service; it is not a new standalone
service. Storefront capture similarly has its own lifecycle outside Cargo APIs.

Interfaces own contracts, DTOs and service traits; base crates implement local
business behavior and storage seams; provider crates implement external effects;
Axum crates connect those services to registered HTTP/MCP operations. Choose the
implementation explicitly. Reference memory stores are useful for tests but do
not establish production durability or cross-process isolation.

## IAM and Better Auth

Directory IAM and Better Auth are different implementations. The
[PostgreSQL IAM guide](../docs/adding-projects/rust.md#postgresql-iam) documents the
base module's runtime environment. Directory CRUD does not turn an ignored
password field into a credential: provider integration and generic mapper/filter
extensions remain tracked separately. Use the authentication service's credential
and session lifecycle rather than inserting directory records to simulate login.

The [Better Auth adapter guide](crates/implementations/iam/forklaunch-iam-better-auth/README.md)
is the reference for its configured endpoints, cookies, encrypted PostgreSQL,
Google flow, callbacks, key rotation, compliance/retention and relay hooks.
Registered application routes are exported to SDKs/MCP; raw auth routes remain
in their reserved HTTP namespace. Configure trusted origins and the auth base URL
explicitly. Broader plugin/options/client behavior is not implied by the
configured adapter's passing conformance suite.

For relay, add `init module --language rust -m relay` to the application path.
Customize the preserved `src/relay_hooks.rs` to resolve the owner and optionally
mint app claim links. The safe default signs in nobody. Run migrations before
starting the new endpoints; signed ingest and claim operations are available
through HTTP/MCP and generated SDKs, while the browser handoff remains HTTP-only.

## CAC and messaging

[CAC](../docs/adding-projects/rust.md#cac-module) exposes code-set operations through
the service interface and Axum contract adapter. Keep custom route registration
in application source so exported contracts and serving behavior agree.

[Messaging](../docs/adding-projects/rust.md#sms-messaging-modules) separates durable
local message state from the provider transport. Configure the base or Twilio
preset and its callback credentials explicitly. Provider acceptance and final
delivery are different states; process authenticated callbacks through the
persistent lifecycle. Managed capability clients in `forklaunch-blueprint-core`
use the instance gateway; see the [capability configuration guide](../docs/adding-projects/rust.md#managed-capabilities).
Provider HTTP transport, Stripe webhook verification and gateway clients each
have independent feature gates.

## Billing and ecommerce

Billing services compose customer, subscription, plan and checkout storage with
an optional Stripe provider. Configure provider credentials and the local store
before mounting the Axum adapter. The [compatibility baseline](../docs/adding-projects/rust-compatibility.md)
and [parity inventory](../plan/RUST-TYPESCRIPT-PARITY-INVENTORY.md) record configured
mapper differences and timestamp/status quirks. External provider effects and
local transactions are not atomically committed together. Interrupted plan
replacement/provider writes and receipt reconciliation remain open: do not claim
exactly-once external financial effects from a successful local transaction.

Ecommerce splits product/variant/inventory/cart/order/payment behavior from
Stripe and PayPal capture. See [provider setup and boundaries](README.md#ecommerce-providers).
The base store owns tenant isolation, local concurrency and durable event state;
provider retries require the documented provider references and idempotency
behavior. Interrupted external capture and cancelled-order reference
reconciliation remain tracked work. Inspect local and provider state before
repeating an uncertain capture.

Ecommerce compliance exports classified order addresses as protected values;
explicit disclosure requires an audit callback. Subject mapping defaults to the
actual configured TS entity-ID behavior; choose customer-ID mapping explicitly
when the application requires it. Required addresses are not nullable, so an
anonymization pass records completion without clearing them. No retention period
is enabled implicitly. Retention and erasure are tenant-scoped and atomic in the
PostgreSQL adapter. See the compliance and retention sections in [the README](README.md).

## Worker execution and shutdown

The shared database worker runtime coordinates bounded concurrency, lease
renewal, retries, administration, optional retention and graceful drain. Native
Redis leases and official BullMQ are distinct wire protocols; use `bullmq` for
actual TS BullMQ interoperability. Kafka uses its broker adapter and explicit
acknowledgment behavior. Use the matching implementation's feature gate from the
catalog, not a generic memory worker in production.

The [worker guide](../docs/adding-projects/rust.md#postgresql-worker-queue) covers
database, Redis, BullMQ and Kafka startup. The
[administration guide](README.md#worker-administration) covers dead letters,
requeue, purge, pause/resume, drain and retention. Generated worker HTTP controls
are optional and bearer-authenticated; they are not the service MCP listener.
Automatic retention is disabled unless configured. Keep handler side effects
idempotent because a crash or expired lease can cause redelivery. Graceful drain
stops acquiring new work and lets the configured active work/shutdown policy run.

## Persistence and application ownership

Run migrations using an administrative connection, then start services with
restricted tenant-aware connections and required audit grants. Keep encryption
keys and service credentials outside generated source. Startup failure after an
upgrade commonly needs a migration, a missing runtime secret, or a corrected
restricted-role grant; bypassing RLS is not a repair.

Sync and module reinitialization preserve user-editable Rust source. Review
embedded library changes separately from application hooks, provider policy and
custom routes. Regenerate contracts and both SDK targets after route changes.
Generated module fixtures exercise relocation, rename and deletion; preserve the
application's own tests as well. Application hook correctness remains the
application's responsibility even when the generic scaffold passes its fixtures.

## Build API docs and run conformance

From the repository root:

```sh
CARGO_INCREMENTAL=0 RUSTDOCFLAGS='-D warnings' cargo doc --manifest-path blueprint-rs/Cargo.toml --workspace --all-features --no-deps
cargo test --manifest-path blueprint-rs/Cargo.toml --workspace --all-features
cargo clippy --manifest-path blueprint-rs/Cargo.toml --workspace --all-targets --all-features -- -D warnings
```

Open `blueprint-rs/target/doc/forklaunch_blueprint_core/index.html` and select a
crate to browse its API. Real PostgreSQL/Redis/Kafka/provider-fixture checks need
the environment specified in `cli/tests/rust_*` scripts; memory tests and ignored
integration tests do not replace those checks. See
[validation commands](../docs/adding-projects/rust.md#validate-the-port),
[compatibility](../docs/adding-projects/rust-compatibility.md), and the
[implementation checklist](../plan/RUST-FRAMEWORK-BLUEPRINT-IMPLEMENTATION.md) for
remaining gates. Publishing and ECS deployment are not established by a local
doc, source-archive or container build.
