---
'@forklaunch/implementation-worker-kafka': patch
---

`KafkaWorkerProducer` waits for its connection before sending. It connected in the constructor without waiting, so a job enqueued right after startup (the first request a service handles, or a test) failed with "The producer is disconnected". A failed connection attempt is retried by the next send.
