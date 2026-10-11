# {{{name}}}

Tenant-bound persistent billing with thirty plan, subscription, checkout session, payment link and billing portal operations, compliance and health. Internal routes use request HMAC; billing portal routes require an asymmetric JWT with the `admin` role. HTTP and Streamable HTTP MCP share the listener and authentication rules. MCP defaults to `/mcp`; configure `MCP_ENABLED`, `MCP_PATH`, `MCP_ALLOWED_HOSTS` and `MCP_ALLOWED_ORIGINS` as needed.

Apply `cargo run -p {{{name}}} -- --migrate` with a schema owner. Run the service with a separate role without SUPERUSER/BYPASSRLS: schema USAGE and SELECT/INSERT/UPDATE/DELETE on `fl_billing.records`, plus INSERT on `fl_billing.sensitive_read_audit`. Every read uses the configured tenant and audits the verified actor. Provider data and opaque session URIs use tenant-derived encryption. Retain previous keys with `LEGACY_ENCRYPTION_KEYS` during rotation.

Checkout/payment-link/portal records use a ten-minute PostgreSQL TTL cache. Set `BILLING_DATABASE_BACKUP=true` for durable backups. Provider session expiry remains separate from cache expiry. Plans and subscriptions are durable.

{{#stripe}}The provider performs authenticated Stripe API requests pinned to the repository SDK version. `stripeFields` contains provider request fields and response objects. Provider calls precede local persistence; external effects can need reconciliation after transport or database failures. Plan replacement deletes the old provider plan first. Cancelled subscriptions cannot be resumed. Mutation retries are disabled.{{/stripe}}

Offline contract and OpenAPI exports do not require live database or provider settings. Add custom routes under `src/routes`. Compliance export/erasure require a `system`-role JWT. The current TS billing entities classify all fields as non-PII, so these endpoints report no classified data with HTTP 404. Provider payloads and session URLs remain encrypted in native storage. Stripe webhook synchronization and platform events are still being ported.
