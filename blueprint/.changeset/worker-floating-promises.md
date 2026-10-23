---
"@forklaunch/implementation-worker-bullmq": patch
"@forklaunch/implementation-worker-kafka": patch
---

Mark intentionally unawaited calls (`failureHandler` on failed jobs, the Kafka producer `connect` and peek consumer `run`) with `void`. No behaviour change; this keeps type-aware lint clean in generated apps.
