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


Billing provider binding foundation review (2026-10-13): explicit administrative
account/mode/deployment configuration is not authenticated provider evidence.
Durable tenant-local ownership and nonblank actors are required for provisioning;
providerFields and public DTOs cannot automatically create bindings. Lookup is
scoped by tenant/account/mode/deployment/kind, with separate public UUID and
unchanged storage key. Exact generation revocation prevents stale handles from
removing a replacement association. Provision/revoke and their audit writes are
atomic; audit RLS allows only SELECT/INSERT to non-bypass roles. Binding handles
are lookup snapshots, not financial-operation capabilities. Existing wrappers
and webhooks do not use this foundation yet. Restricted PostgreSQL isolation,
race, immutable-audit and rollback checks pass.


Direct Stripe context review (2026-10-13): authenticated account/balance reads
verify trusted expected account and mode without API-key-prefix inference. The
non-serializable observation checks immutable client identity and a five-second
window beginning before verification. It is not proof of local ownership or a
guarantee that upstream credentials/proxies cannot change after the reads.
At this initial direct-only checkpoint, managed mode rejected before network
calls; the later generation-scoped gateway review below supersedes that limit. Malformed responses,
account/mode mismatch, 401/403, foreign/reconfigured clients and stale observations
are covered. Current wrappers do not yet consume this check.


Stored-session directory authorization review (2026-10-13): a verified JWT alone
cannot establish a directory identity. Exact complete persisted-token hashes,
expiry/enabled checks and an explicit tenant-local binding precede fresh grant
hydration under directory/user/session locks. Credential grants are replaced,
not merged, and the same snapshot provides organization scope. Unverified actor
context is cleared; snapshot reads carry the verified directory actor and request
correlation. MCP now forwards envelope correlation with precedence over tool
arguments. Generated HTTP/MCP and restricted PostgreSQL tests cover grant changes,
organization movement, invalid/unpersisted JWTs, disable/unlink/revoke/delete and
restart. This remains an explicit mode; default JWT-claims behavior is separate.


Scoped checkout read review (2026-10-13): direct account/mode evidence and explicit
tenant-local durable ownership precede the provider object fetch. The provider
object/customer and exact binding generation are checked; expiry is rechecked
after both network and database work. Joined statement snapshots avoid returning
a binding already revoked/replaced before that read, while future revocation is
not locked out. Sensitive decryption requires an audited actor and rolls back on
audit denial. Cache-only ownership and local customer UUID inference are rejected.
Provider financial mutations remain outside this opt-in read path; managed
identity context was added in the subsequent review below. Restricted-role race/failure tests and Clippy pass.


Managed Stripe context review (2026-10-13): signed, read-only context endpoints
bind instance/organization/account/mode/API version to a database-generated
binding generation. Identity changes, ABA rebinding and row recreation invalidate
previous generations. Raw scoped MikroORM queries bypass identity-map caches,
and checks surround provider reads and final durable reads. Tokens expire within
five seconds measured before verification and cannot authorize financial writes.
The scoped provider reader bounds time and UTF-8 bytes; errors use no-store.
Actual controller/HMAC/rate-limit tests, fresh PostgreSQL mapping queries,
restricted native ownership/audit failures and signed cross-language HTTP pass.
Full platform bootstrap and live provider/cloud deployment remain unvalidated.

Configured checkout cache references (2026-10-13): optional configured mapper
reads validate cache identity and provider object/mode/customer evidence while
preserving application IDs. This repairs public UUID forwarding to Stripe but
does not establish account ownership from cache data. Managed rebinding requires
the separate scoped gateway protocol; existing mutation paths remain separate.


Final diagnostic reentrancy review (2026-10-13): actual TypeScript probes found
that an undeclared error status could re-enter response validation from the
final exception handler and fall through to an HTML stack trace. Final framework
diagnostics now bypass enriched send using the raw Express sender, while normal
handler responses retain validation. Repeated 418 failures and the cross-runtime
201/declared-response cases pass without exposing handler body or stack details.
The opt-in native status diagnostic is redacted and correlated; it does not
rewrite arbitrary handler failures.


Directory audit context review (2026-10-13): authentication, handler CRUD and
compliance now share the framework request context. Generated restricted-role
audits separately verify snapshot reads and actual handler operations, including
HTTP/MCP create/read/update/delete and compliance export/erase. Verified directory
actors and HMAC service actors retain their originating correlation; absent
headers use the framework-generated request ID. No caller-supplied actor is
accepted through this context propagation.


Scoped direct payment-link reads (2026-10-13): the opt-in public UUID lookup
authenticates a fresh account/mode observation, resolves an encrypted tenant-
and deployment-scoped alias, validates the exact provider object, and rechecks
its generation and five-second lifetime after the audited durable read. Provider
reads leave local ciphertext and financial state unchanged. Restricted PostgreSQL
tests cover foreign aliases, revocation/ABA, denied audits, provider evidence,
and expiry while waiting for a database connection. Managed transport currently
fails before network access; the default provider-ID service remains separate.


PostgreSQL member-query review (2026-10-13): generated services explicitly
select database-compatible UUID/timestamp/filter/sort semantics. Allowlisted SQL
identifiers/operators and bound operands evaluate only transient public member
projections, with mandatory organization scope inside the tenant transaction.
Queries are shape/size bounded before cloning; projections reject more than
4,096 candidates or 1 MiB rather than silently truncating. Ordinary indexed pages
retain bounded page hydration beyond that candidate count. These are projection
bounds, not whole-process memory bounds: a storage page can decrypt up to 256
existing rows before projection checks. No new plaintext storage/indexes are
introduced; database parameter logging remains an operator policy. Restricted
PostgreSQL tests cover organization isolation, UUID hydration, budget rejection
and rollback cleanup; 434 actual ORM/native comparisons pass. Direct library
stores retain the previous memory-adapter profile unless explicitly opted in.


Managed payment-link extension (2026-10-13): a dedicated signed endpoint now
performs fixed-kind payment-link reads using the existing generation/account/mode
context protocol. Exact-path HMAC, unknown-field/query rejection, no-store,
expiry across provider/database waits and final binding checks are preserved.
Checkout and payment-link IDs/objects cannot cross kinds; no caller-supplied
provider path is accepted and the raw proxy allowlist is unchanged. Thirty-two
actual companion tests, cross-language signed HTTP/mock checks, both restricted
PostgreSQL native read fixtures and strict Clippy pass. The PostgreSQL fixtures
serialize GRANT/REVOKE to avoid shared catalog races. No financial mutation or
automatic alias provisioning is added. Full platform bootstrap and live provider/
cloud behavior remain outside this receipt.


Auth observer provenance review (2026-10-13): private typed markers distinguish
handled API failures from unexpected failures without trusting HTTP status, error
codes or caller-provided JSON details. Opaque BlueprintError context is excluded
from Debug/Display/equality/public details and preserved on clone. The generated
preset explicitly selects unexpected-only callback dispatch; legacy callbacks
remain all-5xx unless changed. Safe structural tracing still covers every 5xx.
Native tests verify spoof resistance, unchanged response bytes, bounded diagnostic
metadata, callback panic isolation and trace/callback counts. Actual-source
oracles cover get-session, individual/all/other session revocation and invalid
permission schema versus invalid JSON. External struct literal callers must
migrate to error constructors; no storage migration is required.


Signup/signout fault review (2026-10-13): only signup user insertion failures
become handled 422 errors. Account/session creation and transaction failures
remain unexpected, with empty 500 responses, no cookies and one callback under
the reference mode. HTTP signout now mirrors the reference by clearing its three
session cookies and returning success despite storage errors; the strict service
API still returns errors. A failed deletion can leave the persisted session
valid, so HTTP success is not proof of server-side revocation. Native tests verify
rollback and exact independent cookie expectations, including secure variants.
Diagnostics use sanitized structural fields, never raw errors or credentials.
Transaction-completion injection is an adapter fixture, not a real PostgreSQL
COMMIT-failure receipt.


Static CORS and billing/JWT options review (2026-10-14): compatibility CORS is
explicit opt-in and mutually exclusive with native CorsLayer configuration.
Reflected and allowlisted origins remain trusted operator policy; dynamic origin
code is outside this static surface. The 204 Content-Length transport difference
is asserted in differential tests rather than hidden by normalization.
PaymentLink create structural checks run before SDK/HTTP provider calls and
preserve nested options; they are not complete provider payload validation.
Base cache-only update hooks validate public identities and DTOs without granting
cached provider references mutation authority. Optional JWT-header suppression
skips signer/storage side effects, while explicit token issuance remains enabled.
Configurable JWKS grace changes only public-key publication, not expired-key
signing eligibility. Typed durations reject unsupported precision/range before
storage access. Actual-source and focused native proofs are recorded in the
parity inventory; broader authentication/provider gates remain open.


Cache-key and parser-order review (2026-10-14): checkout/portal reads and updates
now bind cached DTO identity to the requested key before selecting a provider
customer or reaching ORM/mapping/provider work. Focused base/Stripe tests cover
both backup modes and prove zero external/storage mutations on denial.
Opt-in unversioned JSON parsing now precedes authentication consistently even
when schema validation is disabled. Parser verifier failures remain redacted,
and original bytes/decoded values are exposed separately. Unsafe numeric and
unrepresentable Unicode values fail explicitly within the supported profile;
MCP and generic schema validation remain separate gates.


Optional JWT subject callback review (2026-10-14): only verified persisted
session/user snapshots reach the trusted resolver. Snapshots are read-only and
Debug-redacted, but accessors may expose session tokens to server callback code,
which must not log or return them. Subject changes affect token claims only;
stored/cookie ownership remains unchanged. Hook failure precedes signing and
local key creation, returns empty500 and reaches unexpected-error observation.
Custom subject authorization mapping is application-owned. Actual/local/external
fixtures cover these boundaries; the callback is disabled by default.


Subscription binding foundation review (2026-10-14): migration6 stores an
explicit administrative provider-customer/local-party assertion and copies it
into append-only audit records. Provisioning locks and audited-decodes the owned
durable row; reads reject changed external/party relations and sealed generations.
Existing aliases/ciphertext and tenant RLS remain intact. This is not Stripe
ownership verification and grants no provider mutation or webhook completion.
Restricted-role tests verify migration compatibility, wrong-scope/tenant denial,
revocation/reprovisioning, and audit-failure rollback. Authenticated provider
inspection remains a separate required boundary.


Preview gateway review (2026-10-14): the native client uses only trusted instance
gateway credentials, fixed capability paths, existing HMAC signatures and fresh
nonces. No provider credential or desktop login is used. Catalog bytes are capped
before decoding, validated against exact issued-ceiling fields and expiration,
and cannot masquerade as managed monthly usage/compliance entitlements. HTTP
failures retain status/retry metadata without provider response text. Browser
UTF-8/numeric compatibility is scoped to the new bounded path; existing provider
transport policy remains unchanged. Chat has no streaming method, preserves
absent timestamps, and retains the native JSON value representation limits.


JSON transformation review (2026-10-14): schema-bound transforms require matching
parser registration and explicit400 metadata. They run atomically before auth,
with original bytes retained and no partial output on ignored validation failure.
Only body presence/media checks are skipped after registered schema success;
other native request checks still run. Warning logs contain static diagnostics
and operation/correlation metadata, never callback payload or input. Callbacks
are trusted server code; anonymous rejection text must remain safe for exposure.


Paused-effect acknowledgement review (2026-10-14): same-account authenticated
event/current-subscription reads plus exact sealed ownership and local inactive
state can complete delivery bookkeeping only. Row/binding xmin and generation,
exact attempt identity, locks, atomic audit and finite context checks protect
completion. There is no causal pause-epoch proof, original-HMAC verification,
provider mutation or subscription write. Post-COMMIT expiry is explicitly recorded
and ambiguous commits require receipt inspection. Managed mode rejects before
network I/O. PostgreSQL/HTTP proofs include ABA, rollback and real lock-delay
expiry. Extra binding UPDATE permission belongs only to the trusted operator
for row locks; default runtime grants are unchanged.


Optional JWT payload review (2026-10-14): trusted immutable verified-session
snapshots feed an object-valued async callback before subject resolution/signing.
Callback failure produces empty500 without key work; invalid local JOSE claims
retain upstream key-mint side effects before rejection. The subject policy wins
over payload sub, while stored/cookie ownership is unchanged. Custom claims are
application policy and must not expose secret snapshot fields. Actual JOSE and
external-signer fixtures verify precedence, failures and suppression; no callback
is installed by default.


Whole-request projection review (2026-10-14): private successful field wrappers
mark complete trusted schema output; only native request-schema checks skip.
Authentication and role enforcement still run in all modes, with explicit401/403
regressions. Original authorization/cookie headers remain the native credential
source even when a projection differs. Transform failure commits no field/body
changes. Raw bytes/query remain available; typed projection extraction is an
explicit handler choice and failed permissive validation needs wire fallback.


Managed paused-evidence review (2026-10-14): the dedicated signed endpoint pins
all fixed provider reads to the token's verified account/API version, with fresh
instance/organization/mapping-generation/mode checks between awaits. Unknown
keys/query, malformed IDs, stale/remapped scope and oversized aggregates deny.
Native envelope and sealed local ownership checks precede bookkeeping completion.
Present event-account metadata must match; absent metadata relies on the scoped
authenticated SDK request, not webhook metadata assumptions. There is no raw
proxy expansion, financial/provider mutation or original-signature/causal proof.
Actual platform, signed-wire and PostgreSQL twins verify these boundaries locally;
live provider/deployment validation remains separate.


## Current IAM PostgreSQL follow-up receipt (2026-10-14)

Both ignored IAM PostgreSQL tests pass against the owned local test database
in256.78 seconds. The exact already-validated all-feature test executable was
copied and run directly, allowing the archive Cargo build to continue independently.
Binary SHA256710b5c6c51712d961fd15c19ce0a871fb479667c20f15f1e2d0aa24c212a88e5
corresponds to the current lifetime slice atd406df5617a20017a26498aeeafb6dd378239c59.
This includes encrypted durable storage, indexed large-record lookups, roles/
permissions/session behavior, tenant isolation, rotation, audits/transactions and
the corrected live HTTP/MCP signout assertions. The diagnostic-observer test uses
a transaction-local TEMP table and rollback; the long storage test owns a unique
restricted role and isolated tenants.

Post-run verification confirms the exact owned role is absent and its fixture
records/audit/unique_claims/lookup_claims are zero. The copied executable's hash was
rechecked before removal. Logs and metadata:
/private/tmp/fl-iam-pg-validated-{run,list,cleanup}.log and
/private/tmp/fl-iam-pg-validated-metadata.json. No cloud/vendor service was used;
this does not close broader Better Auth adapter/options coverage.


## Ecommerce durable provider-scope foundation receipt (2026-10-14)

Additive ecommerce migration9 records explicit administrative payment scope:
tenant, provider, deployment, merchant/account, mode, reference, linked order,
amount/currency and opaque generation. Provision compares the caller's payment
snapshot against a locked durable row and verifies linked order key/body identity.
Reads check payment/order key/body identity and the immutable local relation;
revocation compares exact generation/scope. No provider call, payment/reference
write, automatic backfill or public route is introduced. This is an administrator
assertion, not provider ownership proof or permission to mutate money.

Forced tenant RLS protects bindings; audit RLS permits only INSERT/SELECT even
under broad non-bypass table grants. Actor/correlation are required; audit failure
rolls back provisioning/revocation. Scoped reference uniqueness rejects duplicate
local bindings. Foreign-key deletion cascades remove bindings when payment/order
records are deleted, preventing delete/recreate revival while retaining audit.
Same-record relation changes restored later may match again; effect workflows
must independently compare current snapshots and generation under locks.

Restricted PostgreSQL foundation proof passes, including exact old migrations1–8
followed by9, opaque ciphertext/legacy nullable audit preservation, tenants,
key/body identity, grant rollback, append-only audits, uniqueness, revoke/reprovision
ABA and delete/recreate. Both existing reconciliation PG tests, seven base
regressions and strict all-feature/all-target Clippy pass:
/private/tmp/fl-ecommerce-bindings-{pg,reconciliation,base,clippy}.log. An initial
test-only reference collision was corrected and its exact owned role/tenant cleaned.
Default application routes receive no administrative grants. Short-lived
provider-account/mode verification, cancelled-order repair and reference retirement
remain open; retain existing references for review until that integration exists.


### Stripe resume identity and generated cancellation guard (2026-10-14)

Configured and custom-fallback TS resume now reject substituted or malformed
provider response IDs before returning secrets, matching the existing native
identity check. Fixed diagnostics disclose no provider secret/body. Actual32
cases and native regression pass. Fresh generated checkpoint24a additionally
passes HTTP/MCP cancellation-during-provider-read checks: secrets are withheld,
references retained, no replacement provider payment created. This is a pair of
consistency observations; a race remains after the final check, previously issued
secrets are not revoked, and no provider cancellation or monetary action occurs.
See the dated inventory receipt for exact checkpoint and preserved failure/final
logs.


### Read-only direct ecommerce provider context (2026-10-14)

The opt-in Stripe inspector binds authenticated account/mode reads to an immutable
client and a five-second observation window, then checks the administrative
binding's complete generation/identity before returning a redacted observation.
Restricted PG tests reject replacement ABA and expiry after final database waits;
Connect rejects before I/O. This grants no monetary or repair authority and has
no public route. Operator tenant assignment and remote post-check changes remain
outside this proof. Existing reconciliation APIs are unchanged; see inventory.


### Scoped cancelled-order terminal no-charge repair (2026-10-14)

The new opt-in operator flow requires fresh authenticated direct Stripe scope,
explicit canceled/no-charge evidence and locked unchanged order/payment/binding
snapshots. It rejects succeeded/captured local history. Failure, existing audit and
new tenant receipt commit atomically; precommit freshness/audit errors roll back.
COMMIT ambiguity uses a retained receipt UUID, and postcommit expiry returns an
explicit recorded outcome. References remain intact; no provider write or financial
action occurs. Restricted PG tests cover CAS/ABA, deadlines, tenant isolation,
append-only receipts, rollback and additive migration compatibility. Broader charge
histories and Connect/managed/PayPal flows remain outside this bounded profile.


### Explicit connected ecommerce scope (2026-10-14)

Connected inspection/terminal-no-charge repair now authenticate all provider reads
under the same exact Stripe-Account and immutable client/configuration, with the
same deadline and locked durable snapshot requirements as direct APIs. Direct
methods still reject Connect. SDK/native and restricted PG twins cover wrong-profile
zero I/O, header/config identity, ABA, deadlines and audit/receipt rollback. No
provider write, financial authority or reference clearing is introduced.


### Typed custom JWKS callback boundary (2026-10-14)

Fixed server-owned callback context is independent of request inputs. The application
must implement tenant isolation/encryption in custom storage; mixed default/custom
operations are not atomic. Returned validated native key material/ID govern signing,
and callbacks run after releasing fallback locks. Secret byte buffers zeroize and
have no Debug/serde exposure; callback errors are sanitized. Active hooks disable
administrative rotate; external signers bypass them. Seven adapter cases, all87 IAM
tests and strict Clippy pass. Upstream encrypted JWK wire/public-only/legacy records
remain outside this explicit profile. Copied-binary PG checks and provenance are
tracked in the dated inventory receipt.


### Trusted local key profile and bounded legacy normalization (2026-10-14)

Server-owned validated algorithm/RSA configuration now governs local issuance,
rotation and missing/null native key metadata. Normalization is read-only, bounded
to16 KiB public JSON and strict public coordinates/metadata; conflicting/private
fields cannot override the configured profile. Exact key-ID constraints reject
unusable selections without replacement minting. Explicit records preserve prior
verification behavior.96 native IAM tests and16 actual JOSE observations pass,
with strict Clippy and unchanged stored-document checks. Typed native PKCS#8
compatibility does not import upstream encrypted JWK strings or arbitrary metadata.


### Versioned parser preparation and MCP framing (2026-10-14)

Prepared version outcomes now bind to changed parser/body/field hooks through a
fresh private identity; untouched clones retain sharing. The opt-in JSON wire
profile rejects stale declarations and unsupported request modes before handlers.
MCP framing derives only from encoded bytes, retaining rejection of caller-supplied
transport headers. Exact-body, mutation/reuse and real HTTP/MCP tests pass alongside
HTTP66/Axum140/MCP21 and strict Clippy; dated inventory records retained boundaries.


## Resolved signing capability and configured diagnostics (2026-10-14)

The new ResolvedSigningKey is a trusted server capability imported from validated
private material, without Debug/Serialize/Clone or raw JSON construction. Its
reuse deliberately bypasses storage/expiry/revocation checks; deleting a source
record cannot revoke an already imported private key. The caller owns lifetime.
The local API is not wired as caller-controlled HTTP key selection and does not
replace external signing or strict key-ID checks. Existing claim validation and
private-material bounds apply; no new cryptography/dependency was introduced.
Actual JOSE and focused native public-signature/error/no-I/O tests pass, documented
in the parity inventory.

Configured error tracing preserves sanitized structural diagnostics and an
independent observer policy. Private catch markers cannot be supplied by request
JSON and are removed before response delivery. The new session-list catch covers
only query failure, retaining auth/freshness/commit behavior. Subprocess-isolated
trace/callback counts and authenticated fault tests pass; neither raw stack/message
parity nor all plugin diagnostics are claimed. A forthcoming two-factor profile
has actual-source characterization only at this checkpoint, not implementation
or release certification.


## Direct PaymentLink creation capability (2026-10-14)

The explicit server-owned approved-price profile is separate from default HTTP
routes. An opaque non-cloneable reservation ticket exists only after confirmed
intent commit; inspection cannot mint dispatch authority. Tenant-encrypted intent
and receipt bind operation, actor, UUID, payload and provider scope. RLS, immutable
journal/audit policies and atomic publication are exercised under restricted PG
roles. Real lost-COMMIT-ack tests establish fail-closed reservation and inspectable
completion without an automatic repeat provider POST. Historical receipts are
not current account authority. Trusted cross-crate store completion primitives
are not caller-controlled provider evidence ingestion. Provider-price ownership,
actual charged totals, managed dispatch and operator recovery are outside this
narrow profile and remain explicit work.


## Opt-in fixed two-factor security boundary (2026-10-14)

Factor secrets/recovery/state use the IAM encrypted tenant store, not plaintext
core columns. Password snapshots are rechecked in the authoritative transaction;
challenge login grants no temporary authenticated session. Challenge/recovery/
trust consumption is atomic. Signed browser cookies, origin guards, route rate
limits and committed failure counters bound attempts; current passwords are
required for enrollment/disable/regeneration. Default code-step reuse follows
upstream and does not imply replay-proof TOTP. Existing sessions/social callbacks
are independent. Authorized recovery-code/URI responses must not be logged.
Restricted PostgreSQL, concurrent recovery, RFC vectors, actual client/JWT and
full IAM regression proofs are recorded in the inventory. This fixed password-
login profile does not implement OTP delivery, passkeys or universal MFA. The
immutable5a8 packaged checkpoint predates it; generated opt-inTCP proof is pending.


Generated opt-in TOTP now passes realTCP actual-client20 and HTTP/MCP discovery,
with full generated lifecycle, exact role/tenant cleanup and existing112 frontend
actions preserved. This supersedes the pending generated proof above; it is not
an immutable archive or browser-enforcement claim. Elapsed-clock characterization
confirmed existing rotation expiry, so no unproven session lifetime change was
introduced. Detailed logs and source hashes are in the parity inventory.


## Managed creation publication capability (2026-10-14)

Signed creation-context binds the current remote profile/configuration before
local encrypted reservation. A newly committed non-cloneable dispatch ticket
cannot be recreated through inspection. The separate publication capability
exposes no dispatch/idempotency accessor, admits only explicitly marked managed
intents, and rechecks native actor/scope/approved prices and exact current remote
configuration before local UUID/alias/audit/receipt publication. Historical
completion is not ongoing provider authority. Remote observation and local commit
remain separate transactions; timeout or stale/ambiguous completion stays
inspectable without an automatic provider repeat. Actual HMAC/native PG and
separate real platform journal proofs are distinguished in the inventory.


Backup disableSession success now consumes recovery/challenge authority without
issuing a session or trusted-device cookie. The browser can retain an inert signed
challenge cookie, but subsequent use fails. Authenticated verification retains its
existing token. Atomic memory rollback/single-use, actual-client29 and full IAM118
pass; no new storage/cryptographic primitive or schema was introduced. Restricted
PG and generatedTCP receipts above remain explicitly scoped to earlier branches.


## Verified Plan creation boundary (2026-10-14)

The new explicit profile verifies authenticated Product identity/mode before a
single Plan POST and exact Plan/Product evidence before atomic UUID/alias/audit/
receipt publication. A separate encrypted immutable journal grants dispatch only
after confirmed commit; ambiguity never reconstructs that grant. Restricted PG
and real lost-ack tests pass. Plan administrative bindings remain assertions;
legacy mutable DTOs and Plan CRUD are not secured merely by the new binding kind.
Future Subscription dispatch must resolve independent current Customer/Plan
business associations and verify provider evidence, not copy public UUIDs or
mutable externalId/providerFields. No provider mutation was sent outside fixtures.


## Subsequent delivered OTP and configurable code policies (2026-10-14)

The separate trusted delivery profile now persists encrypted purpose/session-or-
challenge-bound OTP before awaiting a server-owned hook, without database locks.
Unambiguous user/session framing is hashed. Its context has no Debug/Serde or
session token and owns a zeroizing code; delivery destinations never come from
HTTP input. Atomic consumption/issuance, failed-commit reuse, concurrent winner,
lock/counter/expiry and hook-off verification pass restricted PG and actual-client
handler checks. Preparation returns sanitized empty500; asynchronous delivery
failure leaves200 and the stored expiring code. Rust panics and background dispatch
are outside the supported callback contract. Native cancellation after commit
may retain a code and never grants authentication. Default two-factor remains off.

Trusted bounded OTP digits/validity/attempt policy and TOTP six/eight-digit/period
policy preserve defaults. Attempts are current policy; issued OTP expiry/length
remain fixed. TOTP policy applies to existing secrets, preserves the source's
binary64 counter behavior and explicitly excludes negative/epoch-wrap differences.
These profiles do not imply passkeys, universal/social MFA or browser delivery.
Exact source/hash/native/PG/client receipts are in the parity inventory; immutable
d835 packaging predates the later OTP/TOTP option drafts and is scoped separately.


Trusted MFA lifecycle options (2026-10-14): passwordless policy is server-only,
requires authoritative sessions, and is permitted only without an existing
credential password. Mutation transactions recheck absent/present password
snapshots. Skip enrollment atomically enables the factor/user and rotates only
the current session; other sessions/trust remain independent. Disabled TOTP
challenges issue no session/JWT and can still use backup recovery. Restricted
PostgreSQL race/rollback tests and actual-source option ordering pass; see the
lifecycle receipt in RUST-TYPESCRIPT-PARITY-INVENTORY.md.


MFA lifetime policy (2026-10-14): trusted whole-second0..365day options update
new challenge/device rows and cookie Max-Age consistently without extending
existing persisted expiry or changing sessions/OTP code TTL. Actual-source tests
prove inclusive challenge equality and strict future trust expiry. Generic
memory/restricted-PG tests prove one-use and failed trust renewal rollback;
HTTP tests verify proof and credential-renewal cookies. Configuration remains
server-owned, with no schema change or browser enforcement claim.


MFA account lockout (2026-10-14): shared pending-only factor budget is independent
of challenge/OTP attempts. Trusted disabled policy bypasses historical account
locks without rewriting their fields; successful disabled TOTP/OTP makes no factor
UPDATE, while backup-code consumption still requires an atomic write. Enabled
policy checks bounds/counter overflow and representable JS-Date deadline before
mutation. Final restricted PG verifies concurrent increments, rollback, lazy
expiry reset and denied-factor-write behavior. Defaults remain10 failures/900s.


MFA backup generation (2026-10-14): trusted sync hook runs after authoritative
auth/password/factor checks inside mutation transaction; bounded output and
explicit errors produce empty500 and atomic rollback, with no callback details
or cookies exposed. Existing stored-code authority is independent of current
generation settings. Duplicate matches consume all equal codes atomically.
Custom empty/Unicode/weak proofs remain caller policy, as upstream; callbacks
must not reenter their store. Generic encrypted record storage remains unchanged;
field-codec compatibility is a separate open capability. Restricted PG covers
concurrent consumption and session-insert failure after enrollment writes.


OTP field policy (2026-10-14): Plain/Hashed representation is server-owned and
independent of encrypted tenant records. Current-policy mismatch spends normal
proof attempts; restoring policy preserves the remaining old code. Persisted
representation validation does not derive digit width from a hash. Restricted
PG verifies one-use/rollback/lockout and expiry. Async/encrypted field codecs
remain separate; their destructive-consume versus CAS failure semantics must
be implemented explicitly, not inferred from at-rest encryption.
