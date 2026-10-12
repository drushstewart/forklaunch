# Native Better Auth module

This crate implements the configured Better Auth 1.7.6 protocol incrementally.
It currently provides compatible scrypt password hashes and signed session
cookies, asymmetric JWT issuance/public JWKs and sixty-two raw authentication HTTP
routes behind the `http` feature. Configured organization, team, membership,
invitation and dynamic-role APIs are implemented. Provider/OpenAPI, full ORM and
callback parity remain in progress, and it is not selectable as a CLI module preset.

`credentials` uses N=16384, r=16, p=1, a 64-byte derived key, NFKC-normalized
passwords and a random 16-byte salt encoded as lowercase hex text. The salt text
itself is the scrypt input, matching `@better-auth/utils`. Call these CPU-bound
operations using a bounded blocking executor from async HTTP handlers.

`cookies` implements Better Call's SHA-256 HMAC, padded standard base64,
JavaScript URI-component encoding, first-cookie selection and configured
24-hour persistent/transient/secure cookie options. Verification returns a
token only; callers must check persistent session existence, expiry, revocation
and tenant membership. Signing a cookie does not authenticate a user.

`cli/tests/rust_better_auth_crypto.mjs` compares both directions with the actual
installed JavaScript libraries. The existing Better Auth protocol and callback
fixtures cover the configured TypeScript service separately.

`jwt::SigningKey` issues the configured session payload (subject, email, active
organization, issuer/audience, issued time and 15-minute expiry). Public JWKs
contain no private material. The PKCS#8 persistence representation must be
wrapped in tenant-bound encryption by storage. Key restoration preserves IDs
and signatures; the keyring owns database lifecycle and rotation. The JWT fixture
compares native tokens/JWKS with actual configured Better Auth signing and JOSE
verification, including expiry, issuer/audience and signature failures.

`store` provides atomic memory and PostgreSQL backends for the eleven configured
models. PostgreSQL encrypts whole records with tenant/model/record binding,
forces tenant RLS, rejects bypass roles, keeps encrypted unique-field claims
compatible across key rotation, and commits audit events with mutations. The
`fl_better_auth` schema owns its migration ledger independently of other modules.
Use a migration owner to initialize it and grant the runtime role schema usage,
table DML and sequence usage, without SUPERUSER/BYPASSRLS. Request-specific audit
contexts require a nonempty correlation ID. Graph relationships and cascades
belong to the auth service. Model scans currently fail above 4096 records;
indexed queries/pagination remain necessary for larger deployments.

`keyring::KeyRing` initializes keys atomically, selects the newest live key,
restores private keys from storage, rotates keys through trusted operator calls,
and publishes retired keys through the configured 30-day grace period. Both
memory and real PostgreSQL tests cover concurrent initialization, process/service
reconstruction, rotation, exact grace expiry and corrupt-public-key rejection.
The actual-library lifecycle fixture checks stable insertion ties and expired
key replacement as well. The `http` feature integrates token/JWKS HTTP endpoints
with persistent sessions.

`sessions::SessionService` implements native authoritative session creation,
one-hour refresh, creation-based freshness, scoped revocation, sign-out and
transient/expired-session cleanup. Memory and live PostgreSQL lifecycle tests
pass. The actual configured HTTP fixture compares 48 native response bodies and
stored states after session operations; credential endpoints seed identical
records, so this does not claim native sign-up/sign-in. Signed cookie extraction
is exercised at the fixture boundary. The `http` feature integrates production
session cookie headers/clearing and CSRF/Origin guards; the CLI preset remains open. Session queries
currently use the explicitly bounded store scan; indexed lookup remains open.

`accounts::CredentialService` provides atomic native email signup/signin. Scrypt
runs in a shared bounded blocking pool outside tenant write locks; concurrent
signup rechecks email uniqueness inside the write transaction, and signin
rechecks the account after password verification to reject concurrent password
changes. Eighty-one actual-handler comparisons cover response/stored state,
case normalization, callbacks and Unicode password limits; memory and live
PostgreSQL exercise concurrent signup and persistent credential/session records.
Credential DTOs are parsed inputs. The `http` feature applies body/media
validation, CSRF/Origin/callback guards and signed browser cookies. Authenticated password
verification/rotation rechecks authoritative credentials and session state after
blocking work. Rotation optionally revokes all own sessions, including current,
and issues a replacement session whose cookie inherits the existing browser
`dont_remember` flag. Actual handlers and memory/live
PostgreSQL validate wrong/oversized passwords, retained sessions without revocation,
replacement sessions and isolation of other users. User/account updates and
provider linking remain open.

`origins` matches the configured Better Auth trusted-origin protocol: exact HTTP
origins, wildcard hosts, custom-scheme path boundaries, UTF-16 wildcard semantics
and safe root-relative callbacks. The mutation policy checks Cookie header presence,
Origin/Referer fallback, null origins with same-origin Fetch Metadata, login CSRF
and all four callback fields. The actual-library/HTTP fixture compares 2,822 cases,
including encoded separators, traversal, forged origins and browser navigation.
The adapter must apply this policy before mutations and separately validate any
GET callback URLs; dynamic trusted-origin functions and disabled guards are not
part of the configured module.

`http::AuthHttp` exposes `/sign-up/email`, `/sign-in/email`, `/verify-password`,
`/change-password`, `/update-user`, `/list-accounts`, `/unlink-account`, `/ok`, `/get-session`, `/list-sessions`, `/sign-out`,
`/revoke-session`, `/revoke-sessions`, `/revoke-other-sessions`, `/token`, `/jwks`,
`/request-password-reset`, `/reset-password`, `/send-verification-email`, `/change-email` and
`/delete-user`, `/reset-password/:token`, `/delete-user/callback`, `/verify-email`,
`/organization/create`, `/organization/check-slug`, `/organization/list`,
`/organization/get-full-organization`, `/organization/set-active`,
`/organization/get-organization`, `/organization/update`, `/organization/delete`
and `/organization/has-permission`.
It authenticates signed cookies against authoritative tenant-bound storage,
refreshes sessions, emits verified JWT headers and publishes only public keys.
Mutation guards reject forged/missing origins and unsafe callbacks. Revoke/logout
JSON bodies and media errors are validated, and all browser auth cookies are
cleared using separate Set-Cookie headers. Unlike a wildcard exposed-header policy,
`cors_layer()` supports credentialed browsers and explicitly exposes `set-auth-jwt`.
Install it on the **outer** framework router when mounting the raw namespace:

```rust,ignore
let auth = AuthHttp::new(store, public_url, secret, trusted_origins)?;
let cors = auth.cors_layer();
let service = ForkLaunchRouter::new(service_info)
    .nest_internal("/api/auth", auth.router())
    .with_config(RouterConfig { cors: Some(cors), ..RouterConfig::default() });
```

The raw namespace stays outside OpenAPI, SDK and MCP contracts. The fixture
compares 161 actual HTTP body/storage/header/JWT outcomes, including transient
sessions, refresh suppression, media/body errors and scoped revocation. Live TCP
HTTP/MCP tests verify discovery/invocation, cookies, credentialed CORS, signatures,
CSRF and graceful shutdown using memory and encrypted restricted-role PostgreSQL.
HTTPS extraction requires the secure cookie prefix and clears secure attributes.
The credential fixture runs against both the domain backend and production HTTP
adapter. Its 122 HTTP comparisons cover actual signed-cookie issuance, callback
headers, case/Unicode password behavior, opaque additional profile values,
ignored private-field inputs, client metadata and transient session replacement.
Sixty-three additional parser/schema/guard/method cases compare full HTTP errors,
including JSON/form media, validation order, duplicate query keys and unsupported
methods. Live memory and encrypted PostgreSQL exercise credential signup/signin
and rotation through TCP alongside MCP startup and graceful shutdown.

Production rate limiting matches 153 actual HTTP outcomes, including rolling idle
windows, normalized IP/path buckets, special rules, retry headers and pre-parser/
CSRF ordering. The process-global memory store has bounded expiry/insertion
indexes. A live TCP burst admits exactly three of 32 concurrent signin requests
and shuts down cleanly. Defaults enable limits in production; operators may use
`with_rate_limiting` to override this configuration.

Profile updates and linked-account listing/unlinking match 50 actual HTTP
response, cookie-attribute and storage comparisons. Opaque profile fields retain
presence/null values, private inputs remain ignored, token/hash data stays private,
scopes use ECMAScript trimming, and transient cookies retain their browser flag.
Concurrent unlinking preserves at least one account; revoked sessions cannot
update profiles. Live memory/encrypted PostgreSQL verify profile HTTP operations
alongside MCP.

The four configured-disabled email/reset/deletion routes match 128 actual HTTP
response, cookie and storage comparisons. Schema and Origin checks retain their
order, public email operations return configured-disabled errors, and authenticated
email changes/deletion require authoritative sessions. Disabled deletion returns
the actual empty HTTP 404 response. These routes perform no email or deletion
work; callbacks/enable flags are absent from the configured TypeScript module.

Token-based password reset matches 44 actual HTTP/storage comparisons. Body
and query tokens, UTF-16 password limits, newest duplicate selection, duplicate
invalidation, expired/missing-user consumption and exact expiry boundaries match.
Reset creates a missing credential account and retains existing sessions. Hash
work runs outside write locks; atomic consumption permits one concurrent winner.
Live memory/encrypted PostgreSQL exercise reset and subsequent password checks
through HTTP beside MCP. Reset-email delivery remains configured disabled.

GET reset/deletion callbacks match 52 actual HTTP/storage comparisons, including
trusted destinations, encoded raw token segments, query replacement, redirect
content type, combined validation errors and expired-verification cleanup. GET
reset redirects retain valid tokens; the POST consumes them. GET callbacks ignore
mutation Origin checks and use configured authority despite forged Host headers.
Live memory/encrypted PostgreSQL follow redirects with successful reset and MCP
startup/invocation/shutdown. Disabled deletion keeps its coded 404 response.

Remaining user/account/provider/org routes, real TS ORM
conformance, custom callbacks and module generation remain open; these twenty-four
routes do not constitute the complete Better Auth preset.

`credentials::verify_email_token` verifies the configured HS256 email-token
protocol, with 96 actual JOSE/HTTP comparisons. It distinguishes signature/JWT
failures, expiry and the handler claim-schema failures, preserving optional
iat/nbf/exp ordering, critical-header behavior, padding/noncanonical bits, UTF-8
BOMs, duplicate JSON keys and JavaScript numeric overflow. Raw numeric parsing
is local to token validation. This verifier backs the signed email-verification HTTP flow.

Signed email verification matches 52 actual HTTP response/storage/cookie/header
comparisons. Normal verification is idempotent without automatic sign-in. Signed
confirmation, verified-change and legacy-change tokens preserve configured
behavior, authoritative session checks, session reuse/creation and transient
cookies. Error redirects append query text; success redirects preserve relative
locations and Fetch byte-string/whitespace normalization. Malformed claim schemas
retain empty 500 responses. Live memory/restricted-role PostgreSQL verify signup
state changes, forged-token rejection, signed-cookie issuance and subsequent
session/JWT access over HTTP beside MCP. Email sending/change requests remain
configured disabled. Remaining provider/org/account operations and CLI preset
stay open.

Reset credential selection/update also requires accountId=userId, preserving
mismatched legacy rows and creating a proper credential when none matches. The
44-case actual HTTP fixture preserves hash equality groups to detect accidental
updates to legacy rows; memory and restricted PostgreSQL exercise both paths.

The native server-only `CredentialService::set_password` helper matches 36 actual
configured Better Auth API/storage comparisons. It creates only strictly scoped
credentials, fills empty/null/missing passwords and rejects already-set passwords;
authoritative session refresh/expiry and password validation retain actual order.
Memory and restricted PostgreSQL exercise concurrent initial assignment, legacy
row preservation and empty credentials. A deterministic memory-store race revokes
the session after derivation and proves no password is written. No public route is
added for this server-only API; the native raw HTTP count remains twenty-four.

Twenty-nine native raw auth routes include organization creation, slug checks,
listing, full organization graphs and active selection. All 134 actual configured
HTTP/storage/cookie comparisons pass. Creation atomically adds the owner, default
team and keyed team membership; it updates active organization/team unless told
to preserve the current selection. Metadata remains serialized in storage/list
responses and parsed in creation responses, matching the configured adapter.
Selection/full reads enforce membership and persist rejection-driven clearing
without clearing the active team. Password/session HTTP regression remains green.
Live memory and restricted PostgreSQL verify organization HTTP, session/JWT
organization claims, cookies and MCP discovery/invocation/shutdown. Organization
updates/deletion, remaining member/team/invitation/dynamic-role APIs, provider
flows, real ORM conformance, custom callbacks and CLI preset remain open.

Thirty-three native raw auth routes include metadata-only organization reads,
updates, deletion and permission checks. All 212 actual HTTP/storage/cookie
comparisons pass. Dynamic stored grants merge into each configured static role
and remain scoped to one organization; composite roles cannot pool permissions
across separate roles to satisfy a single request. Platform-only default roles
retain mutation denials. Schema/alias ordering, malformed permission JSON/schema
(including numeric overflow), field whitelists and endpoint-specific error bodies
and statuses match actual handlers. Deletion removes organization/member/invite
records and clears only the actor's matching active organization; configured
team/role rows and other sessions' selections remain, matching the actual adapter.
Memory/restricted PostgreSQL cover granted mutations, atomic concurrent deletion
and HTTP storage persistence beside JWT/MCP startup/lifecycle. Remaining member,
team, invitation, dynamic-role management, provider, real ORM, callback and CLI
preset gates remain open.

Thirty-eight native raw auth routes include active member/role reads, role updates,
member removal and organization departure. All 146 actual HTTP/storage/cookie
comparisons pass, including creator bypass boundaries, dynamic grants, duplicate
roles, Unicode trimming, error ordering and email-versus-ID user projections.
Departure/removal delete team memberships and release stored counters atomically;
only the actor's matching active organization clears, retaining active team.
Memory and restricted PostgreSQL exercise competing owners leaving, ensuring one
owner survives, and live HTTP member operations alongside JWT/MCP lifecycle.
Member listing, server-only addition, team/invitation/dynamic-role management,
provider flows, real ORM conformance, callbacks and CLI preset remain open.

The trusted server-only `OrganizationService::add_member` helper matches 40 actual
API/storage outcomes. Typed inputs preserve role arrays by joining them verbatim;
the explicit organization requires no session, while an optional session provides
active-scope fallback and refresh/expiry effects. User/team/organization checks,
duplicate ordering, the configured 100-member limit, keyed team membership reuse,
undercount reconciliation (including its timestamp) and retained overcounts match
the configured adapter. Memory/restricted PostgreSQL verify concurrent duplicate
and last-slot admission; live HTTP rejects public add-member access. Raw auth
route count remains 38. Member listing and the remaining team/invitation/dynamic
role, provider, real ORM, callback, JWT-helper and CLI preset gates remain open.

Thirty-nine native raw auth routes include scoped member listing. All 340 actual
HTTP/storage/cookie comparisons pass across schema ordering, active/ID/slug scope,
filtered totals, public-user projection, missing-user failures, stable date and
Unicode sorting, all filter operators and ECMAScript Number/slice pagination.
Unicode sorting uses ICU's en-US collation, matching the fixture's Node locale;
real ORM collation and query behavior remain a separate open conformance gate.
Memory/restricted PostgreSQL cover filtering, Unicode pages and orphan failures;
live HTTP listing runs beside session/JWT and MCP lifecycle. Team/invitation/
dynamic-role management, provider flows, remaining server JWT helpers, real ORM,
callbacks and generated preset remain open.

Forty-three native raw auth routes include organization/user team lists, team
member reads and active-team selection. All 318 actual HTTP/storage/cookie
comparisons pass, including self scope across organizations, permission-gated
other-user reads, malformed grants, distinct organization/team membership guards,
internal counter/key projections, transient cookies and selection timestamps.
Selection preserves the active organization; clearing an existing selection emits
a cookie once, while successful selection emits one even when unchanged. Memory
and restricted PostgreSQL verify team/session persistence and live HTTP alongside
JWT/MCP lifecycle. Team mutations, invitations, dynamic-role management, provider
flows, remaining JWT helpers, real ORM/callback and generated preset gates remain
open.

Forty-six native raw auth routes include team creation, name updates and removal.
All 110 actual HTTP/storage/cookie comparisons pass, including mutation-specific
authorization and unauthenticated error ordering, schema/field stripping, stored
grant failures, active-team protection and last-team safeguards. Creation stores
zero members without enrolling the actor; updates retain organization/counters.
Removal atomically deletes memberships and removes the team's comma-separated
references from the first 100 pending invitations that have not expired. Expired/
accepted invitations and other sessions' active-team selections remain unchanged.
Memory/restricted PostgreSQL verify concurrent removals preserve the last team,
invitation cleanup and live HTTP mutations beside JWT/MCP lifecycle. Adding and
removing team members, invitations, dynamic roles, providers, server JWT helpers,
real ORM/callback and generated preset gates remain open.

Forty-eight native raw auth routes include team-member addition/removal. All 164
actual HTTP/storage/cookie comparisons pass, covering scoped member permissions,
required/coerced user IDs (including ECMAScript number formatting and failed
object coercion), keyed duplicate reuse, legacy duplicate removal, counter
reconciliation/release and retained active-team references. Organization addition
shares the same membership helper; its 40 actual API/storage comparisons remain
green. Memory/restricted PostgreSQL verify concurrent additions reuse one row and
seat, concurrent removal has one winner and sessions retain active-team values.
Live HTTP runs the new routes beside JWT/MCP lifecycle. Invitation/dynamic-role
management, providers, server JWT helpers, real ORM/callback, raw JSON numeric
edge cases and generated preset gates remain open.

Raw auth JSON and metadata pass 114 actual HTTP/storage/cookie comparisons.
Numbers follow IEEE double coercion and ECMAScript stringification, including
overflow, negative zero and exponent thresholds. Metadata preserves duplicate
key positions and sorts array-index keys before insertion-ordered keys. The HTTP
record boundary strips top-level `__proto__`; strict update parsing rejects
reserved keys after persistence and revives ISO date strings for its response.
Literal serde marker keys remain ordinary metadata through a bounded raw parser.
Workspace tests pass with unified JSON features (233 framework, 101 blueprint),
as do explicit restricted PostgreSQL auth and existing JOSE/password/session/
organization/team regressions. Shared generic deserializer marker keys, unpaired
surrogates/deep nesting and real ORM metadata behavior remain open gates.

Fifty-one raw auth routes include invitation lookup and organization/user lists.
All 258 actual HTTP/storage/cookie comparisons pass: configured verification,
recipient visibility, exact expiry boundaries, inviter joins and missing records,
100-row limit-before-status filtering, client email-query rejection and method
fallbacks. Native memory/restricted PostgreSQL and live HTTP cover reads beside
JWT/MCP. Invitation lifecycle mutations and remaining dynamic-role/provider/
helper/ORM/callback/preset gates stay open.

Fifty-three raw routes include invitation rejection/cancellation, with 318 actual
HTTP/storage/cookie comparisons passing. Verified recipients may reject expired
pending invitations; scoped invitation:cancel can replace any status repeatedly.
Neither operation changes membership or session selections. Memory/restricted
PostgreSQL cover concurrent cancellation; live HTTP runs both routes beside JWT/
MCP. Invitation creation/acceptance and remaining full-preset gates stay open.

Fifty-four raw routes include invitation creation/resend. All 196 actual HTTP/
storage/cookie comparisons pass, including role and creator rules, scoped grants,
member/invitation duplicates, team validation and bounded invitation limits.
Resend updates expiry while retaining stored role/teams, before team/limit checks.
Memory/restricted PostgreSQL verify concurrent last-slot admission and retained
team/member state; live HTTP runs creation/resend beside JWT/MCP. Acceptance and
remaining full-preset gates stay open.

Fifty-five raw routes include acceptance, completing configured invitation
lifecycle. All 186 actual HTTP/storage/cookie comparisons pass: recipient and
verification ordering, exact expiry, member limits, duplicate memberships,
verbatim roles, keyed team/counter enrollment and active scope. Only single-team
acceptance emits cookies and selects a team; absent/multiple teams retain the
prior team. Memory/restricted PostgreSQL verify competing acceptance, last-slot
admission and complete rollback after missing later teams. Live HTTP covers
no-team/single-team transient cookies beside JWT/MCP. Remaining full-preset
gates include dynamic roles, providers, JWT helpers and real ORM/callbacks.

Fifty-seven raw routes include dynamic role listing/lookup. All 272 actual HTTP/
storage/cookie comparisons pass: ac:read grants, raw membership, nullish scope,
case-sensitive name/ID selectors and union validation, first-100 listing, later
lookup and malformed later JSON. Permission responses contain parsed JSON while
storage strings remain intact, including literal serde keys. Memory/restricted
PostgreSQL and live HTTP/JWT/MCP cover both routes; role mutations remain open.

Fifty-eight raw routes include dynamic role creation. All 212 actual HTTP/storage/
cookie comparisons pass, including nullish scope, lowercase names without
trimming, the configured predefined-name guard, permission/resource ordering,
duplicate names beyond 100 rows and ignored additional fields. Public grants
permit only the configured platform resource, and each requested action requires
an actor grant. Missing actions retain order and duplicates in missingPermissions.
Memory/restricted PostgreSQL verify atomic competing duplicate creation and stored
permission JSON; live HTTP runs creation beside JWT/MCP. Role update/deletion and
remaining provider/helper/ORM/callback/full-preset gates stay open.

Sixty raw routes include all five configured dynamic-role operations. Update and
deletion pass 332 and 324 actual HTTP/storage/cookie comparisons. Updates enforce
per-action grants before renaming, reject same-name renames, update duplicate
matches and retain member role strings. Stored updatedAt advances while the
response retains the prior projection. Deletion checks the first 100 matching
members after substring filtering, then exact trimmed roles, and removes matching
stored rows. Name-based predefined protection differs from ID selection. Memory/
restricted PostgreSQL verify atomic deletion, blocked assigned roles and duplicate
updates/deletes; live HTTP runs both routes beside JWT/MCP. Providers, server JWT
helpers, real ORM/callback, shared parser and full-preset generation remain open.

The configured server-only verifyJWT helper now supports persisted Ed25519 keys
with EdDSA/Ed25519 token aliases and issuer overrides. All 159 actual API and 144
key-store comparisons pass: optional claims, numeric/expiry boundaries, audience/
subject truthiness, critical headers, compact encoding, JWK metadata/import rules,
retired keys and first-100 key selection. Invalid tokens/storage/key metadata
return null without minting keys or reading private material. Memory/restricted
PostgreSQL verify restart/retirement and corrupt-private-key independence; live
HTTP confirms signing/verifying helper paths remain absent beside JWT/MCP. Three
of four configured server helpers now have native implementations. Signing,
additional-algorithm provisioning/interop, providers, real ORM/callback, shared
parser and full-preset generation remain open.

All four configured server-only helper entrypoints now have native implementations.
The configured EdDSA signJWT profile passes 129 actual API and 134 key-state
comparisons, with independently verified signatures. Cover schema/default-duration
ordering, key persistence after claim failure, exact claim setters, issuer/audience
precedence, fractional/negative/overflow durations and numeric key rotation. Memory/
restricted PostgreSQL exercise signing and verification beside key lifecycle; live
HTTP/JWT/MCP retain absent public helper paths. Additional signing-algorithm/key
provisioning, typed date/function options, custom callbacks and real ORM/storage
interop remain open; unsupported fresh key profiles fail explicitly. Sixty raw
HTTP routes remain implemented, with provider/OpenAPI/full-preset work outstanding.

Persistent signing profiles support EdDSA/Ed25519, ES256/P-256, ES384/P-384,
ES512/P-521, RS256/384/512 and PS256/384/512. The server-only signing helper
accepts `overrideOptions.jwks.keyPairConfig.alg`; RSA `modulusLength` currently
supports 2048–4096 bits (default 2048). Native PKCS8 private material stays in the
encrypted store. RSA signing uses ring, and prime generation runs on the blocking
executor. Default session signing prefers a live EdDSA key before falling back
to another profile. Public verification can use retired keys and never needs
private material. Actual comparisons cover 189 signing calls/216 key states and
393 verification calls/309 key-store operations with independent JOSE signature
verification. Larger RSA keys, typed date/function options, custom signing/key
adapters, provider/ORM/shared-parser and full-preset parity remain open.

Database OAuth state primitives now pass 66 actual state/cookie and 66 storage
comparisons, using real Better Auth endpoint context, Better Call and the memory
adapter. Native random state/PKCE, signed cookies, expiry/cleanup, malformed and
legacy data, link coercion and atomic consumption pass memory/restricted
PostgreSQL checks. Strict all-feature clippy, rebuilt CLI and 715 embedded sources
pass. Google HTTP authorization/callback and token/account lifecycle remain open;
this foundation adds no raw routes.

Configured Google authorization URL construction now passes 37 actual provider
comparisons: S256 PKCE, scopes, callback URI, login hints, reserved parameters,
overwrite behavior, Unicode/query encoding and JavaScript numeric-key ordering.
Strict all-feature clippy, rebuilt CLI and 717 embedded sources pass; CI runs the
fixture. HTTP social-login/ID-token user-account-session integration, callback
code exchange and token lifecycle remain open. No raw route is added yet.

Google ID-token verification primitives now pass 147 comparisons against the
actual provider, including RS256 signature/JWK import behavior, issuer/audience,
nonce, numeric claim boundaries and duplicate key selection. The verifier takes
trusted supplied certificate keys; HTTP certificate fetching and persistent
user/account/session integration remain open. Strict all-feature/all-target
clippy, rebuilt CLI and all 719 embedded Rust sources pass; CI runs the fixture.

Configured `/sign-in/social` now supports Google authorization URL/state issuance
and direct verified ID-token sign-in with persistent user/account/session
resolution. All 165 actual HTTP/storage comparisons pass, including nested body
validation, missing provider/claims, forged/expired tokens, duplicate-key order,
profile preservation, token replacement and safe verified-email linking. Google
certificate transport has fixed trusted endpoints, no redirects, timeouts and a
one-MiB response bound. The centralized direct sign-in verifier imports all
selected keys and uses the first, matching the configured TypeScript path.
Concurrent signup, orphan rejection, encrypted tokens and tenant isolation pass
memory/restricted PostgreSQL checks. Authorization-code callbacks and the
remaining account/token HTTP lifecycle remain open.

Install Google with `AuthHttp::with_google(GoogleClient::new(client_id,
client_secret)?)`; credentials come from trusted server configuration. Google is
disabled by default. Direct sign-in persists `accessToken` and `idToken`; its
request `refreshToken` and `expiresAt` are intentionally ignored by the configured
TypeScript handler. Certificate endpoint overrides are trusted operator settings
and require HTTPS or loopback HTTP.

Google authorization-code callbacks now pass 226 actual HTTP/storage/wire
comparisons, including S256 verifier binding, trusted fixed token endpoints,
refused redirects, one-time state/cookie correlation, exact ten-minute expiry,
provider errors, POST normalization, success/new-user/error redirects and stored
access/refresh/ID tokens. Token-response normalization covers numeric coercion,
invalid dates and scope forms. Explicit callback linking preserves account
ownership, verified-email policy and merged scopes; malformed provider tokens
match the upstream empty-500/cookie-header boundary. Native memory tests cover
identity races, link conflict rejection and token/scope preservation. The same
checks pass encrypted restricted PostgreSQL with tenant isolation. Strict default and
all-feature/all-target clippy, all 21 native auth tests, rebuilt CLI and 725
embedded sources pass. The `/link-social` initiation route, access/refresh-token
and account-info routes, OpenAPI/error/update-session routes, full ORM/application
hooks and CLI preset remain open. The implemented raw-route count is 62 of 70.

A fresh relocated archive installation (`VwxVQI`, snapshot `f1a966960`) passes
all portable services and four worker backends, CLI/native SDK generation and
all 21 portable auth tests. The complete generated HTTP/MCP startup matrix
passes shared/separate listeners, enabled/disabled/custom endpoints, explicit
and ephemeral ports, TypeScript +2000 fallback, atomic startup rejection and
graceful shutdown. MCP initialization/discovery/health invocation/Origin
rejection and all five actual-TypeScript-signed capability callbacks pass. This
snapshot contains 62 raw auth routes and Google code callbacks; later linking
initiation work is validated separately. The checkout CLI was restored after
the archive install.
