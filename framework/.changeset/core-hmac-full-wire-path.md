---
'@forklaunch/core': minor
---

HMAC request verification now accepts signatures over the full wire path: the request URL without its query string, router mount included, e.g. `POST /payment-link`. That is the path the Rust verifiers and the new `@forklaunch/universal-sdk` signers sign. Express routers used to verify only the router-relative `req.path` (`/`). That form is still accepted, so existing callers keep working, but it is deprecated because a relative path is not bound to its mount; the first time a request uses it, the server logs a warning. Set `HMAC_ACCEPT_MOUNT_RELATIVE_PATHS=false` to reject it once every caller signs the full path.
