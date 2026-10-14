# Rust port engineering security review — 2026-10-12

Scope: framework/blueprint authentication, HMAC, JWT, encryption, generated
credentials/SDKs, HTTP/MCP boundaries and the CLI that emits these applications.
Methods: source inspection, actual TypeScript/JOSE HTTP comparisons, existing
crypto/runtime regressions and RustSec scans of all three workspace lockfiles.
Production deployment controls and the remaining IAM/provider/parity tasks stay
in the implementation checklist.

## Findings fixed in this review

| Finding | Change and evidence |
| --- | --- |
| Recently expired HS256 tokens accepted, optional not-before unchecked | `verify_jwt_hs256` now uses zero tolerance, rejects `exp <= now`, checks optional `nbf`/`iat` numeric types and rejects empty keys. The actual service comparison reproduced Rust 200 versus TS 403 before the fix; 34 paired HTTP outcomes and 16 auth tests now pass. Explicit JWK/JWKS `Validation` policies remain caller-owned. |
| Declared JWT routes returned 401 for rejected tokens versus TS 403 | Contract-aware middleware mapping now matches TS while preserving opaque-session 401 behavior. Nine middleware cases cover the distinction and correlation IDs. Native JSON error envelopes still differ from TS text errors. |
| Redundant AWS legacy TLS connector included affected h2/webpki versions | Removed S3's legacy `rustls` feature while retaining `default-https-client` (current Rustls/AWS-LC). This removed h2 0.3.27 and rustls-webpki 0.101.7 from the framework lockfile. Nineteen S3 tests plus two live MinIO suites pass. |
| Older anyhow unsoundness notice | Framework/blueprint lockfiles now use 1.0.103. |
| Archived CLI YAML parser and unsound dependency | Switched the internal `serde_yml` dependency alias to `serde_norway` 0.9.42. All 1,076 CLI tests pass. The alias preserves source import names, not the old implementation. |
| Invalid OpenAPI auth references and path metadata | Actual TS emitter now references declared security schemes, uses empty non-OAuth scope arrays and marks path parameters required. Four emitter scenarios and both live documents enforce these invariants. |

Primary advisory references: [h2](https://rustsec.org/advisories/RUSTSEC-2026-0258.html),
[webpki CRL parsing](https://rustsec.org/advisories/RUSTSEC-2026-0104.html),
[webpki URI constraints](https://rustsec.org/advisories/RUSTSEC-2026-0098.html),
[webpki wildcard constraints](https://rustsec.org/advisories/RUSTSEC-2026-0099.html),
[anyhow](https://rustsec.org/advisories/RUSTSEC-2026-0190.html),
[serde_yml](https://rustsec.org/advisories/RUSTSEC-2025-0068.html) and
[libyml](https://rustsec.org/advisories/RUSTSEC-2025-0067.html).

## Dependency findings retained with source-specific review

The scan uses cargo-audit 0.22.2 and the RustSec database. The initial reviewed
revision was `57dcc0f2192e025138907a495c1ac9a7e1e3e123`; CI fetches current data.
No scanner ignores, severity filters or platform filters are permitted. Reports
retain all findings. Exact scope/package/version/advisory exceptions live in
[`rust_security_advisories.json`](../cli/tests/fixtures/rust_security_advisories.json).
A new advisory or version fails the gate and requires remediation or another
source review. A passing gate means there are no **unreviewed** findings.

| Scope | Finding | Current reachability assessment |
| --- | --- | --- |
| Framework | [RUSTSEC-2026-0253](https://rustsec.org/advisories/RUSTSEC-2026-0253.html), lru 0.16.4 | AWS S3 1.124.0 uses `LruCache<CacheKey, ...>` for S3 Express identities. `CacheKey` wraps `String` and has no custom destructor; the advisory requires a panicking key destructor during `pop`. The reviewed SDK uses `get_or_insert_mut`. Re-review any SDK upgrade or direct LRU use. Upgrading to an SDK using fixed LRU remains dependency maintenance work. |
| Framework | [RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html), rsa 0.9.10 | The optional `openidconnect` 4.0.1 adapter calls `CoreJsonWebKey` public RS256 verification, which uses `RsaPublicKey::verify`. It exposes no RSA private-key operations; client credentials use HTTP Basic and fixture signing uses jsonwebtoken/ring. Re-review before adding private-key operations or changing this path. |
| Blueprint | [RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html), rsa 0.9.10 | [`rsa_key.rs`](../blueprint-rs/crates/implementations/iam/forklaunch-iam-better-auth/src/jwt/rsa_key.rs) uses the crate only for key generation, PKCS#8 conversion and public modulus/exponent extraction. Private-key signing uses ring and no decryption API is exposed. The timing-sensitive private operations described by the advisory are not used through this crate. Re-review before adding them. |
| CLI | [RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436.html), paste 1.0.15 | An unmaintained proc macro performs identifier concatenation at compile time in `core/manifest.rs` and `core/mutable_enum.rs`. No runtime vulnerability is identified by this notice. Replacement remains maintenance work. |

## Reviewed application boundaries

- **Identity and grants:** authentication is required for non-public contracts;
  missing verifiers fail closed. Native session state is installed only after
  authentication and RBAC. Header presence alone cannot grant access. Opaque
  sessions retain revocation/tenant checks. IAM-base uses salted Argon2id;
  Better Auth compatibility uses its configured scrypt format. Neither format
  automatically upgrades an existing password hash during this review.
- **JWT/JWKS:** algorithms and endpoints come from application policy. JWK key
  usage, key ID, algorithm and configured claims are checked. Critical headers
  and unencoded payloads fail. Remote JWKS disallows redirects, credentials and
  non-HTTPS URLs except loopback development; timeouts, response limits, caching
  and unknown-key refresh cooldowns are explicit. Applications must select
  issuer/audience requirements in `Validation`; the basic HS256 principal helper
  is not an issuer/audience policy builder.
- **HMAC/replay:** signature comparison uses constant-time primitives. The
  request signer includes method/path/body/time/nonce; clock-skew policy is
  explicit. Signature verification alone does not consume nonces. Generated
  platform callbacks share an in-process nonce store; replicated deployments
  need the documented atomic shared implementation. General module HMAC callers
  still own their replay policy.
- **Encryption:** tenant-derived keys, authenticated AES-GCM decryption,
  previous-key rotation and safe errors are covered by the field/cache/object
  fixtures. `FieldEncryptor` avoids Debug and zeroizes stored key material.
  Legacy v2/v3 deterministic encryption preserves TS equality behavior and leaks
  equality. Prefer random-nonce v4 `seal` plus blind indexes for new fields.
  The v4 `anon` metadata is clear and unauthenticated; it cannot authorize access.
- **Better Auth identity storage:** merged TypeScript main binds accounts and
  signing keys to the IAM service encryption context, independently of the
  current organization. The real PostgreSQL adapter fixture verifies context
  restoration, direct wrong-context decryption rejection, encrypted password/key
  columns, and fail-closed legacy account/key policy markers. Existing ciphertext
  requires an explicit migration. Native auth storage separately binds encrypted
  records and forced RLS to its startup-configured `TenantContext`; request claims
  cannot select that storage namespace. Decoded-record protocol interoperability
  does not imply shared MikroORM/native ciphertext tables.
- **Secrets/artifacts:** contract auth metadata describes the method without
  embedding signing secrets. Runtime key/configuration injection belongs to the
  application. The SDK exposes caller-owned headers/hooks and should not receive
  a backend master key. Test fixtures use named test-only credentials and owned
  temporary output; they do not provision production keys.
- **HTTP/MCP:** protected HTTP and MCP calls pass the same router authorization
  boundary. The generated runtime fixtures exercise initialization, discovery,
  invocation, Origin rejection and shutdown. Body/stream/schema limits are
  bounded at their documented layers. Deployment-wide connection quotas, rate
  limits and public Host/Origin policy remain deployment configuration.
- **Generated SDK diagnostics:** Rust client, transport request and typed request
  Debug implementations omit URL, parameter, header and body values. Transport
  errors strip reqwest URLs. Application response models retain ordinary
  Debug behavior. Generated consumers assert credential/body redaction.
- **Generated SDKs:** path/query values are encoded, multipart boundaries belong
  to the encoder, caller cancellation is preserved and SSE event buffers are
  bounded. Base URLs and fetch/HTTP clients are trusted caller configuration.
  Multipart responses have explicit byte/part bounds. Other ordinary
  non-streaming response bodies are buffered; applications needing a
  strict total response-size policy must supply/enforce it at their transport.
  Cookie/redirect behavior follows the selected fetch/reqwest client; explicit
  cross-origin authentication policy remains application-owned.

## Reproduction and coverage

```sh
cargo install cargo-audit --version 0.22.2 --locked
# The CLI lockfile is generated locally rather than checked in.
cargo generate-lockfile --manifest-path cli/Cargo.toml
python3 cli/tests/rust_security_audit_test.py
FORKLAUNCH_SECURITY_REPORT=/tmp/rust-security-review.json \
  python3 cli/tests/rust_security_audit.py
```

`CARGO_AUDIT_BIN` selects an isolated scanner installation and
`FORKLAUNCH_RUSTSEC_DB` selects its database directory. Offline gate tests verify
that reviewed findings remain visible while new advisories, changed package
versions, filters and scanner failures are rejected. CI publishes the audit
report. The full auth/provider release review remains coupled to the open
provider, storage scalability, multipart/MCP, mixed-app and deployment tasks;
this engineering review does not mark those implementation gates complete.


Verified-session policy extension (2026-10-13): the optional router authorizer
runs only after credential verification and grant enrichment. Missing/rejected
credentials never invoke it; an authorizer denial prevents handler dispatch and
retains correlation/telemetry handling. Installing it explicitly replaces native
RBAC for authenticated routes, so the application owns the full route policy.
The TypeScript compatibility policy intentionally uses permission-before-role
precedence and exact scopes rather than native cumulative checks/hierarchy
implication. It is opt-in; native defaults are preserved. The 1,495 actual-source
policy comparisons and generated signed-JWT HTTP/MCP acceptance/rejection cases
cover this boundary. No dependency changes are introduced.


OIDC authorization-code adapter review (2026-10-13): operator-owned issuer/client/
callback configuration controls all outbound URLs. Bounded transport enforces
HTTPS or loopback HTTP and no URL credentials/fragments/redirects. Discovery
checks issuer identity; code exchange consumes non-cloneable ten-minute state,
compares state in constant time, binds provider/client/callback and uses S256 PKCE.
Applications must retain pending state in the initiating browser's server-side
session. RS256 ID tokens are checked by openidconnect for signature, issuer,
audience, expiry and nonce; the adapter also checks present azp, nonempty subject,
future iat and a present access-token hash. Public/confidential client wire tests,
invalid metadata, state/config mismatch, oversized declared/chunked bodies,
redirects and invalid/substituted token tests pass. Errors/Debug omit secrets.
Refresh grants bind newly verified ID tokens to original trusted issuer, subject,
audience, optional nonce and authentication time. Missing ID tokens remain absent;
missing replacement refresh tokens retain the supplied token. Non-Bearer and empty
tokens fail closed. Original claims and tokens must come from trusted server
storage; the application owns rotation concurrency and persistence. ID-token
verification now fetches fresh keys from the discovery-pinned JWKS endpoint,
without credentials, redirects, discovery-key fallback or token-grant retries.
The existing 1 MiB response bound and 15-second request timeout apply. This adds
one network request per returned ID token; unavailable keys reject authentication.
Refresh responses without an ID token do not fetch keys. Owned-provider tests
cover changed key material, revoked/empty/malformed/oversized key sets and exact
request counts. Account/session provisioning remains application-owned. The all-lockfile audit has no unreviewed
findings after the public-verification-only RSA review above.


Versioned field/path review (2026-10-13): native field callbacks run after
transport authentication and cannot change the wire credentials used by the
session verifier. Field callback errors reject a version without exposing error
text; runtime registration validates fields/versions/routes. Selected values are
validated before transforms, so intentionally absent transformed values do not
undergo duplicate wire-presence checks. Path captures decode once after complete
route-shape matching; malformed escapes and non-UTF-8 values return 400 regardless
of schema-validation mode. Encoded slashes stay within the matched capture and raw
URI bytes remain available to authenticators. percent-encoding was already in the
lockfiles; this adds a direct dependency without a version change.


Billing webhook reconciliation review (2026-10-13): migration 4 adds tenant-RLS
intent and append-only-by-runtime-grants resolution audit tables. An advisory
transaction lock serializes delivery and resolution; intent commits on a separate
connection before callbacks start. Errors/cancellation leave ambiguous attempts
blocked instead of blindly replaying external effects. Resolution requires an
actor/correlation context and the exact inspected attempt identity. It cannot
steal an active callback lock. Audit, receipt and pending-marker deletion are one
transaction; failed audit insertion leaves the attempt blocked. Resolution does
not independently verify provider truth: trusted operators must inspect/repair
state before asserting Applied or RetryAllowed. No automatic public resolution
endpoint is exposed. Raw payloads/secrets are not stored. Configure at least two
pool connections and share store clones for admission control.


PayPal capture reconciliation review (2026-10-13): authenticated order retrieval
uses the configured provider origin/merchant credentials, bounded response size
and existing redirect/retry restrictions. Repair accepts only a matching order ID,
CAPTURE intent, COMPLETED order and one final COMPLETED capture with exact amount,
currency and purchase-unit order reference. Pending/refunded, multiple-unit/capture
or ambiguous evidence is rejected without mutation. It never invokes capture,
refund or order delivery. Raw order responses may contain PII and are not audited
or logged; the audit retains only payment/provider/capture identifiers, amount,
currency, prior status and the verified operator context.

Memory/PostgreSQL compare the inspected payment snapshot under the mutation lock.
Payment repair and actor audit are atomic; audit failure leaves the original row.
A delayed legacy claim release checks verified-capture audit evidence and cannot
reset the payment. Migration 6 enforces tenant RLS on audit metadata; runtime roles
need SELECT and reconciliation operators additionally need INSERT. Trusted internal
VerifiedCapture values must never be bound directly to an unauthenticated request.
This does not establish a distributed transaction or recover every provider state;
uncaptured claims, ambiguous/multiple captures and cancelled-order references still
require review. Provider status semantics reference the official
[PayPal Orders documentation](https://developer.paypal.com/api/orders-v2/save-with-purchase/paypal).


Stripe capture recovery review (2026-10-13): retrieval expands `latest_charge`
using configured merchant credentials and the service's Connect account header.
Repair requires matching intent/charge identity, order metadata, currency and full
amounts, succeeded/paid/captured state, zero refunded amount and no refund/dispute
flags. Missing/unexpanded/substituted, partial or unsettled evidence is rejected.
Only GET requests occur; the shared snapshot/audit transaction repairs the payment
without issuing capture/refund or order transitions. Raw responses/client secrets
are not persisted. Live tests verify Connect headers, expansion, twenty invalid
provider variants and absence of mutation requests. Field semantics follow the
[Stripe PaymentIntent](https://docs.stripe.com/api/payment_intents/object) and
[Charge](https://docs.stripe.com/api/charges/object) references.


Custom JWT helper review (2026-10-13): the callback is supplied by trusted Rust
server code, never deserialized from request options. Input validation and default
expiry parsing precede callback invocation. No local key store or JOSE claim setter
runs on this path, matching the actual custom-sign branch. The callback owns claim
validation/signing/key publication. Callback errors are sanitized and no private
failure text is emitted. Tests use a store that panics on access and a pending
signer bounded by caller timeout. Finite JSON claims are supported; non-finite JS
values are explicitly rejected. This helper does not silently switch public token
or JWKS endpoints to external keys.


External HTTP JWT signing (2026-10-13): explicit trusted configuration accepts an
absolute HTTP(S) public-key URL without credentials/fragments and a recognized
asymmetric algorithm. This URL is metadata, never a request-selected fetch target.
The signer receives claims only after authoritative session validation; absent,
forged and revoked sessions cannot invoke it. Local JWKS publication is disabled
with 404; signing success/failure creates no local private-key records. Callback
errors retain sanitized helper errors. Live TCP tests independently verify signed
claims and confirm revocation, callback count, failure redaction and graceful
shutdown. Applications remain responsible for external key publication/rotation,
verification configuration, actual callback algorithm enforcement and deadlines.


Basic compatibility and explicit verifier errors (2026-10-13): compatibility is
selected only by trusted application code. It reproduces TS's permissive Node
base64 and first-two-colon behavior; it does not alter strict native parsing.
Credential rejection cannot create a principal or reach the handler. Callback
passwords are never included in public errors. `with_response_authenticator`
accepts only server callbacks and successful sessions still undergo RBAC/custom
authorization. Cookie routes retain their dedicated verifier. Explicit response
errors must contain public text; applications must not forward provider errors
or secrets verbatim. Live TCP HTTP/MCP tests verify missing/malformed/denied
credentials, successful typed sessions, subsequent role denial and shutdown.


AsyncAPI fixed replies (2026-10-13): reply objects share offline pointer resolution
and document bounds. Only the operation's own channel and its declared messages
are accepted; runtime destinations and external references never initiate network
fetches. Reply targets with unexpected fields, cycles, sibling references or
ambiguous payloads are rejected. Invalid-reply CLI checks preserve both SDKs and
the user-owned source document. This adds message-direction metadata, not automatic
request correlation or authorization. Existing security/header restrictions apply.


Basic custom resource verification review (2026-10-13): the opt-in async decoder
receives the first raw token only after exact prefix/header checks. It supersedes
base64 parsing and synchronous login, so the application callback must verify the
credential, not merely decode it. Null claims reject authentication. Other JSON
claims are preserved as in TypeScript; applications validate their shape and bind
the principal before creating an authenticated session. Explicit callback errors
must contain public text. Cancellation drops the callback future without retries.
The generated fixture verifies credentials before session shape/role checks and
never authorizes malformed or null sessions. Native structured error envelopes
remain unchanged; TypeScript response-body equivalence is not claimed here.


JWT custom resource verification review (2026-10-13): an opt-in trusted async
callback verifies the first raw token after exact Bearer/custom prefix checks.
This callback owns signature and claim verification; the helper is not a JWT
payload decoder. Falsy JSON results reject authentication, and callback failures
become a generic 403 without logging or exposing their details. Applications
validate session shape and bind a principal before normal authorization. Actual
TypeScript comparisons cover both callback success and failure across header and
claim shapes. Native JSON error envelopes and strict built-in JWT verification
remain unchanged.


Worker control-service role review (2026-10-13): the generated control entry
point runs before any job-backend connection, queue construction or retention.
Tokenless mode exposes only health/readiness; authenticated commands require a
distributed publisher and retain constant-time bearer checking and scoped Redis
publication. The process never subscribes as a consumer, so remote drain does not
terminate it and readiness is explicitly local. Publisher counts do not imply
acknowledgement. Shutdown retains the existing bounded HTTP drain. CLI capability
metadata is withdrawn when sync detects unsupported custom/legacy entry points;
platform argv selection requires an explicit compatible native worker role and
never invokes a shell. Real Redis tests verify publication scope, two-consumer
fanout without an extra subscriber, storage-independent startup and shutdown.


HMAC compatibility dispatch review (2026-10-13): the opt-in adapter parses the
first four header components, preserves embedded equals, ignores suffix components
and selects an operator-owned dictionary key before awaiting a trusted verifier.
It returns no session/principal and hides all credential material in Debug.
The callback must reject absent keys and verify method/path/raw body; timestamp
text remains raw, and freshness plus atomic replay policy remain application-owned.
No legacy JavaScript Date parser is introduced. The generated fixture uses the
existing strict RFC3339 verifier with a 60-second skew limit and rejects missing
keys, invalid signatures and expired credentials on HTTP and MCP. Error messages
from custom callbacks must be explicitly public; native structured envelopes and
strict request-HMAC APIs remain unchanged.


Ordered session callback review (2026-10-13): the opt-in session resolver runs
only after application credential verification. Validation sees immutable original
claims; parsed transformations/defaults are intentionally not substituted. Trusted
resolvers can hold request/tenant context and must obtain grants from authoritative
application state. Scope, permission/role, subscription and feature callbacks run
sequentially with short-circuiting; absent required callbacks return explicit 500s.
Missing scope callbacks preserve the TypeScript skip behavior, so applications
requiring scope enforcement must configure one. Callback errors are explicitly
public and must be redacted at the provider boundary. Dropping the policy future
stops polling it, without retries or starting later callbacks; independently
spawned provider work remains application-owned.

The adapter creates no principal and does not replace schema validation. Generated
JWT-authenticated HTTP/MCP tests cover validation, each grant rejection, permission
precedence, missing callbacks and successful original-claim propagation. 2,335
actual middleware comparisons cover scheduling and unchanged-claim behavior with
controlled validator outcomes. Native tests verify default missing callbacks and
cancellation; full schema/error-envelope equivalence remains open.


Auth error format review (2026-10-13): `TypeScriptText` changes only the wire
representation of existing credential/policy rejections. Trusted callbacks still
own the public status/message, including explicit 500 configuration errors; they
must not return private provider details. Credential verification and policy
checks still precede handlers in both modes. Native JSON remains the default,
with body correlation IDs; both modes retain request telemetry and HEAD semantics.
Canonical real TypeScript comparisons verify exact auth text/media, while native
regressions confirm request-validation errors are unaffected. Generated HTTP/MCP
fixtures exercise both representations and error forwarding.


Terminal Stripe cancellation review (2026-10-13): recovery performs an authenticated
GET with the configured Connect account. It requires `canceled`, positive provider
cancellation time, zero received/capturable funds, explicit null latest charge and
matching intent/order/amount/currency. Stripe documents canceled intents as terminal
([API reference](https://docs.stripe.com/api/payment_intents/cancel)); this implementation
deliberately rejects any prior charge rather than inferring refund/authorization
state. Pending, ambiguous and PayPal cancellation states remain review cases.

The local snapshot is compared under the payment lock. Status and operator audit
commit together; audit insertion failure rolls back the update. Tenant RLS and
restricted SELECT/INSERT grants apply to migration 7. Runtime roles need SELECT
on the new audit table before serving the updated confirmation paths. Verified
cancellation blocks later claims/confirmation and contradictory capture evidence;
verified capture blocks contradictory cancellation. References remain available
for investigation. No provider mutation, order transition, delivery or public
reconciliation endpoint is added. Trusted evidence structures must not be bound
directly to caller-controlled input.

### Redis inspection and opt-in safe-value parsing (2026-10-13)

Typed Redis existence/list/delete operations do not decode or authenticate stored
values. They reveal key existence to the already-authorized cache caller, matching
TS EXISTS/DEL semantics, including lists and corrupt ciphertext. Tenant key
namespaces and application authorization remain caller-owned. Batched Redis
existence checks use MULTI/EXEC; batch deletion validates all keys before DEL.
Glob scans are explicit and separate from literal-prefix scans. Neither is a
snapshot. Native raw client access bypasses the surrounding cache validation and
codec, as the existing TS getClient escape hatch does.

The new `JsonRecordCodec::with_typescript_safe_values` profile permits non-JSON
plaintext to decode as a string, and writes top-level strings without JSON quotes.
It can change the type of JSON-looking strings on read; strict JSON stays default.
UTF-8 checking and ciphertext recognition/authentication run before parsing. An
invalid ciphertext, missing tenant or wrong tenant never takes the plaintext
fallback. Typed decoding errors also remain errors, even when the input was valid
JSON of another type. Actual TypeScript safeParse/safeStringify source now replaces
the old fixture's JSON.parse/JSON.stringify substitutes. Both envelope formats,
wrong-context/corrupt-record failures and plaintext migration pass real Redis
interop checks. Native unit coverage checks typed failures and invalid UTF-8.
JavaScript class/BigInt and IEEE-number compatibility are not claimed by this
bounded JSON-compatible-value profile.

Typed Redis compatibility queue methods apply truthiness only after authenticating
and decoding every visited record. Empty arrays/objects remain values; null,
false, zero and empty strings are omitted before conversion to the requested Rust
type. Wrong-tenant or corrupted ciphertext cannot become an omitted/falsy record.
Peeks do not consume on failure; dequeues remove their selected items before
decoding, as ordinary native/TS queue methods do. Count limits apply before
filtering. Explicit TypeScript zero-count peeks read the whole current queue;
the ordinary native zero-count method remains empty. These APIs accept nonnegative
integer counts and do not emulate negative/fractional JavaScript inputs.


### Optional base IAM RS256 sessions (2026-10-13)

The `jwt-sessions` feature allows explicit RS256 issuance while preserving opaque
sessions by default. All four signing environment variables must be present and
valid; malformed keys, partial settings, empty/control-character metadata and
oversized tokens fail closed with sanitized errors. Construction signs a local
probe, and production signing runs on a blocking worker. No Debug implementation
or private-key publication is provided. The checked-in RSA key is an existing
public test fixture, used only by tests.

The database clock determines issuance and the exact one-hour expiry stored in
PostgreSQL. Tenant, subject, issuer, audience, unique jti and sorted server-resolved
permission snapshots are signed. Email and password fields are omitted. Local
session validation hashes the entire credential and checks persisted expiry,
revocation and current user state under tenant RLS; it never accepts an unstored
JWT based on signature alone. Password changes, disabled users and logout retain
their existing invalidation behavior. Existing tokens survive signing-config
changes until their normal expiry/revocation. External stateless JWT verification
is a distinct trust boundary: callers must publish the public key and enforce
current authorization, tenant binding and revocation where needed. Directory
identities remain separate; cookie/CSRF and OAuth/OIDC lifecycle are not supplied
by this issuance feature.

Native signature/claim/error tests and restricted-role PostgreSQL tests cover
exact expiry, cross-tenant rejection, tampering, restart persistence and revocation.
The existing opaque-session PostgreSQL suite also passes. Generated fixture tests
use independently generated Node RSA keys and cryptographic verification.


### Base IAM browser cookie/CSRF composition (2026-10-13)

The opt-in `browser-sessions` adapter uses the existing shared CookieSessions
policy: Secure/HttpOnly/SameSite=Strict cookies, exact HTTPS Origin and constant-time
synchronizer-token checks for unsafe requests, with session/CSRF rotation on login.
BrowserIam reserves an internal HTTP namespace, so browser login/logout/CSRF
operations cannot become MCP tools or SDK contract operations. Shared HTTP limits,
CORS and telemetry continue to apply. Browser responses are no-store and never
return the underlying IAM credential. Shared CookieSession identity access is
trusted-handler-only; it does not itself authenticate a stored binding.

The supplied server-side cookie store contains an IAM bearer token and must be
protected as secret storage, with a dedicated tenant/application namespace and
durable shared persistence in production. No default production MemoryStore is
introduced. Authentication checks the current IAM token ledger, disabled status
and current grants. A store shared accidentally across IAM tenants still cannot
validate another tenant's underlying token. Re-login revokes the prior token;
logout revokes before flushing the cookie. A stale/resurrected cookie record
cannot bypass token revocation. Password changes invalidate the same ledger.
Cookie writes and IAM writes are not a distributed transaction: failed login
attempts try to revoke the newly issued credential; a cleanup/storage failure can
leave an unreachable credential until normal expiry. Generic cookie stores do
not provide an atomic global revocation ledger. Generated durable-store wiring
and OAuth/OIDC remain open.

Restricted-role PostgreSQL browser tests exercise CSRF rejection before credential
issuance, fixation prevention, current grants, disabled users, replacement/logout
revocation, password invalidation and shared-store tenant isolation. Existing
bearer tests pass with the extracted common verifier. The shared cookie suite
also verifies authenticated HTTP/MCP transport behavior. Tests use MemoryStore
only as an explicit fixture; reusing it across router instances is not evidence
of durable storage across process restarts.


### Durable encrypted IAM browser sessions (2026-10-13)

Migration 3 adds a tenant-RLS cookie table keyed by tenant, server-owned namespace
and SHA-256 cookie-ID digest. Cookie records, including their backing IAM bearer
credential, are sealed with the existing tenant encryption key ring. The sealed
payload binds namespace, original record ID and indexed expiry; mismatches,
corrupt records and unavailable keys return sanitized storage failures. No raw
cookie IDs or credentials are stored in cleartext. Debug output omits pool/key/
identity data. Plaintext records are bounded to 64 KiB; encoded input is bounded
before decryption. Runtime stores reject SUPERUSER/BYPASSRLS roles.

Creation retries bounded random ID collisions without overwriting another record.
Save only updates an existing, unexpired record. It cannot reinsert a deleted
cookie after logout/rotation or refresh an already expired record. Indexed expiry
uses whole Unix seconds and the database clock; expiration may precede the record's
fractional timestamp by less than one second. Expired-record purge affects only
the trusted tenant/namespace. Applications schedule expiry cleanup themselves.
Cookie and IAM ledger writes remain separate transactions; failure cleanup is
best effort, and an unreachable credential can remain until ordinary IAM expiry.

Generated IAM enables browser routes only with a valid exact HTTPS
SESSION_COOKIE_ORIGIN, using the service name as namespace and __Host-iam cookie.
A rename requires re-login. The configured encryption key ring is shared with
the encrypted directory store. Operators must apply migrations and grant runtime
CRUD on the browser table. TLS termination and trusted-origin CORS remain
application responsibilities. OAuth/OIDC flows are not implied by durable cookie
storage. Restricted-role tests cover ID collisions, encryption/redaction, RLS,
record/namespace/expiry swapping, retained-key rotation, expiry, oversize rejection
and deletion/expiry resurrection prevention, alongside the browser lifecycle.


Base IAM OIDC identity review (2026-10-13): optional `oidc-sessions` reuses the
framework provider's verified code exchange; the public service does not accept
a caller-constructed `OidcLogin` as proof. Exact tenant/issuer/subject bindings
require administrative provisioning and never auto-link by email. Identity/user
row locks serialize local issuance with unlink/disable/delete. Provider tokens
are dropped and provider grants do not override local grants. Existing local
session hashing, current-user checks and revocation remain authoritative.
Caller-owned browser-bound single-use pending state remains required. The new
blueprint dependency path uses the same reviewed openidconnect public RS256
verification path as the framework; the exact RSA advisory exception now records
both blueprint usages. No new private-key operation through rsa is introduced.
All three lockfiles pass the current advisory gate with reviewed findings visible;
the offline scanner regression suite passes. Evidence:
`/private/tmp/fl-iam-provider-security{,-tests}.log`.


Base IAM browser OIDC review (2026-10-13): pending authorization state can now be
explicitly serialized into a secret-bearing storage payload. It is never accepted
as verified identity. Restore requires authenticated encrypted server storage,
retains absolute expiry and provider/client/callback binding, and rejects oversized,
expired or future-lifetime records. The PostgreSQL adapter binds ciphertext to
tenant, namespace, hashed browser cookie and hashed state, and atomically removes
the record before network exchange. Independent restricted database connections
prove that exactly one callback can consume a pending flow.

CSRF-protected start/callback endpoints require the initiating browser cookie and
exact configured origin. The GET callback is a static CSP-hashed page, does not
reflect query values or set a new cookie, removes code/state from the URL, and
uses same-origin requests before a fixed `/` navigation. A real Chromium fixture
proves cross-site Strict-cookie exclusion followed by bound same-origin callback,
CSP execution, HttpOnly protection, absent Referer leakage and unbound-browser
rejection. Native PostgreSQL/provider fixtures separately cover issuance, replay,
process reconstruction, cookie rotation and revocation. This is not a claim of
one browser-to-Rust end-to-end deployment test.

The flow discards provider tokens and requires explicitly provisioned local
issuer/subject bindings. It does not infer account ownership from email or add
self-service account linking. Storage and IAM writes remain separate transactions;
unreachable credentials can remain until expiry after failed cleanup.


Better Auth diagnostic observer review (2026-10-13): native raw HTTP 5xx events
carry router templates, status and bounded allowlisted database identifiers.
Concrete token paths, query/cookie/body values, freeform error messages/stacks
and driver row details are excluded. Real PostgreSQL NOT NULL failure verifies
structural SQLSTATE/table/column preservation without private row data. Internal
response extensions are removed before returning the response. Trusted observer
callbacks are synchronous and should not block; panics are caught to preserve
HTTP behavior, but existing application panic hooks still execute. Default tracing
uses the same sanitized event. This observes handled 5xx responses as well as
unexpected failures, unlike the upstream unexpected-error-only hook.


Base IAM directory identity association review (2026-10-13): explicit administrative
bind/unbind/coordinated-delete methods require a trusted nonblank actor, tenant-
bound store and correlation ID. One-to-one composite foreign keys and forced RLS
prevent cross-tenant associations; no email/password/provider-claim autolinking
or grant copying occurs. Directory/auth/session/provider deletion and new admin
audits roll back together on failure. Ordinary directory deletion fails while
bound. Legacy credential-user deletion still cascades its association using its
existing unaudited API semantics; this change does not claim to add actor auditing
to that old entry point. Complete persisted token hashes, session expiry and user
disabled status gate association resolution; resolving a directory ID grants no
directory authorization itself. Restricted-role concurrency, rollback, stale-token
and unchanged-old-grants checks pass.
