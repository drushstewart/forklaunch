# Canonical IAM service

This fixture bundles the repository's actual TypeScript Express, core, common and
Zod validator sources; external runtime dependencies are pinned in `package.json`.
There are no replacement handlers, auth middleware or validator implementations.
The native counterpart is `sample-service/examples/canonical_iam.rs`, which uses
`forklaunch-blueprint-sample-service::iam_router` and the published IAM contracts.
Both seed Ada and Grace and use the same test-only HS256 key.

```sh
mkdir -p /tmp/fl-canonical-http-dependencies
cp cli/tests/fixtures/canonical-iam/package.json /tmp/fl-canonical-http-dependencies/
npm install --prefix /tmp/fl-canonical-http-dependencies --ignore-scripts
FORKLAUNCH_TEST_CANONICAL_DEPENDENCIES=/tmp/fl-canonical-http-dependencies \
  CARGO_INCREMENTAL=0 node cli/tests/rust_canonical_http_parity.mjs
```

The runner creates an isolated source bundle, starts both services on ephemeral
loopback ports, checks successful reads/lists, missing users and valid/missing/
malformed/expired/forged JWTs, and retrieves both generated OpenAPI documents.
Telemetry export is disabled for this local fixture. It terminates both processes
and removes its owned temporary source bundle even on failure.

The sample list handler intentionally ignores pagination (matching the existing
Rust sample); this fixture does not claim pagination conformance. Rejected auth
checks assert each implementation's exact status: missing credentials return 401
in both; malformed, expired and forged JWTs return TypeScript 403 versus Rust 401.
These three known differences are explicit regression expectations, not equivalence
claims. TypeScript uses `authenticated` for JWT-only contracts; the Rust IAM
contracts currently use `protected` without RBAC. Framework error envelopes and
access-level validation are separate auth/error parity work.
Whole-document OpenAPI normalization, multipart, streaming, other auth modes,
MCP conformance and production IAM persistence remain separate checklist items.
