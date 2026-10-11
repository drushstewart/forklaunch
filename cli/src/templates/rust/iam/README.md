# {{{name}}}

PostgreSQL IAM with tenant isolation, encrypted directory CRUD, Argon2id credentials, and persistent opaque bearer sessions.

Apply migrations using a schema-owner connection:

```sh
DATABASE_URL="$DATABASE_OWNER_URL" cargo run -p {{{name}}} -- --migrate
```

Grant the runtime role USAGE on the schema and SELECT/INSERT/UPDATE/DELETE on `fl_iam_users`, `fl_iam_organizations`, `fl_iam_roles`, `fl_iam_sessions`, `fl_iam_directory`, `fl_iam_directory_unique_claims`, and `fl_iam_directory_audit`. Grant USAGE on `fl_iam_directory_sequence_seq`. It must have neither SUPERUSER nor BYPASSRLS. Configure `DATABASE_URL`, a trusted `TENANT_ID`, `ENCRYPTION_KEY`, `HMAC_SECRET_KEY`, and `JWKS_PUBLIC_KEY_URL`, then run `cargo run -p {{{name}}}`. The service rejects privileged database roles. Environment files are examples; export variables before running.

Create users and roles through `PostgresIamService`'s administrative methods in your application's trusted provisioning path. No default account or password is created.

`POST /auth/login` accepts `{ "email": "...", "password": "..." }` and returns a session whose `id` is the bearer credential. Send `Authorization: Bearer <id>` for the legacy `/users`, `/organizations`, permission lookup, and logout routes. The legacy `/users/:id` SDK/MCP operation is `getSessionUser`. `POST /auth/logout` revokes the current session. `/health`, `/openapi.json`, and `/docs` are public. Login responses disable caching.

The `/user`, `/role`, `/permission`, and `/organization` families expose the configured TypeScript module's thirty CRUD and grant-surfacing operations through HTTP and MCP. Internal routes verify request HMAC signatures; protected routes verify asymmetric JWKS JWTs and their declared roles or permissions. User reads/updates/deletes resolve the JWT subject's organization from storage and enforce it inside each directory transaction. Sensitive fields are encrypted with tenant- and record-bound envelopes; uniqueness claims support retained encryption keys, and successful reads/writes commit actor audits atomically. Directory users are separate from credential/session users; CRUD passwords are ignored as in the configured TypeScript mapper. Provision authentication identities through the repository API.

`GET /discovery/auth-methods` is public and returns 204 for base IAM. The system-role JWT routes `/compliance/export/:userId` and `/compliance/erase/:userId` return 404 because the configured TypeScript IAM entities explicitly classify every scalar as `none`. Field encryption does not change that classification. Application-specific compliance classifications and entity extensions require a corresponding native compliance implementation.

The tenant comes from server configuration, never client headers. This scaffold serves one configured tenant. Review the configured role/access policies for your application. Cookie/CSRF sessions, OAuth/OIDC providers, verification/reset flows, and Better Auth client compatibility remain separate work.

HTTP health and Streamable HTTP MCP share the configured listener. MCP is served at `/mcp` by default, and tools retain HTTP route authentication. Set `MCP_ENABLED=false` to disable it, or configure `MCP_PATH`, `MCP_ALLOWED_HOSTS` and `MCP_ALLOWED_ORIGINS` for deployment.
