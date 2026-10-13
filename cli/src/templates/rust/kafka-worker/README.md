# {{name}} Kafka worker

Implement idempotent application work in `src/handler.rs`. The generated handler
fails until you implement it. Set the connection and trusted tenant environment
from `.env.example`, then explicitly create queue/dead-letter topics with
`cargo run -- --migrate`. Topic creation requires broker administrative permissions;
normal operation disables automatic topic creation. Provision production replication
and retention with your broker administrator.

Publish with `cargo run -- --enqueue ID JSON`, inspect pending group records with
`--peek LIMIT`, or inspect retained failures with `--dead-letters LIMIT`. Summaries
omit payloads and credentials. Kafka permits duplicate job IDs. Topics and consumer
groups are deterministically scoped by namespace, trusted tenant and queue. Different
group IDs consume independently. Use broker ACLs to enforce tenant authorization.

Each process consumes sequentially; scale across replicas and topic partitions.
Give each worker a stable, unique `KAFKA_TRANSACTIONAL_ID`, and each independent
publisher a separate `KAFKA_PUBLISHER_ID`. Reusing an identity fences an older
producer. Consumer offsets and retry/dead-letter publications commit atomically.
A failed or uncertain transaction stops the worker; restart explicitly. Handler
side effects remain at-least-once and must be idempotent. Attempt counts track
handler dispositions; a crash before commit can repeat the same attempt.

Shutdown drains the active handler within `WORKER_DRAIN_MS`. Cancellation leaves
its offset uncommitted so another replica/restart can recover it. Handler execution
is bounded by `WORKER_HANDLER_TIMEOUT_MS`. Assignment changes fail closed rather
than committing a record from an older consumer generation. Pending inspection is
bounded, does not advance offsets, and can return fewer records during open
transactions. Ordering is per partition. Kafka log retention governs history;
per-job database inspection and deletion are not Kafka operations.

Optional `WORKER_HTTP_ADDR` enables health/readiness. Set a random 32+ character
`WORKER_CONTROL_TOKEN` for bearer-authenticated `/control/pause`, `/control/resume`,
and `/control/drain`. These affect the local process unless optional Redis signaling
is configured with `WORKER_SIGNAL_REDIS_URL` and `WORKER_SIGNAL_NAMESPACE`. Then
HTTP commands and `--signal pause/resume/drain` publish to the same scoped group;
responses report subscriber counts, not acknowledgements. Delivery is ephemeral.
Subscription failures drain active work and exit with an error; restarts begin
running without replay or automatic desired-state restoration.

`KAFKA_CLIENT_PROPERTIES` accepts librdkafka TLS/SASL options as a JSON object of
strings. Keep it out of logs. Delivery-critical offset, transaction and isolation
settings cannot be overridden. OpenAPI and SDK exports contain no application
routes until application HTTP contracts are added separately.


## Separate control service

Run this executable with `--control-service` and `WORKER_HTTP_ADDR` to serve
health/readiness without connecting to job storage, claiming jobs or running
retention. Without `WORKER_CONTROL_TOKEN`, controls return 404. Authenticated
controls additionally require the distributed signal configuration above; they
publish to consumers without becoming a consumer or signal subscriber. Readiness
describes this control process, not remote queue health. Draining consumers does
not stop the control service; SIGTERM stops it cleanly.

New release manifests advertise distinct control and consumer roles when the
installed generated sources support this entry point. Older/custom sources keep
their existing command. The health/control server does not expose MCP tools.
