# Notes service migrations

Notes owns notes, encrypted attachment metadata, the transactional outbox, events,
and cache-version behavior. These SQL files run through SQLx's migration runner,
which records versions and SHA384 checksums in `_fieldnotes_notes_migrations`,
locks competing migration runners, and applies each migration transactionally.
The runtime API role never performs migrations. Infrastructure creates the
`fieldnotes_api` and `fieldnotes_worker` roles before these migrations run.

From `src/notes`, set the migration owner's `DATABASE_URL` (or use a private
`python3 ../../scripts/with-env.py <env-file> <command>` to load a private environment file):

```sh
cargo run -- --migrate-status
cargo run -- --migrate-only
python3 migrations/create.py add_note_tags
```

Inspect and edit the newly created SQL before applying it. Never modify an
already-applied migration: SQLx rejects checksum drift. The build embeds migration
files, so rebuild the service image to ship a new migration. API startup does not
implicitly upgrade schemas. `make migrate` invokes the same explicit migration command.

These migrations are forward-only. Use a reviewed compensating migration for a
reversible change, or restore a verified backup when a destructive rollback is
required. Role provisioning remains infrastructure-owned. The worker's Apalis
schema and Auth's schema use their own migration systems.

## Explicit adoption of the earlier review app

Earlier app versions applied the first three files via PostgreSQL initialization
without recording history. Do not run this adoption step on a fresh database or
an unknown existing schema. Normal `migrate-up` never silently adopts a schema.

1. Back up the database and verify its schema, policies, grants, constraints,
   indexes and trigger definitions against the unchanged initial migrations.
2. Verify the original migration contents against `INITIAL_CHECKSUMS` in
   `runner.rs`. These are immutable SHA384 hashes from the initial review app.
3. With the migration owner's connection, explicitly record the reviewed schema:

   ```sh
   NOTES_BASELINE_CONFIRM=reviewed-initial-schema cargo run -- --migrate-baseline
   cargo run -- --migrate-status
   cargo run -- --migrate-only
   ```

Baseline rejects changed initial migration files, existing migration history,
missing tables, disabled/unenforced RLS, missing initial policy names, missing
cache-version column/trigger, or worker access to Notes plaintext tables. Those
checks catch partial installations; they do **not** replace the full schema review
in step 1. It records only versions 1–3 in one transaction under the SQLx migration
lock. It does not execute application DDL, update application rows, or mark later
migrations applied. Repeating baseline fails instead of rewriting history.

Migration 005 adds append-only compliance access records. The API role has only
INSERT privilege, with forced tenant RLS. Reads of protected database members,
attachment payloads and cached snapshots commit an audit record before plaintext
is returned. Records contain verified subject IDs, entity/member classification,
resource/workspace IDs and fixed access reasons; they contain no note text,
attachment names, content bytes, cookies or encryption material. Subject IDs and
resource identifiers remain access-controlled audit metadata, not public logs.

Audit transactions use a separate pool capped at four connections, a five-second
operation timeout and synchronous commit. This prevents deadlocks when request
transactions occupy the main pool; failures deny protected reads. Audit survives
business transaction rollback. Storage retention/export remains an explicit
privileged operational policy; no public audit or erasure endpoint is implied.

Migration 006 adds operation/purpose metadata to the audit ledger and permits
only `service:insights` inserts from the worker role. The API role can insert its
existing user receipts and `service:notes` receipts. Worker input decryption and
API result decryption acknowledge separate committed audit transactions before
application code receives plaintext; unavailable audit storage fails the attempt.
The worker uses a dedicated bounded pool for these acknowledgments. This records
service access, not an invented user action. Payload, ciphertext, and keys are
never included in the receipt. Existing rows and user audits remain readable to
privileged operators; runtime roles cannot read, update, or delete the ledger.

New generated repositories use the version 2 encrypted insight job channel. This
is a protocol change, not a database rewrite: no migration transforms existing
`note-insight-v1` outbox rows or `fieldnotes-note-insights-v1` queue jobs. Drain
them using the old worker before upgrading, or implement an authorized adapter
that decrypts and re-encrypts with the new purpose after auditing access. See
`src/insights/README.md` for the v2 wire contract and directional trace scope.
