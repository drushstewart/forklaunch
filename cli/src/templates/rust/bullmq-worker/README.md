# {{name}} BullMQ worker

This worker uses the official BullMQ wire protocol and exchanges jobs with TypeScript BullMQ producers and consumers. Implement `handler::ApplicationHandler` before consuming. Handlers receive the event ID, full JSON event, queue and current attempt count. Make side effects idempotent: delivery can repeat after a crash or expired lock.

Run without arguments to consume, `--enqueue ID JSON` to publish an object event, `--peek LIMIT` to inspect waiting and active events, and `--dead-letters LIMIT` to inspect failed jobs. Duplicate event IDs receive separate protocol job IDs. `--migrate` checks connectivity; BullMQ creates its structures lazily and requires no native lease schema. Default retries are three attempts with exponential backoff; completed and failed jobs remain retained by default.

Set `REDIS_KEY_PREFIX` (or `FORKLAUNCH_REDIS_KEY_PREFIX`) to the application's Redis ACL namespace. Native leased Redis workers use a separate storage protocol; changing backend does not migrate their backlog.

Optional `WORKER_HTTP_ADDR` exposes `/health`, `/ready` and bearer-protected `/control/pause`, `/control/resume`, `/control/drain`. Pause stops new fetches while active work finishes. Drain is irreversible and shutdown waits for active work within `WORKER_DRAIN_MS`. Optional scoped Redis Pub/Sub distributes controls using `--signal pause/resume/drain`; loss of the subscription drains the worker. Control publications are not durable desired state.

Workers export empty application HTTP/MCP contracts; management endpoints are separate. Environment configuration is listed in `.env.example`.
