# {{{name}}}

This Redis worker polls durable jobs, renews leases, retries failures, and drains its current delivery on Ctrl-C or SIGTERM. Implement idempotent application work in `src/handler.rs` before consuming jobs. The initial handler returns `worker_handler_not_implemented`; it never silently acknowledges unimplemented work.

```sh
REDIS_URL=redis://localhost:6379/ WORKER_NAMESPACE=my-app TENANT_ID=acme cargo run -p {{{name}}}
```

Configure Redis persistence and replication for your durability requirements. Namespace and tenant keys provide logical isolation; configure Redis ACLs separately. Bind `TENANT_ID` and `WORKER_NAMESPACE` from trusted application configuration. The adapter uses a standalone Redis connection manager; Redis Cluster redirection support is not implemented. The example env file is not loaded automatically. Commit bundled `.forklaunch/framework-rs` and `.forklaunch/blueprint-rs` source with the app.

With runtime configuration set, `--enqueue ID JSON` submits a job and `--inspect ID` reports its state without the payload or lease token. `WORKER_QUEUE` defaults to the crate name. Timing and retry settings are listed in `.env.example`; invalid values fail startup. Contract/OpenAPI export requires no Redis connection.

The runner processes one job at a time; run multiple processes for concurrency. On shutdown it stops claiming jobs, renews the active lease, and waits up to `WORKER_DRAIN_MS`. A timeout cancels the handler and leaves the job recoverable after lease expiry. Keep all handler work inside its returned future. Delivery is at least once; external side effects must be idempotent. Application error codes are retained as failure reasons; detailed error reporting belongs in the handler.
