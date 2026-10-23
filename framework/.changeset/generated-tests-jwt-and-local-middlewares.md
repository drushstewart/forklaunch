---
'@forklaunch/testing': minor
'@forklaunch/core': patch
---

`@forklaunch/testing`:

- `startTestJwtIssuer()` starts a local RS256 issuer that serves its JWKS over HTTP (default path `/api/auth/jwks`, as IAM serves it) and mints tokens with `sub`, `tenant_id`, `roles` and `permissions` claims, mirroring the Rust IAM's session tokens. `BlueprintTestHarness` takes `jwt: true` (or issuer options) to start one, point `JWKS_PUBLIC_KEY_URL` at it and return it as `setup.jwt`. `surfaceFromTestClaims` gives routers `surfaceRoles` / `surfacePermissions` that read those claims. Previously `JWKS_PUBLIC_KEY_URL` always pointed at `localhost:3000`, which nothing serves in a generated app, so JWT-protected routes could not be exercised in tests.
- The harness starts the containers a test needs (database, Redis, Kafka, S3) concurrently instead of one after another.
- Postgres and Redis containers wait for their "ready" log line rather than the default port check. The port check runs `docker exec`, which never settles under the Bun runtime, so every Redis-backed test hook in a Bun app hung until it timed out. Postgres also no longer reports ready during its init-time restart.
- The Kafka container advertises the host port clients actually reach (a free port bound up front) and waits for the broker to start, so producers in tests can connect; the KRaft cluster id default is now a valid one.

`@forklaunch/core`:

- Local SDK calls with `executeMiddlewares: true` run the middleware chain the way a server does: a middleware that answers the request itself (an auth 401/403) ends it before the controller runs, async hyper-express middlewares that take no `next` continue when they resolve, and the stand-in request/response carry what the express and hyper-express adapters read (`getHeaders`, `locals`, hyper-express's status/header fields and body readers). Before, every middleware ran even after one had responded, the controller then overwrote the response, and the express and hyper-express response enrichment crashed on the stand-in response.
- Response validation for a default error status (400/401/403/404/500) no longer crashes with TypeBox when the route declares that status: a compiled TypeBox schema cannot be a union member, so the declared schema and the string message are tried in turn. Zod output is unchanged.
