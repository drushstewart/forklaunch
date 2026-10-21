# Insights worker

Apalis 1.0/PostgreSQL consumes encrypted `SealedNoteJob` values. The API writes
note revision and encrypted outbox intent in one transaction. The dispatcher
locks up to 32 outbox rows with `SKIP LOCKED`, calls Apalis's public transactional
`queries::push_tasks`, and marks those rows dispatched in the **same transaction**.
The operation UUID is the queue idempotency key. A lost commit acknowledgement
can be inspected/retried without creating another task from a dispatched row.

The worker decrypts using an injected key bound to workspace + operation +
`note-insight-job-v2`, checks every inner/outer identity, and computes a Unicode
excerpt and whitespace-delimited word count. Neither plaintext nor keys implement
Debug here. Queue arguments remain ciphertext; persisted errors are fixed codes.
Results are sent with the generated Rust `NotesClient::apply_insight` typed body,
not a handwritten HTTP request. The internal API serializes result application,
checks original outbox identity/current note revision, and emits one durable event
per operation. Retries are at least once; duplicate and stale results are terminal
successful acknowledgements. This is deterministic local processing, not an AI model.

`--migrate-only` installs the pinned Apalis schema and grants the dedicated worker
role queue permissions. Run it once with the migration owner. Normal startup uses
`fieldnotes_worker`, which cannot read notes; its outbox access is intentionally
cross-tenant internal service authority. Never give that database credential or
the internal HTTP secret to browser clients. The worker does not run migrations
on normal startup. SIGTERM/SIGINT stop dispatch and request graceful worker drain;
remaining work can be reclaimed by Apalis after a worker dies.

Environment: `DATABASE_URL`, `NOTES_INTERNAL_URL`, `INTERNAL_AUTH_SECRET`,
`ENCRYPTION_KEY_HEX` (64 ASCII hexadecimal characters). The API URL is the trusted
internal service origin; the public proxy excludes `/internal/`.

Proof: library unit tests verify envelope bindings/tamper/Unicode bounds. The
ignored `postgres` integration test uses an **owned disposable database**, creates
tables, and exercises actual Apalis migrations/consumer, concurrent dispatch,
producer rollback, and enqueue/mark rollback. Set
`FIELDNOTES_WORKER_TEST_DATABASE_URL` only to that disposable database.
Generated-app acceptance additionally checks API revision/idempotence, live SDK
publication, restart recovery, SSE delivery and RLS with runtime roles.

TypeScript clients submit notes through the generated API. This is not wire
compatible with BullMQ or the legacy Redis list queue: Apalis owns its SQL queue
schema. AES-GCM envelope vectors are separately shared with the TypeScript auth
fixture; a common JSON shape alone does not establish queue compatibility.

## Package and migration ownership

This worker is its own Cargo package, coordinated by the root Cargo workspace.
From the generated repository root, build or run this package independently:

```sh
cargo build --locked --manifest-path src/insights/Cargo.toml
cargo run --locked --manifest-path src/insights/Cargo.toml --bin fieldnotes-insights
```

The run command reads the runtime environment listed above. To initialize the
queue, set `DATABASE_URL` to the migration owner's connection and run:

```sh
cargo run --locked --manifest-path src/insights/Cargo.toml --bin fieldnotes-insights -- --migrate-only
```

`migrations/runner.rs` invokes the pinned Apalis migration API, then applies
`migrations/001-worker-grants.sql`. Apalis owns its queue schema and migration
history; this service owns the runtime-role grants. Its DDL is not copied into
application SQL. The `fieldnotes_worker` role must already exist, as it does
with the local Compose role bootstrap. Normal worker startup uses that restricted
runtime role and never calls the migration runner. Tests can invoke
`fieldnotes_insights::migrations::run(&pool)` against an owned migration database.

Notes, note events and transactional outbox tables remain owned by the Notes
service's migrations. Queue initialization does not migrate those business tables.

## Project tasks

Run from this project directory:

```sh
make fmt-check
make lint
make check
make test
make build
```

`make fmt` formats only this Cargo package. `lint` runs Clippy for all package
targets with warnings treated as errors. Override the Cargo command when needed:
`make CARGO='cargo +stable' test`. These targets use the local `Cargo.toml`; the
application workspace still coordinates shared dependencies and its lockfile.

`make run` launches this service with its configured environment.
`make migrate` runs queue initialization with `--migrate-only`; supply the
migration-owner `DATABASE_URL`, not the restricted runtime credential.

## Source layout

The worker follows the TypeScript worker project boundaries:

- `lib.rs` exports the package API; `main.rs` delegates startup.
- `bootstrapper.rs` opens the database and selects migration or normal startup.
- `registrations.rs` constructs the worker state and infrastructure dependencies.
- `worker.rs` composes the queue consumer, dispatch/cleanup schedules and shutdown.
- `domain/services` owns insight analysis and typed SDK publication.
- `domain/types` owns the encrypted job and result types.
- `persistence` owns transactional dispatch and bounded attachment reclamation.
- `migrations` owns queue initialization and runtime grants.
- `__test__` contains unit and PostgreSQL integration tests.

The dependency graph never imports the Notes server: publication uses the generated
Rust client, while attachment cleanup uses restricted SQL and object-store access.

## Encrypted job channel v2

New repositories use queue `fieldnotes-note-insights-v2`, outbox kind
`note-insight-v2`, and purpose `note-insight-job-v2`. The encrypted plaintext
is `{payload: NoteJob, trace?: traceparent}`. Generated channel schemas bind
identity through `payload.*`; broker routing fields remain outside ciphertext
and must match the authenticated inner identity. The worker imports the trace
parent only after durable service-actor access audit acknowledgement. Audit and
processing share the bounded job deadline. No baggage or tracestate is carried.

Existing v1 deployments must drain their old outbox and queue with the v1 worker
before switching, or supply an explicitly authorized v1-to-v2 adapter. No SQL
migration rewrites, deletes, or silently relabels queued ciphertext. The signed,
audited result channel remains `note-insight-result-v1`; this release propagates
trace context from API to worker only, not back into the result API callback.
