---
'@forklaunch/core': minor
---

HMAC request verification now checks the signature over the full wire path (the request URL without its query string, router mount included, e.g. `POST /payment-link`), the same path the Rust verifiers and the new `@forklaunch/universal-sdk` signers sign. Express routers used to verify the router-relative `req.path` (`/`); signatures over that form now fail with 403 unless the server sets `HMAC_ACCEPT_MOUNT_RELATIVE_PATHS=true`, which also accepts the old form. That flag is deprecated, since a relative path is not bound to a mount. Callers that build headers with `generateHmacAuthHeaders` must pass the full path.
