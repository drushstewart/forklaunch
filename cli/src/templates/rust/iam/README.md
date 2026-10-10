# {{{name}}}

PostgreSQL IAM with tenant isolation, Argon2id credentials, and persistent opaque bearer sessions.

Apply migrations using a schema-owner connection:

```sh
DATABASE_URL="$DATABASE_OWNER_URL" cargo run -p {{{name}}} -- --migrate
```

Grant the runtime role USAGE on the schema and SELECT/INSERT/UPDATE/DELETE on `fl_iam_users`, `fl_iam_organizations`, `fl_iam_roles`, and `fl_iam_sessions`. It must have neither SUPERUSER nor BYPASSRLS. Configure `DATABASE_URL` and a trusted `TENANT_ID`, then run `cargo run -p {{{name}}}`. The service rejects privileged database roles. Environment files are examples; export variables before running.

Create users and roles through `PostgresIamService`'s administrative methods in your application's trusted provisioning path. No default account or password is created.

`POST /auth/login` accepts `{ "email": "...", "password": "..." }` and returns a session whose `id` is the bearer credential. Send `Authorization: Bearer <id>` for user, permission, organization, and logout routes. `POST /auth/logout` revokes the current session. `/health`, `/openapi.json`, and `/docs` are public. Login responses disable caching.

The tenant comes from server configuration, never client headers. This scaffold serves one configured tenant. Add application-specific role/access policies before exposing directory routes to your users. Cookie/CSRF sessions, OAuth/OIDC providers, verification/reset flows, and Better Auth client compatibility remain separate work.

HTTP health and Streamable HTTP MCP share the configured listener. MCP is served at `/mcp` by default, and tools retain HTTP route authentication. Set `MCP_ENABLED=false` to disable it, or configure `MCP_PATH`, `MCP_ALLOWED_HOSTS` and `MCP_ALLOWED_ORIGINS` for deployment.
