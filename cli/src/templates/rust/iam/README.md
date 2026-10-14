# {{{name}}}

PostgreSQL IAM with tenant isolation, encrypted directory CRUD, Argon2id credentials, and persistent opaque or explicitly configured RS256 bearer sessions.

Apply migrations using a schema-owner connection:

```sh
DATABASE_URL="$DATABASE_OWNER_URL" cargo run -p {{{name}}} -- --migrate
```

Grant the runtime role USAGE on the schema and SELECT/INSERT/UPDATE/DELETE on `fl_iam_users`, `fl_iam_organizations`, `fl_iam_roles`, `fl_iam_sessions`, `fl_iam_directory`, `fl_iam_directory_unique_claims`, and `fl_iam_directory_audit`, and (when browser sessions are enabled) `fl_iam_browser_sessions`. Grant USAGE on `fl_iam_directory_sequence_seq`. It must have neither SUPERUSER nor BYPASSRLS. Configure `DATABASE_URL`, a trusted `TENANT_ID`, `ENCRYPTION_KEY`, `HMAC_SECRET_KEY`, and `JWKS_PUBLIC_KEY_URL`, then run `cargo run -p {{{name}}}`. The service rejects privileged database roles. Environment files are examples; export variables before running.

Create users and roles through `PostgresIamService`'s administrative methods in your application's trusted provisioning path. No default account or password is created.

`POST /auth/login` accepts `{ "email": "...", "password": "..." }` and returns a session whose `id` is the bearer credential. Send `Authorization: Bearer <id>` for the legacy `/users`, `/organizations`, permission lookup, and logout routes. The legacy `/users/:id` SDK/MCP operation is `getSessionUser`. `POST /auth/logout` revokes the current session. `/health`, `/openapi.json`, and `/docs` are public. Login responses disable caching.

Opaque sessions are the default. To issue RS256 JWTs, configure all four of
`SESSION_JWT_PRIVATE_KEY_PEM`, `SESSION_JWT_KEY_ID`, `SESSION_JWT_ISSUER` and
`SESSION_JWT_AUDIENCE`. Partial or invalid signing configuration rejects startup.
Publish the matching public key at the trusted JWKS endpoint configured by token
consumers; this module does not automatically publish keys. New tokens contain a
unique `jti`, subject, tenant ID, issuance/expiry times, issuer/audience and a
snapshot of server-resolved roles/permissions. Tokens expire after one hour and
must fit the existing 4096-byte bearer limit. Email/password fields are omitted.

Local session routes always check the full token's stored hash and current user
state, preserving logout, password-change revocation, disabled users and tenant
isolation. A valid signature alone cannot authenticate an unstored token there.
Changing signing configuration affects future issuance; existing stored sessions
retain their normal expiry/revocation lifecycle. Downstream stateless JWT consumers
must separately enforce current grants/revocation and tenant policy when required.
Directory identities remain separate from credential/session identities.

The `/user`, `/role`, `/permission`, and `/organization` families expose the configured TypeScript module's thirty CRUD and grant-surfacing operations through HTTP and MCP. Internal routes verify request HMAC signatures; protected routes verify asymmetric JWKS JWTs and their declared roles or permissions. User reads/updates/deletes resolve the JWT subject's organization from storage and enforce it inside each directory transaction. Sensitive fields are encrypted with tenant- and record-bound envelopes; uniqueness claims support retained encryption keys, and successful reads/writes commit actor audits atomically. Directory users are separate from credential/session users; CRUD passwords are ignored as in the configured TypeScript mapper. Provision authentication identities through the repository API.

`GET /discovery/auth-methods` is public and returns 204 for base IAM. The system-role JWT routes `/compliance/export/:userId` and `/compliance/erase/:userId` return 404 because the configured TypeScript IAM entities explicitly classify every scalar as `none`. Field encryption does not change that classification. Application-specific compliance classifications and entity extensions require a corresponding native compliance implementation.

The tenant comes from server configuration, never client headers. This scaffold serves one configured tenant. Review the configured role/access policies for your application. OAuth/OIDC providers, verification/reset flows, and Better Auth client compatibility remain separate work.

HTTP health and Streamable HTTP MCP share the configured listener. MCP is served at `/mcp` by default, and tools retain HTTP route authentication. Set `MCP_ENABLED=false` to disable it, or configure `MCP_PATH`, `MCP_ALLOWED_HOSTS` and `MCP_ALLOWED_ORIGINS` for deployment.


Enable browser sessions with `SESSION_COOKIE_ORIGIN=https://app.example.com`, an
exact HTTPS origin with no path. Migrations add `fl_iam_browser_sessions`; grant
the restricted runtime role CRUD on that table. The service uses the existing
encryption key ring and tenant, with its service name as the store namespace.
Changing that name requires a fresh browser login. Omit the variable to leave
browser routes disabled; empty, non-HTTPS or malformed values reject startup.

GET `/auth/browser/csrf` establishes a Secure/HttpOnly/SameSite=Strict `__Host-iam`
cookie and returns `csrfToken`. POST `/auth/browser/login` accepts email/password
JSON plus that cookie, the `x-csrf-token` header and the exact trusted Origin.
It rotates cookie and CSRF identifiers and returns only the replacement CSRF
token. GET `/auth/browser/session` returns current subject/grants/expiry.
POST `/auth/browser/logout` requires the current CSRF proof, revokes the IAM
credential and clears the cookie. All responses are no-store. Browser endpoints
are excluded from SDK/MCP contracts; existing bearer routes still require their
Authorization header. Applications own TLS termination and trusted-origin CORS.

Cookie IDs are hashed in PostgreSQL; server-side records, including the backing
IAM credential, are encrypted and bound to tenant, namespace, record ID and
expiry. Sessions survive process restarts and support retained encryption keys.
Updates cannot resurrect deleted or expired records. Records expire after thirty
minutes without writes; the backing IAM credential keeps its one-hour maximum.
Invoke `PostgresCookieStore::purge_expired` and IAM expiry maintenance from a
trusted scheduled task. Cookie persistence and IAM writes are separate transactions;
failed persistence can leave an unreachable credential until normal expiry.
