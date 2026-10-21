# Notes: generated Rust application for local review

This application combines Rust services and an Apalis worker with Better Auth,
a browser UI, PostgreSQL, Redis and object storage. The CLI composes ordinary
projects and adds Notes business code. Generation does not start or deploy it.
Consult the validation receipt for the exact snapshot that has been tested.

## Run locally

```sh
python3 scripts/setup.py
docker compose up --build -d
docker compose cp gateway:/data/caddy/pki/authorities/local/root.crt ./local-ca.crt
```

Trust the development CA locally, then open `https://localhost:8443`. Alice
(`alice@example.test`) can edit the shared workspace; Bob (`bob@example.test`)
can view it. The demo password is `DEMO_PASSWORD` in the private `.env` file.
Each account also has a private workspace. Create a note, request an insight,
and open a second tab to see live updates. Stale revisions return a conflict.

`docker compose logs -f notes insights` shows service and worker activity.
`docker compose down` stops this project while preserving its data. Keep `.env`
with the data: losing its encryption key prevents recovery. Adding `-v` deletes
this project's persistent volumes.

Only the HTTPS gateway is published, bound to loopback. Internal development
connections use plaintext HTTP/PostgreSQL/Redis; their explicit local exceptions
must be removed when configuring remote TLS endpoints. Application encryption
protects note content, jobs, results and attachments at rest. Tenant identifiers,
revisions, routing metadata and timestamps remain visible. The API and worker
are trusted plaintext processors; this is not end-to-end encryption.

## Structure and ownership

```text
src/
  core/security/       # application identity, roles and encryption policy
  notes/               # independent Cargo service and owned migrations
    api/{contracts,controllers,routes}/
    domain/{schemas,types,services,mappers}/
    persistence/
    migrations/
    application.rs     # application route/state/command assembly
    registrations.rs
    sdk.rs
    build_assets.rs    # additional presence assets
  insights/            # independent Cargo Apalis worker
    domain/
    persistence/
    migrations/
    worker_hooks.rs    # typed backend registration and business hooks
    build_assets.rs    # job/result channel assets
  auth/                # Better Auth TypeScript service and owned migrations
  client-sdk/{typescript,rust,events}/
web/                   # browser application
Dockerfile.rust        # Rust services, worker and compile-generated SDK assets
Dockerfile.typescript  # TypeScript auth and web/gateway targets
compose.yaml           # per-service language Dockerfile and build dependencies
infra/                 # local provisioning and gateway configuration
```

`lib.rs` and `mod.rs` declare exports. `main.rs`, `bootstrapper.rs`, `server.rs`,
`worker.rs` and mandatory build entrypoints remain generated delegators.
Framework libraries own contract extraction/validation, HTTP/MCP security and
listener lifecycle, bounded worker supervision, encryption and audited access.
Application files own workspace membership, domain SQL, revisions, attachment
ownership and the insight algorithm.

Finite HTTP, SSE and WebSocket routes bind typed contract descriptors. The
framework applies origin and transport policy and refreshes stream authorization.
The worker uses native Apalis leases/retries; processing and publication are
bounded and at least once. Encrypted callbacks also authenticate the exact wire
bytes with HMAC and atomically claim nonces before decryption. Revision and
operation ledgers provide application idempotency. Durable acknowledged audits
precede protected reads and worker-channel plaintext access.

## Contracts and generated clients

`src/notes/domain` is a host-compilable Cargo contract library using the
service's `sdk.rs`. Rust structs/enums supply schemas and typed runtime bindings.
Building the service emits TypeScript operations, OpenAPI and contract JSON to
`src/client-sdk/typescript/notes`, plus Rust client source under
`src/client-sdk/rust`. The Rust SDK package builds its own modules from the same
contracts, avoiding a parallel workspace build race. The web imports the
service's generated TypeScript SDK. Notes and attachment timestamps are native Rust `DateTime<Utc>` values and generated TypeScript `Date` values; PostgreSQL stores their declared millisecond precision. Migration 007 explicitly normalizes older review timestamps and invalidates old cache generations. Presence uses the generated `notes/presence.browser.ts`
client with the same schema validation as the Node client; browser cookie authentication
and cancellation are handled by the native WebSocket transport.

Building Insights emits typed encrypted job/result channel assets under
`src/client-sdk/events` from the actual shared Rust DTOs. Different purposes and
wire envelopes are explicit. Consumers verify schema, tenant/context and an
acknowledged audit before returning plaintext.

`forklaunch sdk generate --target all` is also available for explicit artifact
regeneration. Compile-time generation remains part of the service/worker build.
`FORKLAUNCH_MODE=contract` exports finite service contracts without connecting to
application resources. Auth and Insights declare no HTTP contract export surface.
The native Apalis worker does not expose leased-backend HTTP management routes.

## Build, lint and test

Each Rust project has a local Makefile. Their generated targets are authoritative;
no shared `scripts/cargo-project.mk` is required. These commands work from the
application root:

```sh
cargo build --workspace
cargo test --workspace
make -C src/notes lint
make -C src/notes test
make -C src/insights check
make -C src/insights CARGO='cargo +stable' build
```

The root is a virtual Cargo workspace with one lockfile. After the first
resolution, retain `Cargo.lock` and use `--locked` for reproducible validation.
The Docker builder honors an existing lockfile and resolves one if absent.
Auth and web commands live in their respective `package.json` files.

Run `forklaunch analyze --verify --strict --pretty` from the application root
to check compiler results, generated asset freshness and supported static router
assembly. The public `/health` probe has a narrow, expiring declaration in
`.forklaunch/analyze.json`. The framework mounts public OpenAPI documentation;
business APIs use generated bindings. Static analysis complements the live security checks
below and does not prove runtime authorization or encryption by itself.

Native runtime commands need service-specific connection URLs and credentials;
the root `.env` supplies Compose substitutions and is not a complete host-runtime
configuration. `scripts/with-env.py --env-file FILE -- COMMAND` loads literal
dotenv values without executing shell code; existing environment values win.
Docker is the complete review workflow.

## Migrations and security checks

Notes, Insights and Auth own separate migrations. Compose runs Notes and worker
migration jobs with an owner connection before starting restricted runtime roles.
The native pool rejects privileged logins and active roles, including inherited
privileges. Audit connections use separately bounded pools. Service migrations
still declare forced row-level security and explicit grants.

For an already configured native environment, Notes accepts `--migrate`,
`--migrate-only`, `--migrate-status` and explicit reviewed `--migrate-baseline`.
Insights accepts `--migrate` or `--migrate-only`. See each service's migration
README before adopting an older database. Startup does not silently run Rust
schema migrations with runtime credentials.

Run `python3 scripts/check-all.py` against the running local stack to replay
authentication, HTTP/MCP, worker results, live authorization, RLS and encrypted
storage acceptance. It uses this Compose project's credentials without printing
them and leaves synthetic notes and attachments for storage inspection.
The `scripts/check-*` fixtures exercise authentication, APIs, MCP, live stream
revocation, encrypted storage and tenant isolation. Some require a running stack,
retained fixture records, or explicit owned-process parameters; read their headers
before running. NoteStorage is an actual ciphertext-only database row. Its classified fields
require protected storage types at compile time; generated bindings declare
fixed encryption purposes and validate the hydrated row before audited access.
Composite attachment/cache and outbox declarations retain explicit adapter
bindings for their dynamic contexts. Compliance artifacts distinguish protected
models from declarations; they are not a certification or automatic erasure of
every external copy.

## Telemetry and MCP

The API and worker use shared telemetry startup/shutdown. To export to your own
collector, set `OTEL_EXPORTER_OTLP_ENDPOINT` and run:

```sh
docker compose -f compose.yaml -f compose.telemetry.yaml up -d
```

Optional OTLP headers belong in private configuration. No collector or external
export is enabled by default. The API supports `MCP_ENABLED`, `MCP_PATH`,
`MCP_LISTENER`, `MCP_HOST` and `MCP_PORT`; the supplied gateway exposes shared
`/mcp`. Custom paths/listeners require matching gateway configuration. Internal
worker callbacks are excluded from MCP discovery.

The default `vendor/framework` snapshot is a development dependency source,
separate from business code. `generation.json` records generated files and
`.forklaunch/composition.json` records the recipe. Versioned dependency mode is
available when those versions are published. No public crate publication or
cloud deployment occurs as part of this review.
