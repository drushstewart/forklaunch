---
'@forklaunch/infrastructure-redis': patch
'@forklaunch/infrastructure-s3': patch
---

Read `v4` field envelopes. Native (Rust) cache and object-store writers seal records as randomized, stamped `v4` envelopes; the TypeScript readers only recognized `v1`–`v3` and returned the ciphertext as the value.
