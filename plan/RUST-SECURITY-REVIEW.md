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
