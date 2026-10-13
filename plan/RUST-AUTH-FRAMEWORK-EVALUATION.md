# Rust authentication candidate evaluation

Reviewed 2026-10-12 America/Los_Angeles (2026-10-13 04:55 UTC). This closes the
plan's ten **evaluation criteria**, not cookie, OAuth, passkey or provider
implementation tasks. Registry metadata and hashes of inspected release sources
are recorded in [the evidence receipt](rust-auth-candidate-evidence.json).
No candidate was added to production dependencies during this review.

Keep the existing ForkLaunch IAM contracts and domain services. Select narrow
protocol/session components behind adapters: `tower-sessions` for browser-session
transport, `openidconnect`/`oauth2` for relying-party flows, and `webauthn-rs` for
a later passkey adapter. This is a selection decision; each integration still
needs its own locked dependency audit, persistence wiring and adversarial tests.
Do not embed an entire competing application framework or replace ForkLaunch's
authorization rules with a candidate's default policy.

## 1. Maintenance and release cadence

These are observed non-yanked releases, not inferred maintenance guarantees.
Versioned package source was downloaded from the official crates.io API and
inspected without executing package code. For WebAuthn, the stable 0.5.5 source
was selected rather than the 0.6.1-dev prerelease.

| Candidate | Observed release / UTC date | Cadence and integration decision |
| --- | --- | --- |
| [loco-rs](https://crates.io/api/v1/crates/loco-rs) | 1.2.0 / 2026-09-24 | 1.0–1.2 releases July–September; reference implementation, not an embedded dependency. |
| [rs-auth](https://crates.io/api/v1/crates/rs-auth) | 0.1.2 / 2026-04-15 | Three April releases; small early API, defer adoption. |
| [auth-framework](https://crates.io/api/v1/crates/auth-framework) | 0.6.0-rc48 / 2026-10-13 | Rapid RC churn and multiple release lines; defer a wholesale integration. |
| [tower-sessions](https://crates.io/api/v1/crates/tower-sessions) | 0.15.0 / 2026-02-01 | 0.14 dated January 2025; selected narrowly with maintenance caveat below. |
| [axum-login](https://crates.io/api/v1/crates/axum-login) | 0.18.0 / 2025-07-20 | No newer registry release in this receipt; optional reference, not needed for existing IAM. |
| [axum_session_auth](https://crates.io/api/v1/crates/axum_session_auth) | 0.21.0 / 2026-08-10 | Several 2026 releases; alternative session stack, avoid two competing stacks. |
| [openidconnect](https://crates.io/api/v1/crates/openidconnect) | 4.0.1 / 2025-07-06 | Narrow protocol library; selected for OIDC relying-party support. |
| [oauth2](https://crates.io/api/v1/crates/oauth2) | 5.0.0 / 2025-01-21 | Selected lower-level OAuth client and OIDC dependency. |
| [oauth-axum](https://crates.io/api/v1/crates/oauth-axum) | 0.1.4 / 2024-12-18 | Wrapper with limited release activity; use underlying protocol crate directly. |
| [webgates](https://crates.io/api/v1/crates/webgates) | 1.2.0 / 2026-08-15 | 1.0–1.2 released May–August; optional reference, no default adoption. |
| [authkestra](https://crates.io/api/v1/crates/authkestra) | 0.13.1 / 2026-09-26 | Several September minor releases; promising modular design, defer integration. |
| [webauthn-rs](https://crates.io/api/v1/crates/webauthn-rs) | 0.5.5 stable / 2026-04-30 | Stable and development lines coexist; selected stable line for later review. |

The tower-sessions/axum-login maintainer requested help taking over maintenance
on July 12, explicitly saying the projects were not abandoned. Treat this as a
continuity risk, not proof of abandonment. Keep our session-store adapter small
and replaceable. [Maintainer announcement](https://github.com/maxcountryman/axum-login/discussions/330)

## 2. Security posture and dependency risk

The existing [ForkLaunch security gate](RUST-SECURITY-REVIEW.md) audits our actual
three lockfiles. It does not certify unused candidates or every optional feature.
Before adding a selected component, resolve only required features in our
workspace, inspect the resulting dependency tree, run the same advisory gate,
and review cryptographic/provider configuration. An upstream claim of an audit
or production readiness is not independently established by this assessment.

Concrete source findings supporting the selection:

- rs-auth 0.1.2 defaults to implicit account linking and does not require email
  verification for password login. Its Google user-info model does not retain
  `email_verified`, while the service marks OAuth users verified. Its callback
  loads state and deletes it in separate store operations. These are blockers
  to reuse without tests for verified-provider identity, browser binding and
  atomic consumption; this review does not claim an exercised exploit.
  [Config](https://docs.rs/crate/rs-auth-core/0.1.2/source/src/config.rs),
  [Google mapping](https://docs.rs/crate/rs-auth-core/0.1.2/source/src/oauth/google.rs),
  [Service](https://docs.rs/crate/rs-auth-core/0.1.2/source/src/service.rs),
  [Callback](https://docs.rs/crate/rs-auth-axum/0.1.2/source/src/handlers/oauth.rs)
- auth-framework's reviewed RC defaults enable enhanced RBAC and PostgreSQL;
  enhanced RBAC enables its API server. That expands the integration boundary
  beyond the missing ForkLaunch components. Broad feature and compliance claims
  remain upstream claims; we have not validated its full default/optional graph.
  [Published manifest](https://docs.rs/crate/auth-framework/0.6.0-rc48/source/Cargo.toml)
- tower-sessions documents lost-update risks for concurrent session mutation.
  Do not put replay counters, one-time OAuth consumption or authoritative
  revocation state in a load/modify/save JSON session map. Use atomic store
  operations for those invariants. [Session documentation](https://docs.rs/tower-sessions/0.15.0/tower_sessions/)
- oauth-axum describes adding tests as future work in its published README.
  A provider preset wrapper supplies no reason to replace the protocol library
  and explicit ForkLaunch flow tests. [Published README](https://docs.rs/crate/oauth-axum/0.1.4/source/README.md)

## 3. Password hashing defaults and upgrades

Loco 1.2.0 explicitly selects Argon2id/version 19 with library-default parameters;
rs-auth 0.1.2 uses Argon2 defaults. Both create random salts and parse PHC strings
for verification. Their inspected hash helpers do not perform persistent
rehashing. auth-framework's inspected helper uses Argon2id with 46 MiB, one
iteration/one lane and a 128-byte password bound. These observations describe
code, not a recommendation to copy one fixed work factor.
[Loco helper](https://docs.rs/crate/loco-rs/1.2.0/source/src/hash.rs),
[rs-auth helper](https://docs.rs/crate/rs-auth-core/0.1.2/source/src/crypto/hash.rs),
[auth-framework helper](https://docs.rs/crate/auth-framework/0.6.0-rc48/source/src/utils/password.rs)

Keep ForkLaunch's salted Argon2id/PHC domain boundary. Hashing costs must be
benchmarked under concurrent login load. A future upgrade policy should verify
an old PHC hash, compare its algorithm/work factors with the server policy, then
replace it atomically only after successful authentication. Neither importing a
session crate nor selecting a password helper supplies that persistence policy.
Session/protocol libraries do not choose our password or account-recovery rules.

## 4. Session storage and CSRF

`tower-sessions` supplies a pluggable `SessionStore`, expiry and ID lifecycle.
Its published default cookie configuration is Secure, HttpOnly and SameSite
Strict. Keep a server-side store and a cookie containing only an opaque ID;
rotate on login/privilege changes and invalidate on logout/reset. Configure the
cross-site OIDC callback cookie separately rather than weakening all session
cookies. [Source](https://docs.rs/crate/tower-sessions/0.15.0/source/src/service.rs)

SameSite is not our complete CSRF policy. Unsafe browser-authenticated methods
need validated Origin/Referer policy and a session-bound CSRF token, with no
state change through GET. Prove rotation, expiry, revocation, cookie deletion,
concurrent logout and multi-replica behavior through real HTTP and MCP adapters.
These are integration acceptance conditions, still open in the implementation
plan. Browser-only sessions must not silently become bearer credentials.

`axum-login` permits custom user/authentication/authorization backends and binds
sessions to `session_auth_hash`; changing a password hash can invalidate a
session. It still requires application-owned credential verification and policy.
`axum_session_auth` uses a different session stack with cached user data and
cookie/header modes; that would add a second revocation/cache model.
[axum-login](https://docs.rs/axum-login/0.18.0/axum_login/),
[axum_session_auth](https://docs.rs/axum_session_auth/0.21.0/axum_session_auth/)

## 5. OAuth/OIDC provider coverage

Choose openidconnect for provider discovery, verified ID tokens and relying-party
flows; use oauth2 for OAuth-only providers and token exchange. The official
examples include PKCE, state and nonce verification, and warn against following
HTTP redirects because of SSRF. OAuth2 also documents timing-resistant secret
comparison. The ForkLaunch adapter must bind single-use state to the initiating
browser/session and tenant, enforce expiry, constrain issuer/redirect URLs and
verify audience/issuer/nonce before creating an identity.
[OIDC API](https://docs.rs/openidconnect/4.0.1/openidconnect/),
[OAuth2 API](https://docs.rs/oauth2/5.0.0/oauth2/)

Provider configurations are not interchangeable: test Google OIDC and GitHub
OAuth separately, including refresh, linking/unlinking and provider errors.
The rs-auth release supplies Google/GitHub presets; oauth-axum supplies several
presets but leaves transient state storage to callers. Neither makes provider
HTTP results equivalent to trusted local role or tenant assignments.
[rs-auth](https://github.com/rs-auth/rs-auth),
[oauth-axum](https://docs.rs/oauth-axum/0.1.4/oauth_axum/)

This selects client/relying-party functionality. Running a general OAuth/OIDC
identity-provider server is a separate domain; neither the name `openidconnect`
nor an OAuth client integration completes that server role.

## 6. Multi-tenant organizations

None of the selected narrow libraries supplies the complete ForkLaunch directory,
organization membership, tenant isolation and encrypted persistence contract.
Keep those domain/store traits. Key external identities by trusted issuer/provider
and subject within a tenant; do not infer membership from a matching email.
Organization membership must be checked again at the HTTP/MCP authorization
boundary, including after session refresh and membership revocation.

The higher-level candidates expose accounts, groups, permissions or storage
traits; that is not evidence of our tenant-filter semantics. In particular,
a group label in webgates must not be treated as a database tenant isolation
mechanism. Its documented policy alternatives use OR semantics.
[Webgates policy API](https://docs.rs/webgates/1.0.0/webgates/authz/access_policy/struct.AccessPolicy.html)

## 7. Roles, permissions, scopes and feature entitlements

Use candidate backends to establish identity, then map verified claims into
`forklaunch_auth::Principal` and run our existing `authorize` function. ForkLaunch
combines separate role, permission, scope, feature and subscription conditions;
forbidden roles/permissions also deny access. A generic role hierarchy or OR
policy is not equivalent. Do not map arbitrary provider claims into entitlements
without an explicit server-side mapping. Preserve role/tenant revocation and
subscription changes across cached sessions.
[Local authorization implementation](../framework-rs/crates/forklaunch-auth/src/lib.rs)

axum-login's user/group permission trait is adaptable, but its route macros
would duplicate our contract authorization middleware. Webgates is a useful
policy-design reference, not a replacement policy engine for this port.

## 8. WebAuthn/passkeys

Select webauthn-rs's stable API for a future optional adapter. The project warns
that registration/authentication challenge state must be held server-side; its
serialization escape hatch can enable replay if used as client-controlled state.
Use an expiring, atomically consumed server record, fixed RP ID/origin policy,
credential ownership checks and counter updates. Test replay, wrong origin/RP,
wrong user, missing user verification and recovery/enrollment authorization.
[WebAuthn documentation](https://docs.rs/webauthn-rs/0.5.5/webauthn_rs/)

Authkestra advertises WebAuthn/TOTP and fresh-proof requirements, while
its published facade delegates implementation to feature-gated crates. That is
a reason to revisit its engine later, not evidence that ForkLaunch passkey
persistence/recovery/client contracts are implemented.
[Published facade README](https://docs.rs/crate/authkestra/0.13.1/source/README.md)

## 9. Independence from Axum

Keep OIDC/OAuth verification and IAM policy in framework-independent domain
services. openidconnect/oauth2 expose HTTP-client abstractions rather than Axum
handlers. tower-sessions belongs at the Tower transport boundary. axum-login and
axum_session_auth are explicitly Axum-oriented. Loco brings its application and
ORM conventions; embedding it would duplicate our framework lifecycle.
[Loco project](https://github.com/loco-rs/loco)

rs-auth separates core/store/Axum crates; webgates separates domain and adapters;
authkestra separates engine, Axum and Actix adapters and leaves account ownership
to applications. These are promising boundaries, but importing their facades
would still require translating stores, identity semantics and errors.
[rs-auth core](https://docs.rs/rs-auth-core/0.1.2/rs_auth_core/),
[Webgates manifest](https://docs.rs/crate/webgates/1.2.0/source/Cargo.toml),
[Authkestra facade](https://docs.rs/crate/authkestra/0.13.1/source/README.md)

## 10. OpenAPI, SDK and MCP boundaries

Adapters must register ForkLaunch route contracts and retain the shared verified
session/principal extension. Document cookies/headers as security schemes,
expose typed public DTOs, and ensure generated clients never embed server secrets.
Keep issuer keys, provider refresh tokens, password hashes and CSRF storage out
of SDK artifacts and logs. HTTP, MCP tool dispatch and WebSocket upgrades must
run the same authorization decision after authentication.

No reviewed candidate demonstrated a drop-in replacement for our contract IR,
OpenAPI/AsyncAPI generators, both SDK languages and MCP forwarding semantics.
That is an integration assessment against our local architecture, not a claim
that those projects cannot support adapters. Acceptance requires generated-module
init/change/sync/delete, actual HTTP/MCP startup, auth failure paths and client
roundtrips. The corresponding implementation checklist items remain open.

## Decision record

Proceed with narrow, optional session and OIDC adapters; preserve the existing
Argon2id/JWT/HMAC and IAM domain. Retain webauthn-rs as the passkey candidate.
Defer rs-auth, auth-framework, webgates and authkestra adoption, and avoid layering
axum-login/axum_session_auth over an already-owned IAM session model by default.
Revisit a candidate only for a concrete missing capability, with a pinned feature
graph and evidence that it preserves the ten criteria above.
