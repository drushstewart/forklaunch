# {{{name}}}

Native Better Auth with encrypted tenant-bound PostgreSQL, cookie sessions,
credentials, Google OAuth, organizations, teams, roles and permissions. The 70
configured raw auth routes default to `/api/auth`. Set `BETTER_AUTH_BASE_PATH`
to a fixed namespace (use `/` for root); a path already present in `BETTER_AUTH_URL` takes
precedence. Five application contracts
provide public discovery, HMAC role/permission surfacing and system-role JWT
compliance export/erasure through HTTP and MCP.

Apply migrations with an owner connection:

```sh
DATABASE_URL="$DATABASE_OWNER_URL" cargo run -p {{{name}}} -- --migrate
```

Grant the runtime role USAGE on schema `fl_better_auth`, SELECT/INSERT/UPDATE/DELETE
on its `records`, `unique_claims` and `audit` tables, and USAGE on
`fl_better_auth.records_sequence_seq`. The runtime role must have neither
SUPERUSER nor BYPASSRLS. Export the settings in `.env.example` before running
`cargo run -p {{{name}}}`. Set `BETTER_AUTH_URL` to the public origin; configure
Google credentials as a pair when enabling Google. JWKS verification defaults
to this service's effective auth namespace plus `/jwks`, with JWT issuer and
audience equal to the canonical configured origin.

`GET /discovery/auth-methods`, `/openapi.json` and `/docs` are public. Scalar docs
include the application API and the complete BetterAuth API as separate sources.
Raw auth
routes keep cookie/CSRF semantics and remain outside the MCP tool registry.
Role/permission surfacing verifies request HMAC; compliance JWTs resolve their
system role from current stored organization membership rather than token role
claims. Classified exports preserve opaque fields; erasure deletes classified
Account/Jwks rows and retains unclassified User/Session records.

HTTP and MCP share the listener by default. Set `MCP_ENABLED=false` to disable
MCP, or use `MCP_LISTENER=separate` and `MCP_PORT` for another listener. Set
`MCP_PATH`, `MCP_ALLOWED_HOSTS` and `MCP_ALLOWED_ORIGINS` for deployment.

The internal `<auth-base-path>/test-callback` exchanges a signed session cookie for
JWT/session/email fragment values and clears only the configured session cookie.
`<auth-base-path>/oauth-redirect` emits the script-safe OAuth/SSO popup page. Both validate
callback destinations against the configured trusted origins.

Root auth reserves its own endpoint namespaces and keeps application routes,
docs, SDKs and MCP available.

This scaffold serves one trusted configured tenant. Parameterized and
percent-encoded auth namespaces, complete ORM/hooks and broader custom
Better Auth options remain parity work.
