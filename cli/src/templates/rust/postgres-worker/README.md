# {{{name}}}

This PostgreSQL worker polls durable jobs, renews leases, retries failures, and drains its current delivery on Ctrl-C or SIGTERM. Implement idempotent application work in `src/handler.rs` before consuming jobs. The initial handler returns `worker_handler_not_implemented`; it never silently acknowledges unimplemented work.

```sh
DATABASE_URL=postgres://schema_owner:password@localhost/app cargo run -p {{{name}}} -- --migrate
DATABASE_URL=postgres://restricted_app:password@localhost/app TENANT_ID=acme cargo run -p {{{name}}}
```

Use your own credentials. Runtime roles require schema USAGE, table DML, and sequence USAGE in `fl_worker`; SUPERUSER/BYPASSRLS roles are rejected. Bind `TENANT_ID` from trusted application configuration. The example env file is not loaded automatically. Commit bundled `.forklaunch/framework-rs` and `.forklaunch/blueprint-rs` source with the app.

With runtime configuration set, `--enqueue ID JSON` submits a job and `--inspect ID` reports its state without the payload or lease token. `WORKER_QUEUE` defaults to the crate name. Timing and retry settings are listed in `.env.example`; invalid values fail startup. Contract/OpenAPI export requires no database connection.

The runner processes one job at a time; run multiple processes for concurrency. On shutdown it stops claiming jobs, renews the active lease, and waits up to `WORKER_DRAIN_MS`. A timeout cancels the handler and leaves the job recoverable after lease expiry. Keep all handler work inside its returned future. Delivery is at least once; external side effects must be idempotent. Application error codes are retained as failure reasons; detailed error reporting belongs in the handler.
