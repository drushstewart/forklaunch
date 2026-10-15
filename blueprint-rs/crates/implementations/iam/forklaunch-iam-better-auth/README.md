# Native Better Auth module

This crate implements the configured Better Auth 1.7.6 protocol: compatible
scrypt hashes and signed cookies, asymmetric JWT/JWKS, all 70 raw authentication
HTTP routes, organizations, teams, memberships, invitations and dynamic roles.
The `application-http` feature adds the seven configured HTTP/MCP application
contracts. CLI generation accepts `--module iam-better-auth` for native Rust.
The application feature includes the two custom internal popup helpers.
Scalar docs include both application contracts and the complete BetterAuth API.
Fixed auth namespaces, including root mounts, are configurable. Dynamic/encoded namespaces,
complete ORM/hooks and broader nondefault options remain parity work. The sections below retain
validation history; the latest entries describe current coverage.

Application authorization uses the exact verified user/organization/session
scope and current stored membership. Session JWTs include the persisted session
ID. Scoped HMAC routes include all three identifiers in their signed paths;
legacy user-only lookups return no grants. Builtin roles expose their configured
platform permissions, while custom-role permission strings remain opaque.
Session primary keys and user foreign keys use SQL UUID equality; organization
and member text fields retain case-sensitive comparisons. Retired low-level
JWT helpers without a session ID cannot confer scoped application privileges.

Raw authentication HTTP 5xx responses emit a tracing error with the matched
route template, status and allowlisted structural database metadata. Applications
can install `AuthHttp::with_api_error_observer` for a trusted synchronous callback.
It receives no concrete token paths, query strings, cookies, freeform error
messages or stacks. Callback panics are caught; the application's global panic
hook still runs. Callbacks should return promptly. Successful responses and 4xx
failures do not invoke the observer. The historical callback mode covers all raw 5xx
responses. Opt in with `with_api_error_observer_mode(
AuthApiErrorObserverMode::UnexpectedOnly)` to exclude explicitly handled API
failures, matching the configured Better Auth callback dispatch. Provenance is a
private typed marker retained through service errors and removed from response
extensions; error codes, JSON details and application diagnostic metadata cannot
impersonate it. Session lookup and the two caught revocation branches preserve
the actual handler's handled-error boundary; malformed permission JSON remains
unexpected while a parsed-invalid permission schema is handled.

The generated preset selects unexpected-only mode for callbacks an application
installs. It does not install a callback itself. Built-in sanitized tracing still
records every raw5xx in either mode, so this does not claim identical default
TypeScript logger event counts or freeform log payloads. Responses, cookies and
headers do not change when selecting callback mode.

Generate a native application with this preset:

```sh
forklaunch init application my-app --language rust --modules iam-better-auth
```

Use the generated service README and `.env.example` for migrations, restricted
PostgreSQL grants, tenant/encryption configuration and HTTP/MCP settings.

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
belong to the auth service. Whole-model scans fail above 4096 records; callers
can traverse insertion-ordered 256-record pages within a transaction. Exact
user/organization/session unique-field lookups use indexes. `find_page` also
supports nonunique relationship fields with case-sensitive string matching:
user IDs on sessions/accounts/members, account/provider IDs, organization IDs,
team IDs/membership keys, invitation emails, role names and verification identifiers. PostgreSQL
stores keyed blind indexes, including retained encryption keys; queries deduplicate
rotated-key matches and recheck decrypted fields before returning rows. Memory
maintains equivalent ordered indexes inside the transaction snapshot.

Unfiltered, unsorted `/organization/list-members` requests with ordinary integer
pagination use `find_window`: an indexed total count plus only the requested
insertion-ordered member window, followed by one bounded user batch lookup.
The default page is 100; integer limits through 4096 use this path. The same
tenant transaction lock keeps count and page consistent. Negative, fractional,
nonfinite, larger, filtered or sorted pagination retains the existing adapter
compatibility path. Counts trust maintained relationship indexes and do not
decrypt off-page ciphertext; corrupt off-page records therefore do not fail a
normal page, while returned records still authenticate and missing page users
still fail. Existing mutation audits and tenant/organization checks are unchanged.


Migration `0003_relation_indexes.sql` adds the forced-RLS `lookup_claims` table;
grant the restricted runtime role its SELECT/INSERT/UPDATE/DELETE privileges.
Migration `0004_unique_claim_record_index.sql` indexes unique claims by record
owner, making replacement/backfill and foreign-key deletion use a selective
index instead of scanning a tenant's claims.
Opening the store rebuilds legacy relationship indexes in bounded pages under
the tenant write lock; this startup cost scales with the existing dataset.
Deploy index-aware writers together, since old binaries do not maintain these
claims. `reindex_relationships` also supports trusted key-rotation/backfill work.
Signup/signin, session collection operations, linked-account listing/removal and
organization membership checks/listing now use scoped indexes. Member listing,
mutations, organization deletion/full graphs, dynamic roles, invitation membership
guards and application role/permission surfacing also use scoped relationship
queries. Stored legacy non-string organization values retain JSON equality through
a paged fallback; valid string relationships use indexes. Team reads/mutations,
membership-key/pair lookup and invitation listing/creation also use scoped indexes.
Provider identity, password/reset and compliance account queries use scoped
indexes. OAuth state and reset callback expiration cleanup traverse keyset pages,
preserving global cleanup and one-time token semantics. All production auth
service traversals now use scoped indexes or keyset pages. Retention scans retain
only the oldest requested batch, preserving insertion-order ties; dry runs count
without collecting decrypted rows. Response arrays still require memory
proportional to their returned matches, and global expiration/retention scans
remain linear in tenant records.

`keyring::KeyRing` initializes keys atomically, selects the newest live key within
the configured TypeScript adapter's first 100 inserted records,
restores private keys from storage, rotates keys through trusted operator calls,
and publishes retired keys through the configured 30-day grace period within
that same 100-record window. Administrative rotation retires all live keys using
bounded pages. Operators must prune old records before the selection window fills:
newly appended keys outside the first 100 are not published or selected, matching
the configured upstream behavior. Both
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
session cookie headers/clearing and CSRF/Origin guards, including the generated
CLI preset. Token reads, sign-out and single-session revocation use exact indexed
lookups. Session lists and bulk revocation traverse bounded storage pages under
one transaction, preserving insertion order, foreign-user isolation and expired
session semantics beyond 4096 rows. Collection pages use the user-ID index;
listing still materializes the matching response array.

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

Google `/link-social` now supports direct verified ID-token linking and browser
authorization with server-owned user/email state. Existing own accounts update
tokens; new links enforce verified provider email and matching local email;
another owner's provider identity returns a conflict. The combined social/link
HTTP fixture passes 289 actual response/storage comparisons, including nested
schemas and ignored direct-link fields. The browser callback fixture passes 236
actual response/storage/wire comparisons through the real linking entrypoint.
Direct-link ownership/policy/token checks pass memory and encrypted restricted
PostgreSQL. Configured `/update-session` matches the TypeScript plugin field
policy: protected organization/team fields cannot be set, and there are no
configured editable session fields. The implemented route count is 64/70.
Access-token refresh, account-info, OpenAPI/error routes, full ORM/application
hooks and CLI preset remain open.

Google `/get-access-token`, `/refresh-token` and `/account-info` now pass 157
actual response/storage/wire comparisons. Account selection uses local owned
account IDs; automatic refresh observes the five-second boundary and preserves
stored scopes and omitted provider fields. Live provider tests verify token
rotation and reject an account unlinked during an in-flight refresh. All 22
native tests, encrypted restricted PostgreSQL, strict all-feature clippy, the
161 HTTP and 289 social regressions, rebuilt CLI and 728 packaged assets pass.
The implemented raw-route count is 67/70. Error/OpenAPI/reference routes,
full ORM/application hooks and the native CLI auth preset remain open.

Configured `/error`, `/open-api/generate-schema` and disabled `/reference` now
complete all 70 raw routes. The default error HTML is adapted under the upstream
MIT license and matches full rendered responses, URLSearchParams first-value
selection, preserved entities and production redirects. The OpenAPI response
uses the fixed preset's declarative contract with the trusted server URL; it is
compared in full with the actual configured TypeScript generator. Custom plugin
schemas/options are outside this fixed preset. All 27 development and 27
production public-response comparisons, 22 native tests and strict all-feature
clippy pass. Full ORM/application hooks, application surfacing/compliance,
retention and native CLI preset integration remain open.

Fresh archive installation `IqcM1T` (snapshot `4f0540ba6`, all 70 raw routes)
passes 22 portable auth tests, all portable services/four worker backends and
CLI/native SDK generation. The complete generated HTTP/MCP startup matrix,
initialization/discovery/health invocation/Origin rejection, shared/separate
listeners, enabled/disabled/custom endpoints, explicit/ephemeral ports,
TypeScript +2000 fallback, atomic startup rejection and graceful shutdown pass.
All five actual-TypeScript-signed managed capability callbacks pass. The
checkout CLI was restored after the archive installation. Later application
surfacing changes are validated separately from this snapshot.

Native application `SurfacingService` now matches 37 actual TypeScript result
comparisons. It selects the newest unexpired organization-bearing session,
retains exact member role strings, and returns stored permission strings
without parsing, splitting or unioning them. Expiry boundaries, duplicate
ordering, other users/organizations and missing/falsy scopes are covered. The
shared memory/restricted PostgreSQL check passes with ciphertext inspection
and tenant isolation; strict clippy and 734 packaged assets pass. HTTP/MCP
application-route wiring, compliance/retention and CLI preset remain open.

Real MikroORM 7.2.3 / PostgreSQL conformance exposed a TypeScript adapter
integration defect: compliant password/private-JWK objects reached Better Auth
without explicit plaintext access, causing sign-in and session-JWT 500s. The
configured adapter now reveals classified results only inside Better Auth's
trusted request context, including transaction adapters, preserving audit reads
and leaving ORM entity fields wrapped. All 24 real-ORM checks pass: encrypted
columns, audited access, missing/wrong tenant rejection, rollback, restart and
native consumption of actual ORM passwords/sessions/accounts with JWT checks.
The adapter utility passes a strict isolated type check against the installed
adapter signature. Existing 161 HTTP, 157 token, 27 public and 37 surfacing
comparisons pass. CI now installs the pinned real ORM/adapter and supplies shared
fixture environment settings. This is initial real-ORM conformance; complete
storage/hook coverage, compliance/retention and CLI preset remain open.

Native Better Auth compliance now preserves classified export fields as opaque
objects with explicit audited plaintext access. Fourteen comparisons against
the real encrypted ORM cover Account/Jwks selection, other subjects, erasure,
remaining user/session records and partial per-entity read failures. Native
memory/restricted PostgreSQL also verify atomic rollback on write failure,
report labels, tenant isolation and ciphertext protection. Erasure rechecks
ownership before deleting a previously selected account.

Seven-calendar-year Account retention passes 41 actual TypeScript comparisons:
UTC, Los Angeles, Lord Howe and Apia cutoff behavior, leap days, repeated hours,
half-hour gaps, skipped days, exact age boundaries, dry runs, entity filters,
batches, repeat runs, preserved metadata and persisted anonymization timestamps.
The implementation validates timezone round-trips and selects the earliest
valid UTC instant. All 26 native auth tests, restricted encrypted PostgreSQL,
strict all-feature clippy, rebuilt CLI and 739 embedded assets pass. Application
HTTP/MCP wiring, custom callback helpers, full ORM/hook coverage and CLI preset
integration remain open.

The optional `application-http` feature now combines the five configured
application contracts with the raw `/api/auth` namespace. Discovery is public;
role/permission surfacing verifies request HMAC; compliance export/erasure
verifies JWKS JWTs and resolves the `system` role from current trusted storage.
Role claims in tokens do not bypass revocation. All five actual TS controller
contracts and nine response schemas match, including string IDs and opaque
permission strings. A live TCP test verifies HTTP and MCP initialization,
discovery, signed tools, missing/invalid credentials, role revocation, classified
export protection, erasure/replay, raw-route separation and graceful shutdown.
All 27 native tests, strict all-feature clippy and 742 packaged assets pass.
The native CLI preset, custom callback helpers and remaining full ORM/hooks
coverage are still open; this does not declare full parity.

Native CLI generation now accepts `iam-better-auth` in Rust/mixed applications
and module initialization. It retains the default service name `iam`, rejects
conflicting IAM presets before writing files, embeds the encrypted auth store,
and emits operator configuration, migrations, application contracts and health.
The relocated generated fixture passes sync/reinitialization preservation,
TypeScript/Rust SDK compilation, real restricted PostgreSQL startup, tenant
session isolation, independent JWT verification, HMAC MCP tools, shared/separate
and disabled MCP listeners, restart persistence, graceful shutdown, rename,
contract/SDK refresh and deletion. All 1,016 CLI tests pass. E2E CI includes the
fixture and archive installation includes the new preset. Custom callback
helpers, additional docs sources and complete ORM/hooks remain open.

Fresh archive installation `XXfFfM` (snapshot `f288c81c4`) passes with the
new native Better Auth preset: portable service and SDK compilation, all 27
packaged auth tests, every other portable service/four worker backends,
CLI/native SDK generation and the complete generated HTTP/MCP startup matrix.
All five actual-TypeScript-signed managed capability callbacks pass. The checkout
CLI was restored after archive installation. Callback-helper changes after this
snapshot have separate validation.

Both configured internal popup helpers are native. `/api/auth/test-callback`
validates trusted destinations, exchanges through the existing auth handlers,
clears only the configured session cookie and replaces the destination fragment
with JWT/session/email values. `/api/auth/oauth-redirect` emits matching
script-safe OAuth/SSO HTML, including duplicate-query coercion. Neither becomes
an MCP tool. Ninety-two actual Express 5.2.1 / TypeScript comparisons cover HTTP
and HTTPS, invalid origins/userinfo/schemes, anonymous/signed/forged/expired
cookies, corrupted signing keys, response media/Accept negotiation, HEAD, cookie
headers and HTML escaping. Independently generated JWTs verify against their
own public keys and match claims; signatures are not compared byte-for-byte.

The relocated generated service also passes these helpers against a fresh
PostgreSQL container, plus shared/separate/disabled HTTP/MCP listeners, tenant
isolation, SDKs, restart, graceful shutdown and rename/delete. All 27 native
tests, strict default/HTTP/all-feature clippy, 40 original callback checks,
161 raw HTTP regressions, five application contracts/nine response schemas and
749 packaged sources pass. `application-http` no longer enables unused SQL or
field-encryption features unless PostgreSQL is selected. Additional docs
sources, nondefault base paths, complete ORM/hooks/client integration and
broader custom options remain open.

### Additional Scalar documentation sources

`RouterConfig.docs_sources` accepts typed URL or inline JSON sources with optional
slug/default selection. Scalar receives the generated application API first,
then the additional documents. BetterAuth applications now include the complete
raw auth schema without adding its endpoints to OpenAPI application contracts,
SDK operations or MCP tools. The TS preset preserves the schema's existing
server URL and relative paths, fixing its duplicated `/api/auth` request paths.
Documentation titles and both Scalar/Swagger inline configuration are escaped.

Twelve comparisons execute the actual pinned Scalar 0.10.25 renderer, the actual
TS registration factory and Better Auth schema generator, then evaluate served
native HTML in a JS VM. They cover remote/inline/string documents, multiple
sources, slugs/defaults and script terminator/Unicode escaping. Live native TCP
and generated fresh PostgreSQL services verify both docs sources and unchanged
MCP discovery. All 27 auth tests, 38 framework tests, strict all-feature/all-target
clippy for both crates, generated HTTP/MCP/SDK/restart/rename/delete checks and
751 packaged sources pass. Nondefault base paths, complete ORM/hooks/client
integration and broader custom options remain open.

### Configurable non-root authentication namespaces

`AuthHttp::with_base_path` and generated `BETTER_AUTH_BASE_PATH` configure a fixed
non-root namespace; a path already present in `BETTER_AUTH_URL` takes precedence.
Raw routes, both popup helpers, Google/reset callbacks, docs and the generated
default JWKS URL use that resolution. JWT issuer/audience use the canonical
origin, matching Better Auth's normalized options. Unsafe namespaces and MCP
collisions fail before service startup. The actual TS server helpers now use
resolved auth URLs, and its configured Google callback no longer concatenates
an ignored base path or omits a slash for relative base-path settings.

All 343 actual Express/TS/schema/provider comparisons pass across seven
HTTP/HTTPS/default/custom/trailing-slash/URL-override configurations. The full
application HTTP/MCP authentication/compliance test runs under three namespaces.
A fresh generated PostgreSQL service passes custom-path separate MCP, URL-path
override on persisted-session restart, disabled MCP, default self-JWKS without
an override, SDKs and rename/delete. All 28 native tests, 1,016 CLI tests, strict
clippy, 161 raw HTTP regressions, 40 existing callbacks, 12 Scalar comparisons
and 751 packaged sources pass. Root/dynamic/encoded namespaces, full ORM/hooks/
client integration and broader custom options remain open.

### Root-mounted authentication and reserved namespaces

`BETTER_AUTH_BASE_PATH=/` (or empty) mounts raw authentication at root while
keeping application contracts, docs, SDKs and MCP reachable. The framework's
`merge_internal` API reserves declared fixed namespaces; collision validation
and parent nesting apply to each reservation. The actual TS preset now limits
its root catch-all to known auth namespaces, avoiding interception of application
routes. Root reset errors no longer introduce an extra slash.

All 450 actual Express/TS comparisons pass across nine configurations, including
630 raw-route dispatch probes and root signed sessions/Google/reset/popup flows.
The full native application HTTP/MCP test runs under five configurations. A fresh
generated PostgreSQL service passes root HTTP/MCP and default self-JWKS without
an override, as well as custom/separate and URL-override/disabled listeners,
persisted sessions, SDKs and rename/delete. All 28 native/40 framework tests,
strict all-feature/all-target clippy for both crates and 751 packaged sources
pass. Dynamic/encoded namespaces and broader ORM/hooks/client/options work
remain separate gates.

### Archive-installed root/docs snapshot

Fresh Cargo archive installation `EklWnG` validates committed snapshot `f20252656`.
All portable services and four worker backends compile, both SDK targets pass,
and all 28 packaged auth tests pass. The complete generated HTTP/MCP matrix
covers enabled/disabled/custom endpoints, shared/separate listeners,
explicit/ephemeral ports, TypeScript's +2000 fallback, atomic startup rejection
and graceful shutdown. All five actual-TypeScript-signed native capability
callbacks pass forgery/staleness/replay/redelivery checks. The checkout CLI was
rebuilt after the archive run. This confirms source portability and service
startup; broader parity and published registry installation remain open.

### Actual shared and generated frontend client interoperability

The shared `clientIamBetterAuthSdkClient` and generated `iamSdkClient` factories
now accept `authBasePath` (default `/api/auth`, `/` for root); a path already in
`host` takes precedence. Both initial templates and subsequent AST injection
carry the option. Eight live scenarios execute both actual TS factories and
Better Auth 1.7.6 over native PostgreSQL HTTP: default/custom/root/URL-override
namespaces, 80 requests total. Signup additional fields, sessions, profile
updates, independently verified JWT/JWKS, signout and signin pass. The fixture
owns Node cookie persistence and public-origin forwarding; universal SDK core
construction remains a separate boundary and does not establish unified mixed
SDK parity.

The final run also exercises automatic pinned-client dependency setup, actual
CLI TypeScript client generation, restricted PostgreSQL, shared/separate/disabled
MCP, self-JWKS, SDKs, persistence, shutdown and rename/delete. All 1,016 CLI tests
and 751 packaged source checks pass. E2E CI installs the pinned client and reacts
to changes in both the auth module and frontend factory. Broader custom client
plugins/hooks, unified mixed SDK behavior and indexed storage remain open.

## Managed-app relay

Add relay to an existing generated Rust IAM service with
`forklaunch init module --language rust --module relay --path <application>`.
This also works in mixed applications. An optional positional service name
selects a renamed Better Auth service; the default is `iam`. The command creates
`src/relay_hooks.rs` and `RELAY.md` and adds relay contracts and startup wiring.
Sync and module reinitialization preserve the application-owned hooks. Duplicate
installation and ambiguous customized startup wiring fail explicitly.

The hook receives provider tokens after the nonce is committed. Applications
must securely persist those tokens and return an existing owner's user ID if
login is intended. The generated default redirects home without signing in.
An optional claim hook returns a product-specific claim URL; its default declines
with 409. A failed hook does not make a consumed nonce reusable.

The authenticated HTTP operations `POST /relay/session-ingest` and
`POST /claim/mint` also appear in MCP discovery and generated SDKs. Ingest uses
`INSTANCE_ID` (fallback `default`) and `INSTANCE_HMAC_KEY` (fallback
`HMAC_SECRET_KEY`). Claim minting accepts internal key ID `default` and, when
configured, instance-secret key ID `platform`. Route-specific HMAC policies do
not authorize those keys on unrelated routes. Sign the exact request body using
the normal ForkLaunch HMAC format.

`GET /relay/handoff?ticket=...` is a browser-only redirect. A ticket expires after
five minutes and is atomically consumed before minting the one-day Better Auth
cookie. Unknown, expired, replayed, or failed redemptions redirect home without a
new cookie. Redirects must be root-relative and contain no ASCII control bytes;
protocol-relative and slash-backslash prefixes are rejected. The handoff record
contains no provider tokens. Run migrations before starting an updated service:
relay adds a model to the existing encrypted/RLS/audited auth store.

For direct library integration, implement `relay::RelayHooks<S>`, configure
route-scoped keys with `http::relay::configure_auth`, and use
`AuthHttp::application_router_with_relay`. Include `http::relay::contracts()` in
any separately exported contract registry. Custom hook business logic remains
application-owned and must be tested with the application's provider policy.

`PostgresAuthStore::with_member_query_profile(MemberQueryProfile::Postgres)`
selects the configured MikroORM/PostgreSQL semantics for organization member
filtering and sorting. Direct library callers retain the memory-adapter profile
unless they opt in; the generated PostgreSQL preset opts in explicitly. The five
supported fields are `id`, `organizationId`, `userId`, `role`, and `createdAt`.
UUID comparisons apply only to `id`; organization/user IDs remain opaque text.
This profile assumes UUID member IDs, matching the configured ORM schema.
Custom native records with opaque non-UUID member IDs are rejected by filtered
queries after opting in; valid uppercase UUID spellings retain their original
stored keys during hydration.
Database collation, timestamp parsing/time zone, SQL LIKE wildcard/escape rules,
and numeric pagination casts are evaluated by the actual runtime database.

Ordinary unfiltered/unsorted integer pagination continues using blind-index
counts/windows without scanning the organization. Configured filter/sort queries
and other pagination forms collect at most 4096 candidate members, with a 1 MiB
serialized public-projection budget. Input queries are bounded before cloning or
reads to 64 KiB serialized JSON, 1024 entries per array, 4096 nodes and depth16.
The projection bound is not a bound on total decrypted memory: the existing
store reads/decrypts up to256 complete records per page, including custom fields,
before projected fields are checked. Exceeding either budget fails explicitly;
there is no truncation or memory-semantics fallback. Reads are audited and count
and page selection share the tenant transaction lock. Only returned page users
are hydrated. Stable ties retain native insertion order.

Public member fields are passed transiently as SQL parameters to a bounded
recordset; credentials and private user fields are excluded. This adds no
plaintext database persistence or application logging. Database/proxy parameter
logging can still record those public fields: deployments must configure that
logging according to their data policy. SQL identifiers/operators are fixed
allowlisted fragments, tenant/organization selection remains mandatory, and
invalid input/database failures produce the existing sanitized wire response.

Signup catches user-row creation failures as handled `FAILED_TO_CREATE_USER` (422).
Unexpected account/session or transaction failures return an empty 500 without
cookies and invoke an installed unexpected-only observer. Signout HTTP is
best-effort: storage failures still return success and expire the three session
cookies, while the strict service API retains errors and transaction rollback.
A successful HTTP signout during a storage failure does not prove server-side
revocation: the rolled-back persisted session remains valid until separately
revoked or expired, even though this browser has cleared its cookies.
Handled-failure diagnostics contain only static route and allowlisted structural
metadata. The fault oracle uses actual Better Auth handlers with an owned adapter;
its transaction-completion injection is not a PostgreSQL COMMIT-failure test.

`AuthHttp::with_jwt_header_disabled(true)` implements the optional Better Auth
`disableSettingJwtHeader` setting: session reads keep their normal JSON/session
behavior but skip JWT signing and the automatic JWT/expose response headers.
Explicit `/token` and `/jwks` behavior is unchanged, including external signer
failures and remote-JWKS suppression. The configured generated preset leaves
this option off. Separately configured CORS expose headers are unchanged.

`KeyRing::with_jwks_grace_period` and `AuthHttp::with_jwks_grace_period` accept
`std::time::Duration` for public-key publication grace (default 30 days).
Expired keys remain visible only while `expiresAt + grace > now`, compared in
whole milliseconds. This never extends private-key signing eligibility or enables
a local JWKS endpoint when using a remote signer. The typed subset excludes
JavaScript negative/NaN/infinite option values, rejects sub-millisecond durations
and values exceeding signed 64-bit milliseconds before storage access, and reports
checked timestamp overflow as an error. Existing first-100-key publication bounds
remain unchanged; no custom getJwks/createJwk adapter hooks are implied.

`AuthHttp::with_jwks_path` relocates public-key publication and the raw/application
OpenAPI operation; `jwks_path()` exposes the relative path for client/verifier
configuration. The default remains `/jwks`. The old path returns 404 after
relocation, as do HEAD/POST and the relocated endpoint with a remote signer.
Supported paths are fixed ASCII segments containing letters, digits, `.`, `_`,
`-`, and `~`. Encoded/dynamic/query/fragment paths, dot traversal, duplicate or
trailing slashes, and existing auth/application namespaces are rejected before
router construction. This intentionally excludes upstream dynamic routing and
conflicting paths that can shadow another authentication endpoint.
Root `/` is supported under a nonroot auth namespace: the application mount
preserves its trailing slash (for example `/api/auth/`), and the bare mount URL
returns 404. Raw relative routers serve `/`; manual nesting must preserve the
trailing slash. The complete auth namespace remains reserved. Root auth combined
with root JWKS is rejected in either builder order. Unknown application namespaces
remain subject to the application's registry/mount collision checks. Configure
external clients and JWT verification URLs separately when relocating JWKS.

`AuthHttp::with_jwt_subject_resolver` installs an optional asynchronous
`jwt.getSubject` callback independently of local/external signing. It receives
an owned, read-only `JwtSessionContext` after cookie verification and persisted
session/user validation. `None` falls back to the user ID; `Some("")` preserves
the empty subject. Only the JWT subject changes: persisted session ownership,
cookie identity, session ID, email and organization claims remain unchanged.
Custom subjects may require explicit application authorization mapping; they
must not be treated as replacement stored user IDs. The configured preset leaves
this hook unset. Header suppression skips the hook on session reads, while
explicit token issuance still invokes it. Callback errors return an unexpected
empty 500 without cookies before local key minting or external signing.
The callback is trusted server code. Snapshot records can contain sensitive
custom user fields and the secret session token; do not log or expose them.
Context Debug output is redacted. Arbitrary payload-definition callbacks and
custom JWKS adapter hooks remain separate options.

`AuthHttp::with_jwt_payload_resolver` installs an optional trusted asynchronous
`definePayload` policy over the same immutable, secret-bearing `JwtSessionContext`
as the subject resolver. Its object replaces the configured payload; it does not
merge session/email/organization claims. No callback preserves that preset, while
`None` means empty object spread (the upstream null/undefined behavior). Primitive
and array spreading are intentionally outside the typed object-return interface.
The initial `iat` is followed by callback claims, then the resolved `sub` overrides
any payload subject. Payload resolution runs before subject resolution or signing;
callback failures produce sanitized empty 500 responses without minting keys.
Existing local JOSE and external-signer claim defaults/validation still apply;
local claim validation can mint a key before rejecting invalid claims, as upstream
does. Suppressed automatic JWT headers skip both callbacks; explicit `/token`
still invokes them. Claims never alter persisted cookie/session ownership. Custom
claim policies may require separate application authorization mapping. Never log
or expose callback snapshots, which can contain secret session tokens.

`AuthHttp::with_jwt_expiration_at(DateTime<Utc>)` is the typed counterpart of a
trusted `jwt.expirationTime: Date`. It accepts millisecond-precision dates within
JavaScript's Date range and floors milliseconds to Unix seconds, including dates
before the epoch. An explicit payload `exp` still wins, while subject resolution
runs last. Past expiration dates intentionally issue already-expired JWTs; cookie
and persisted session lifetimes remain unchanged. Without this option, the
configured 15-minute JWT lifetime remains. Invalid JavaScript dates and finer
than millisecond native timestamps are outside this typed subset. Arbitrary duration-string
configuration on HTTP and custom JWKS adapter hooks are separate extensions.

`with_jwt_lifetime(Duration)` configures relative expiration using whole,
nonnegative JavaScript-safe integer seconds; zero matches the upstream immediate
expiry behavior. Fractional seconds and values above 9,007,199,254,740,991 seconds
are rejected before any storage access. The lifetime and absolute-date setters
replace one another: the last call wins. Existing payload `iat` arithmetic and
explicit non-null `exp` precedence are retained for both local and external
signers. This typed subset does not implement the full signed/fractional
JavaScript duration-string interface or change cookie/session expiry.


`AuthHttp::with_jwt_expiration_seconds(f64)` accepts a finite numeric absolute
JWT expiration in seconds (the numeric `jwt.expirationTime` option). Fractional,
negative, past, and large finite timestamps are retained through the existing
local/external signer paths; these are absolute NumericDate seconds, not a
relative duration or millisecond timestamp. Numeric, Date and lifetime setters
replace each other, so the last configured setter wins. Non-null payload `exp`
still overrides the configured expiry, and subject policy runs last. Persisted
sessions and cookies retain their existing expiration. Nonfinite numbers are
rejected before storage/signing; JavaScript external callbacks accepting such
non-JSON values are outside this typed configuration subset.

The internal key repository now provides one boundary for default-store key
reads, creation and retirement across issuance, signing, verification and JWKS
publication. A new key's rotation expiry is included in its initial encrypted
insert; there is no subsequent expiry-only update/audit mutation. Signing restores
and validates the record returned by repository creation rather than retaining the
pre-persistence generated key. Tenant-bound transactions, default 100-key lookup
window, rollback and post-creation claim-failure persistence remain in effect.
The typed custom adapter described below shares this boundary; absent hooks keep
the existing encrypted tenant store as their fallback.


`keyring::adapter::JwksAdapter` provides independent `with_get_jwks` and
`with_create_jwk` callbacks. Install it with `KeyRing::with_jwks_adapter` or
`AuthHttp::with_jwks_adapter`; session JWT issuance, explicit signing, verification
by key ID, live/algorithm selection and public JWKS reads use that same configured
source. If primary-algorithm selection is empty, the custom reader is invoked
again for latest-any fallback, and the second snapshot is authoritative as in
Better Auth. Default-store selection retains its atomic snapshot. The read callback returns `Option<Vec<JwksKeyRecord>>`; `None` means no
records. The create callback receives `JwksKeyMaterial` with creation/rotation
expiry already set and returns the authoritative record, including its chosen ID
and key material. It owns persistence before returning. After empty-source
initialization, JWKS re-reads that source and errors if still empty, matching the
upstream independent-hook behavior. Missing hooks continue to use the default
store; configuring one hook does not implicitly copy records between backends.

The adapter's tenant label is fixed by server configuration and passed with the
trusted auth base URL and clock. Requests cannot supply it. The application must
bind its callbacks to the same tenant as its `AuthStore`: the label does not
implement isolation in an arbitrary external backend. Default-only operations
retain their atomic tenant transaction. Before invoking either custom callback,
any fallback transaction is committed and released; callbacks can safely access
the same store without a held default lock. Mixed repositories do not form one
atomic transaction, and callback failures cannot roll back callback-owned effects.
Callback errors are sanitized. Custom sources own synchronization and rotation;
`KeyRing::rotate` rejects an active custom hook rather than modifying unrelated
fallback records. External JWT signer mode still bypasses local key callbacks.

This is a typed native key format: `JwksKeyMaterial` contains validated plaintext
PKCS#8 bytes with an explicit supported algorithm and millisecond-precision times;
`JwksKeyRecord` adds an ID. It does not import Better Auth's encrypted private JWK
strings or replace their encryption format. The default PostgreSQL backend still
encrypts records; custom callbacks must provide their own encryption at rest.
Private byte buffers zeroize on drop, including explicit clones; secret-bearing
material/record types have neither `Debug` nor serde implementations. Trusted
persistence code can read private bytes and must not log them. Malformed records and arbitrary JavaScript adapter objects are outside this
typed interface. Typed callback records carry explicit algorithms; legacy native
store records use the bounded normalization described below. Custom read sets above 4096 records
fail explicitly; they are not truncated to the default store's 100-record window.
No generic request context, arbitrary plugin schema, or per-call signing-key
pinning API is introduced by these callbacks.

`JwksAdapter::with_get_jwks_records` additionally accepts mixed
`JwksReadRecord::Private` and `::Public` snapshots. `JwksPublicKeyRecord::new`
accepts the exact native public-JWK shape (including matching `kid` and `alg`),
with at most six fields and 2048 bytes per string. Unknown/private fields are
rejected before serialization. Public-only records support publication and
signature verification. Selecting one for signing fails without minting or
silently selecting another key. The existing private-only reader, material
accessor and create callback are unchanged. A public-only create return remains
unrepresentable by the typed create callback; upstream accepts it structurally
but signing subsequently fails. This does not add arbitrary JWK import or change
the encrypted default backend.

`KeyRing::sign_jwt_with_key_id` is a trusted server-only per-call constraint,
corresponding to upstream `signJWT` configuration `signingKeyId` with an optional
`signingAlgorithm` assertion. It is separate from JSON `overrideOptions` and
adds no HTTP endpoint. Default storage uses an exact tenant-bound ID lookup;
a custom source is read exactly once. Missing, expired, algorithm-mismatched,
public-only or invalid keys fail without minting or selecting an alternative.
Expiry equal to the current millisecond is accepted, matching upstream's strict
less-than check for an explicitly pinned ID. IDs are limited to 512 bytes.
Missing/null algorithm metadata inherits the trusted configured key profile.
Algorithm-only lazy provisioning, caller-supplied resolved signing
keys and forwarding pinning metadata to an external signer remain separate APIs.

`KeyPairConfig` configures the supported local algorithm and RSA generation size
through `KeyRing::with_key_pair_config` or `AuthHttp::with_key_pair_config`.
The default remains EdDSA; RSA sizes remain bounded to 2048–4096 bits. Trusted
per-call signing `overrideOptions.jwks.keyPairConfig.alg` overrides the configured
algorithm for that signing operation. Providing a `jwks` override replaces the
configured object, so an empty override resets the algorithm to EdDSA and RSA
size to 2048, matching the upstream shallow merge. Issuance, empty-store initialization,
rotation, legacy verification and publication share the configured profile.

For native persisted records whose algorithm is missing or null, normalization
fills missing public `alg`/`kid` and missing row curve metadata in memory only.
Cryptographic coordinates and public curve remain required. No stored row is
rewritten and no update audit is emitted. Public JSON is bounded to 16 KiB before
parsing. Conflicting metadata, private/unknown fields and unsupported shapes fail
rather than overriding configured defaults. Explicit-algorithm records retain
the existing strict restoration/publication and verification behavior. This is
native PKCS#8 compatibility, not import of Better Auth encrypted private JWK
strings. Upstream's arbitrary public-JWK metadata spread remains outside this
strict normalization profile; typed custom adapters already supply explicit
validated algorithm metadata.

`KeyPairConfig::with_rotation_interval(Duration)` optionally expires newly
created local keys. Durations must contain whole milliseconds and fit the
JavaScript Date range; zero disables expiry. Defaults remain unchanged. Session
issuance, explicit signing without a `jwks` override, initial JWKS publication
and operator rotation use the same configured interval. A `jwks` override
replaces the configured object, including this interval. Existing rows retain
their stored expiry. Unpinned signing replaces expired keys on demand; no timer
is installed. Explicit key-ID signing never replaces its requested key. Public
retention still uses the independent grace period and strict expiry-plus-grace
boundary. Invalid configured expiration arithmetic fails before repository I/O;
this bounded native configuration rejects unsupported clocks/overflow instead of
persisting an invalid JavaScript Date. Custom callbacks retain persistence and
synchronization ownership; no cross-store atomicity is implied.

`KeyRing::sign_jwt_with_algorithm` adds a trusted server-only algorithm constraint.
It reads once, selects the newest live matching key and never falls back to a
key with another algorithm. Existing matching keys remain usable even when not
advertised for creation. Missing keys can be created only for the configured
primary or profiles supplied by `KeyPairConfig::with_additional_profiles`.
The typed list rejects duplicate/primary algorithms, more than 16 entries,
nested lists and per-extra rotation intervals; the shared rotation policy applies.
An expired matching key is excluded, so an advertised profile may be minted on
next use. Explicit ID pinning still refuses expired IDs without replacement.

A `jwks` override clears inherited additional profiles through the same shallow
replacement semantics as other key options. The constrained API rejects raw
`keyPairConfigs` instead of treating request-shaped JSON as new advertisement;
use the trusted typed configuration. Selecting a public-only record fails without
minting. A trusted custom creation callback still owns its returned key, including
its algorithm: actual Better Auth uses that return even if it differs from the
requested generation profile. No extra callback call or default-store write is
invented to enforce a different contract. Applications requiring stricter remote
key provisioning must enforce that constraint inside their trusted callback.

External signers can opt into `JwtSigner::sign_with_context` and the trusted
`KeyRing::sign_jwt_with_signer_context` entrypoint. `JwtSigningContext` preserves
an absent versus empty header object, explicit JSON nulls, and optional key-ID
and algorithm hints. These hints are forwarded without local lookup or algorithm
validation; the external callback owns enforcement. Unlike local JOSE signing,
which replaces header `alg`/`kid` with the selected key, the external callback
receives caller headers unchanged. Existing implementations of `sign` remain
compatible: the provided context method deliberately ignores metadata unless
an implementation overrides it. `ExternalJwtSigner` forwards the context-aware
method; ordinary HTTP session/token issuance supplies an empty context and does
not accept key-selection authority from request parameters.

The typed context bounds ID/algorithm strings to 512/128 bytes and header objects
to 1024 values, 32 nested levels, 16 KiB of raw text and 64 KiB of encoded JSON.
It has no Debug/Serialize implementation. This is a bounded JSON subset, not
JavaScript undefined/prototype parity. Callback errors retain the existing native
sanitized failure response rather than exposing upstream callback exception text.

## Optional delivered OTP

Two-factor routes remain off until `AuthHttp::with_two_factor()` or
`AuthHttp::with_two_factor_otp(Arc<dyn OtpDelivery>)` is selected. The latter adds
server-owned OTP delivery alongside verified TOTP. Domain callers can configure
`TwoFactorService::with_otp_delivery`. Delivery destinations and implementations
are never accepted from an authentication request. The hook receives an owned
`OtpDeliveryContext`: `user()` exposes the public persisted-user projection and
`code()` exposes the short-lived secret. No session token is supplied; the context
has no Debug or Serialize implementation. Do not log or return the code.

Implement `OtpDelivery::prepare(context) -> BlueprintResult<OtpDeliveryFuture>`.
The returned owned, Send future produces `BlueprintResult<()>`. Preparation
failure yields a sanitized empty HTTP 500. A future returning an error records a
sanitized internal diagnostic and retains HTTP 200, matching upstream asynchronous
delivery failure. Default dispatch awaits the future after committing the OTP and
releasing the storage transaction. Explicit Result failures are supported; Rust
panics are outside the callback contract. No optional background-task dispatcher
is implemented. Cancellation after commit can leave the expiring code stored;
it never issues a session or rolls back a code that may already have been delivered.

The default profile uses six uniformly generated decimal digits, 180 seconds of
validity (the exact expiry instant is still valid), and five failed attempts.
Resending replaces the code for the same verified session or signed challenge;
distinct challenges and tenants remain isolated. A successful verification and
session/trust issuance commit atomically. Invalid attempts commit their counters;
other persistence failures roll back consumption. Removing the delivery hook
blocks new sends and OTP enrollment but still permits verification of already
issued codes while two-factor routes remain enabled. OTP enrollment and direct
authenticated OTP verification rotate the current session when enabling protection.
Password sign-in advertises methods captured inside the challenge transaction.

OTP-only users have no TOTP/recovery record and therefore no account-wide factor
failure counter, matching upstream. Where a TOTP record exists, OTP failures share
its account lockout. The per-code attempt bound and HTTP rate policy still apply.
Custom storage callbacks, optional background dispatch, and other
plugin options remain separate parity work. This profile protects subsequent
password sign-ins; existing sessions and social/provider flows retain their own
policy. The actual-client fixture uses an owned JSON-line HTTP-handler bridge;
it does not claim browser delivery or transport behavior.

`OtpOptions::new(digits, validity: std::time::Duration, allowed_attempts)` adds
trusted policy through `TwoFactorService::with_otp_options` or
`AuthHttp::with_two_factor_otp_options(delivery, options)`. It accepts 1–32 digits,
1–100 attempts, and positive whole-millisecond durations representable as signed
milliseconds. Timestamp addition is checked before storage reads or delivery;
out-of-range expiry fails without a write. Upstream numeric minutes are represented
as a Duration, so 0.025 minutes is `Duration::from_millis(1500)`. Negative, nonfinite,
zero/fallback, and sub-millisecond JavaScript option values are outside this typed
subset. Constructors reject unsupported values instead of broadening the policy.

An issued code keeps its persisted expiry when policy changes. Verification uses
the service's **current attempt limit**, matching upstream; changing digits does
not reject an otherwise valid previously issued code. Resending resets attempts
and uses the current validity/digits. Options are trusted server configuration,
never request parameters. Default builders retain six digits / 180 seconds / five
attempts. Custom OTP storage transforms and background dispatch remain separate
unsupported options; the existing encrypted record format is unchanged.

## Trusted TOTP parameters

`TotpOptions::new(TotpDigits::Six | TotpDigits::Eight, Duration)` accepts a positive
whole-millisecond period from 1ms through 24h. Use
`TwoFactorService::with_totp_options` or
`AuthHttp::with_two_factor_totp_options`; the HTTP builder preserves independent
OTP delivery/options in either builder order. Default builders retain SHA-1,
six digits, 30 seconds and the fixed ±1-step verification window. These parameters
are trusted configuration; request fields cannot override them. Both enrollment
and URI retrieval advertise the configured digits/period. Current configuration
also governs verification of existing secrets without rewriting factor records.
Applications changing policy must account for authenticators retaining old settings.

The duration maps to upstream seconds as binary64 `milliseconds / 1000`, followed
by the upstream `seconds * 1000` division and floor for the counter. This matters
at fractional-period boundaries: 2.007 seconds produces a counter of 99 at
200700ms, rather than integer division's 100. Supported clocks are nonnegative
JavaScript-Date milliseconds through 8640000000000000. Invalid clocks fail closed
before unsigned conversion. The previous window at epoch zero is skipped: upstream
wraps that negative counter to unsigned64, while native retains its historical
fail-closed boundary. Negative clocks, sub-millisecond/zero/negative periods,
periods over 24h, and other digit widths are outside this typed subset.

TOTP lifecycle policies are described below. No configurable hash algorithm
is exposed by the upstream plugin; this profile continues to use SHA-1. The actual
source fixture also characterizes these lifecycle policies independently of the
cryptographic parameter vectors.

## Trusted two-factor lifecycle policy

`TwoFactorLifecycleOptions::default()` preserves required passwords, enabled TOTP,
and verification before enrollment enables protection. Apply it through
`TwoFactorService::with_lifecycle_options` or
`AuthHttp::with_two_factor_lifecycle_options`. These builders preserve the installed
OTP delivery hook and both OTP/TOTP parameter sets. Request fields never set policy.

- `with_totp_disabled(true)` blocks TOTP verification before authority lookup;
  URI retrieval first requires a session, and enrollment first checks its password.
  Existing trusted-device authentication remains valid. Otherwise a password login
  can issue an empty-method challenge: it grants no session/JWT, but recovery codes
  remain available. Disabling the method does not delete factor data or backup codes.
- `with_skip_verification_on_enable(true)` atomically marks enrollment verified,
  enables protection and rotates the current session without waiting for a TOTP.
  Other sessions and trust records remain intact. Failure during rotation rolls back
  the factor, user flag and session writes together. Defaults still require a code.
- `with_issuers(enrollment, uri)` accepts independent optional issuer strings of at
  most 256 UTF-8 bytes. Empty strings mean fallback. Enrollment chooses a nonempty
  request issuer, then its configured issuer, then the application's issuer. Later
  URI retrieval chooses its independent issuer or the application issuer.
- `with_passwordless(true)` permits an omitted password only if the persisted
  credential account has no password. An existing password still requires proof.
  `with_uri_passwordless(Some(false))` and `with_backup_passwordless(Some(false))`
  override that setting for URI retrieval and backup generation; `None` inherits.
  HTTP validation/OpenAPI reflect only these trusted per-route settings. Direct
  domain callers pass an empty password string when omitting it.

Passwordless authorization always begins with a persisted, unexpired session.
The native implementation additionally rechecks credential-password absence inside
its mutation transaction: a password added after the initial snapshot rejects the
operation. This stronger atomic race guarantee is tested separately from upstream
stable-state ordering. Missing, null and empty credential-password strings count as
absent; malformed non-string values fail closed. Existing required-password paths
retain the hash snapshot and mutation-time comparison. No email linking or new
social-sign-in protection is implied. Lifetime and generation options are described
below; custom backup storage codecs remain separate parity work.

`TwoFactorLifetimeOptions::new(challenge, trusted_device)` accepts independent
whole-second `std::time::Duration` values from zero through 365 days. Defaults stay
600 seconds and 30 days. Apply through `TwoFactorService::with_lifetime_options`
or `AuthHttp::with_two_factor_lifetime_options`; other configured factor options
and delivery hooks survive either builder order. This bounded API excludes
fractional, negative and nonfinite JavaScript values.

The policy sets both durable record expiry and cookie `Max-Age`. Existing records
retain their expiry after policy changes; new challenges and newly issued/renewed
trust use the current policy. Challenge proofs are valid at their exact expiry
instant and rejected afterward, matching the upstream consume boundary. Trusted
proofs require strictly future expiry. Thus zero-age challenges are consumable only
at their issuance instant by a caller retaining the cookie, while zero-age trust
is immediately expired; browsers may discard zero-age cookies immediately. This
changes neither session expiry nor delivered-OTP lifetime. Trust renewal remains
transactional and one-use, including rollback when replacement insertion fails.

`AccountLockoutOptions::new(enabled, threshold, duration)` configures the shared
pending-sign-in failure budget through `TwoFactorService::with_account_lockout_options`
or `AuthHttp::with_two_factor_account_lockout_options`. Defaults remain enabled,
10 failures and 900 seconds. The typed subset admits thresholds 1..=100 and
whole-millisecond durations from zero through 365 days. Invalid options fail before
storage access; negative/nonfinite JavaScript options are outside this profile.

TOTP, backup codes and delivered OTP share this counter only during pending sign-in
and only when a factor row exists. Authenticated verification bypasses the account
budget without resetting it; OTP-only users without a factor row retain their
separate OTP attempt limit. Per-challenge and OTP attempt limits remain enforced
when account lockout is disabled. Disabled account policy leaves historical counters
and deadlines untouched, including on success. Re-enabling honors the stored
existing deadline, while the current threshold/duration controls later failures.
Locks clear at equality; zero-duration locks can therefore clear on the next request.

Failure increments, lock deadlines, proof attempts and successful consumption are
transactional in the native store. Expired-lock clearing also persists when the
request deliberately rejects an exhausted challenge; backend errors roll back the
transaction. Counters must be unsigned integers (missing/null means zero), and
increment overflow fails closed. Concurrent native failures cannot lose increments.

Lock deadlines follow upstream binary64 arithmetic (`duration_ms / 1000 * 1000`
plus the clock's millisecond timestamp) and JavaScript Date truncation toward zero.
The admitted clock/deadline range is the intersection of JavaScript Date's
±8.64e15 milliseconds and Chrono's representable UTC dates. An unrepresentable
result fails before mutating the counter. Sub-millisecond clock precision is
normalized by `timestamp_millis`; unrelated session/OTP deadline logic is unchanged.
Disabled-policy successful TOTP/OTP verification does not rewrite the factor row;
it therefore does not require factor-update capability merely to reset a disabled
counter. Backup consumption and enrollment still update the factor. A dedicated
factor-update-denying store wrapper tests this distinction over memory and the
restricted PostgreSQL store, including rollback when backup consumption is denied.

### Backup code generation options

`BackupCodeOptions::new(amount, length)` configures future generation: amount is
0..=100, random-character length is 1..=127, with defaults 10/10. A dash is inserted
after the first five characters (or after all characters for shorter codes).
`TwoFactorService::with_backup_code_options` and
`AuthHttp::with_two_factor_backup_code_options` install this trusted policy;
request fields cannot change it. Existing codes remain valid independently of
later generation options until consumed or regenerated.

`with_backup_code_generator(Arc<dyn BackupCodeGenerator>)` on the service, or
`with_two_factor_backup_code_generator` on `AuthHttp`, installs a synchronous
server callback overriding numeric options. Closures returning
`BlueprintResult<Vec<String>>` implement the trait. Output is bounded to 100 codes,
each at most 128 UTF-8 bytes. The upstream typed callback is synchronous; no
asynchronous generator behavior is promised. The callback receives no user,
session, token or secret context, runs after authorization/password checks inside
the mutation transaction, and must not reenter that store. Explicit errors and
out-of-bounds output produce a sanitized empty HTTP 500 without cookies; Rust
panics are outside this fallible contract. Enrollment and regeneration mutations
roll back on failure.

The callback owns proof strength. Empty lists, empty strings, duplicate strings
and Unicode are preserved, matching the upstream custom-generator behavior. A
successful verification consumes every exact duplicate of that proof atomically.
Do not log generated proofs or callback error contents. Codes continue to use the
existing encrypted tenant store; custom backup storage codecs are a separate,
unimplemented extension. JavaScript negative/fractional generation sizes and
positive amounts with zero length are outside this bounded typed profile.
