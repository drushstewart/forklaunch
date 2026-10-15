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
below; custom backup storage codecs are described below.

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
existing encrypted tenant store; built-in backup field codecs are described below.
Custom asynchronous backup codecs are described below. JavaScript negative/fractional generation sizes and
positive amounts with zero length are outside this bounded typed profile.

### Backup code field storage and migration

`BackupCodeStorage::{LegacyArray, PlainJson, Encrypted}` selects a trusted service
policy through `with_backup_code_storage`, or
`AuthHttp::with_two_factor_backup_code_storage`. Direct `TwoFactorService::new`
retains `LegacyArray` for existing callers. **The first HTTP MFA opt-in now selects
`Encrypted`**, matching the configured Better Auth plugin; subsequent option
builders preserve the selection. An explicit legacy/plain builder overrides it.
HTTP request fields cannot choose a codec. Tenant record encryption remains
required independently of this field policy.

New string fields carry the private native marker `backupCodesFormat: 1` alongside
`backupCodes`. Unmarked legacy arrays remain readable under every profile, with no
read-time rewrite. Authorized regeneration or successful consumption writes the
selected format. Unknown markers, unmarked strings, or incompatible shapes fail
closed during verification; authorized regeneration replaces old proofs without
decoding them. Older native binaries do not understand marked strings: upgrade all
readers before enabling this writer. The marker is a native storage format, not
an upstream database-row interchange format.

The current selected policy decodes marked strings; it does not autodetect the
old codec. Plain-to-encrypted produces a sanitized empty HTTP 500 and preserves
codes/attempts; encrypted-to-plain rejects the proof and applies ordinary pending
challenge attempts. Restoring the old policy allows the unchanged proof. Malformed
plain JSON/null rejects the proof; malformed encrypted fields, unknown keys, and
authentication-tag errors fail without consuming codes. Arbitrary JSON objects or
non-string code arrays are outside the typed profile and fail closed. All success
mutations (code removal, challenge, session, trust and counters) share the existing
tenant transaction; failure to commit preserves all of them. Every duplicate of a
successfully supplied code is removed. This stronger transaction composition is
not a claim of identical upstream multi-adapter races.

`BackupCodeEncryptionKeys::legacy(secret)` and `::versioned(current, keys, legacy)`
provide independent keys through `with_backup_code_encryption_keys` or the HTTP
`with_two_factor_backup_code_encryption_keys`. Keys have no Debug/Serialize or
secret accessor; derived keys are zeroized. Without explicit keys, encrypted mode
uses the service's signing-secret bytes. Secrets are 32..=4096 UTF-8 bytes; up to
16 distinct u32 versions are supported. Encryption uses SHA-256 key derivation and
XChaCha20-Poly1305 with a fresh 24-byte nonce, followed by hex. Versioned writes use
canonical `$ba$version$hex`; retained read versions and optional legacy keys permit
rotation. Permissive upstream version-read prefixes are bounded to 48 bytes.

The admitted 100 codes × 128 UTF-8 bytes require up to 77,101 escaped JSON bytes,
154,282 bare ciphertext hex bytes, or 154,330 bytes including the bounded read
prefix (canonical u32 writes need at most 15 prefix bytes). Existing OTP limits
remain 32 plaintext bytes and 192 stored bytes. Empty/Unicode codes and duplicates
remain admitted, as with the trusted generator. Four fixed-nonce Node/Rust vectors,
actual plugin policy/failure observations, HTTP defaults, and restricted PostgreSQL
transaction/one-use tests cover this built-in subset. Custom asynchronous backup
codecs use the separate reservation protocol below.

### Asynchronous backup field codecs

Install a trusted `BackupCodeEncryptionCodec` with
`TwoFactorService::with_backup_code_encryption_codec` or
`AuthHttp::with_two_factor_backup_code_encryption_codec`, passing
`BackupCodeCodecOptions::default()` (30 seconds). Its paired `encrypt` and
`decrypt` methods receive owned `BackupCodeCodecText` and return
`BackupCodeCodecFuture`, a fallible owned future. No user, password, session token,
or database transaction is passed to the callback. Text is zeroized on drop and
has no Debug/Serialize implementation. Callback errors become a static empty HTTP
500; callback error contents and proofs must never be logged. Panics are outside
this Result contract. Explicit builtin storage/key selection removes the hook;
other configuration builders preserve it.

Plaintext JSON is bounded to 77,101 UTF-8 bytes and 100 strings of at most 128
UTF-8 bytes each; encoded callback text is bounded to 154,330 bytes. Empty and
Unicode callback text are admitted. The existing supplied-proof HTTP bound remains
128 bytes. Decryption uses the currently installed codec for marked strings;
unmarked legacy arrays remain readable. Successful custom writes carry native
`backupCodesFormat: 2`; every builtin/custom mutation also writes a fresh revision.
Builtin readers reject format 2 instead of guessing its encoding. Upgrade every
reader before enabling custom writers: these native envelopes/revisions are not
upstream row interchange, and older binaries lack their concurrency protections.

A versioned encrypted Verification reservation records an opaque owner, generation
and authority digests, phase, and deadline. No transaction spans an awaited user
future. Enrollment/regeneration reserve before encoding; verification reserves
before decoding and records a second Reencoding phase before encoding remaining
proofs. Final publication checks the same reservation, factor revision, current
session/challenge, password policy, account lockout and fresh clock, then commits
factor/challenge/session/trust mutations atomically. Stale signout, deletion,
regeneration and competing consumption cannot resurrect or reissue proofs. These
checks deliberately strengthen upstream races; they do not promise identical
multi-adapter scheduling.

Decode errors preserve proofs and attempts. Invalid decoded JSON proofs apply the
ordinary failed-attempt policy. Reencoding failure preserves proofs but consumes
only an unchanged pending challenge; authenticated sessions remain intact. A
changed challenge is never deleted by an older reservation. Failed regeneration
encoding preserves old proofs. Timeout/cancellation leaves a durable reservation
for restart recovery; expired Decoding/encoding reservations release ownership,
while expired Reencoding reservations additionally consume that exact unchanged
challenge. Lazy recovery commits even when it invalidates the supplied challenge.
`cleanup_backup_codec_states` offers explicit tenant-scoped pages of at most 256
rows; ordinary expired-verification cleanup observes the same recovery rules.
No background scheduler or detached callback is installed.

The validated 1..=30,000 whole-millisecond lease covers both callback stages.
Timeouts bound cooperative polling and publication checks, not synchronous method
construction or blocking/non-yielding code. Clock checks occur after writer
acquisition and immediately before commit; they cannot guarantee when a database
COMMIT physically completes. Missing reservations, different owners, expired
leases and upgraded instances without the codec fail closed. Arbitrary JavaScript callback/value semantics remain outside this typed profile.

### OTP field storage policy

`TwoFactorService::with_otp_storage(OtpStorage::Hashed)` or
`AuthHttp::with_two_factor_otp_storage(OtpStorage::Hashed)` selects SHA-256 over
UTF-8 followed by unpadded base64url. `Plain` remains the default. Both modes still
use the existing encrypted tenant record: field hashing is separate from storage
encryption and is not a replacement for rate limits on low-entropy OTPs. Delivery
receives the original code; the stored representation and diagnostics never expose
it through these builders.

Current policy controls verification, including previously issued codes. Switching
plain/hashed modes causes an ordinary invalid-code attempt; restoring the original
mode allows the remaining unexpired proof. Current digit settings do not infer or
validate an old code's width from the hash's 43-character encoding. Attempts,
account lockout, expiry, atomic single use, and rollback retain their existing
semantics. Invalid string inputs within the existing 128-byte limit count as failed
proofs; malformed non-string bodies fail validation before consuming attempts.

Built-in encrypted OTP encoding and asynchronous custom OTP callbacks are described
below. Their failure/consumption rules differ from this plain/hashed profile.
Custom asynchronous backup codecs are described below.


### Encrypted OTP field representation

`OtpStorage::Encrypted` uses SHA-256 of the existing trusted service signing-secret
bytes, then XChaCha20-Poly1305 with a fresh random 24-byte nonce. The stored field is
lowercase hex of nonce, ciphertext and tag. `OtpEncryptionKeys::legacy(String)`
selects independent bare-hex field keys; `OtpEncryptionKeys::versioned(current,
Vec<(u32, String)>, Option<String>)` adds `$ba$version$hex` envelopes and optional
legacy decoding. Install an `Arc` with `with_otp_encryption_keys` on the service or
`with_two_factor_otp_encryption_keys` on `AuthHttp`; this also selects encrypted
mode. Selecting Plain/Hashed leaves the configured keys available for later
Encrypted selection. Cookie signing and generic tenant-row encryption are unchanged.

Configuration accepts 1..=16 distinct version keys, a present current u32 version,
and 32..=4096-byte UTF-8 secrets. Derived keys and temporary owned secrets are
zeroized; the key configuration has no Debug/Serialize implementation. Retained
versions can decode old proofs; retired/unknown keys, malformed envelopes/hex and
invalid tags produce a sanitized empty 500 **after committing OTP consumption**,
matching upstream. No new failed-attempt increment or session/trust issuance occurs;
expired account-lock clearing that precedes consumption still commits. A failed
transaction commit instead rolls back consumption and returns the storage failure.
Asynchronous custom OTP codecs are described below; no database transaction
spans their user futures.

The admitted ciphertext payload is at most 32 bytes (the current maximum generated
OTP width), with at most 192 bytes of stored envelope text. Version parsing admits
the bounded decimal-prefix behavior used by upstream, including leading ECMAScript
whitespace and `+`; unsupported versions fail closed. Decryption uses replacement
UTF-8 decoding like upstream. The existing 128-byte supplied-code limit remains:
upstream can accept a 144-character ciphertext as the proof after switching to
Plain, while this bounded native profile rejects that input before consuming an
attempt. Existing digit proofs and hashed fields are not rewritten. Plain/Hashed
to Encrypted conversion errors consume; Encrypted to Plain/Hashed comparison
failures retain the proof and increment its attempt count.

### Trusted asynchronous OTP field codecs

`TwoFactorService::with_otp_hash_codec` and `with_otp_encryption_codec` install
server-owned `OtpHashCodec` or `OtpEncryptionCodec` callbacks. `AuthHttp` exposes
matching `with_two_factor_otp_*_codec` builders. Configure delivery separately;
these builders preserve OTP/TOTP and lifecycle policies. Selecting a built-in
`OtpStorage` or encryption key configuration removes the custom codec. The
feature is opt-in; default field storage remains plain inside encrypted records.

Callbacks receive only owned `OtpCodecText` secrets and return an owned
`OtpCodecFuture`. Secret wrappers have neither `Debug` nor `Serialize` and zeroize
their strings on drop. Inputs and outputs are limited to 1024 UTF-8 bytes; empty
and Unicode results are supported. Encoded/hash output cannot contain `:` because
upstream uses that delimiter for its attempt counter. The HTTP supplied-code
limit remains 128 bytes. Callback `Err`, timeout and invalid output produce a
sanitized empty HTTP 500. Rust panics and blocking callback constructors are not
modeled as callback errors; callbacks must cooperate with the Tokio runtime.
`OtpCodecOptions` admits whole-millisecond deadlines from 1ms through 30s and
requires an enabled Tokio timer. This bounds cooperative future waiting and
acceptance of results, not execution of blocking or non-yielding callback code;
there is no forced interruption or panic guarantee. No transaction is held while
callbacks run.

A private versioned envelope in the existing encrypted verification record binds
encoding/verification to a random revision and persisted deadline. No SQL schema
migration or new `AuthStore` methods are required. Verification consumes proof
availability before invoking the callback; failure or cancellation cannot rearm
that proof. Failed resend encoding restores an older proof only if it still owns
the exact reservation. Session/trust issuance, counters and final state commit
atomically. Completion rechecks the exact row/revision, authority, current
lockout and clock. Clock checks occur after acquiring each writer, before each
commit and again before invoking a callback after its reservation commit. A final
COMMIT can itself complete after the checked deadline; this is not a promise of
post-COMMIT rollback or an absolute storage-completion deadline. A new send, consumed replacement, signout or expired authority
cannot be overwritten by an older callback. These race checks are deliberately
stronger than upstream's audited asynchronous behavior.

Existing methods advance their supplied trusted time with monotonic elapsed time
while awaiting callbacks. The additive `send_otp_with_clock` and
`verify_otp_with_clock` entrypoints sample a caller-owned clock again before final
commit; HTTP uses its configured clock. Backward clock movement fails closed.
Cancellation spawns no detached task. Expired encoding leases lazily recover a
still-live older proof; expired verification leases remain consumed.
`cleanup_otp_codec_states` visits at most 256 tenant verification records and
returns an opaque continuation. Operators must invoke it to physically remove
unvisited expired tombstones; no automatic background sweep is promised. Removing
an expired state cannot authorize a stale callback, which requires an existing
matching revision and unexpired lease.

Instances without a codec recognize custom envelopes and reject verification
without deleting pending work. Deploy this envelope-aware version to all readers
before enabling custom codecs: older binaries do not implement the envelope
lifecycle. This native envelope is not a Better Auth database interchange format.
Custom callback storage is distinct from whole-record encryption. Backup-code
field codecs remain a separate capability.

### Trusted server-only backup-code disclosure

`TwoFactorService::view_backup_codes_for_user(user_id)` reads current plaintext
backup codes without consuming, regenerating, migrating, or resetting any proof,
challenge, session, or counter. Like upstream `auth.api.viewBackupCodes`, this is
trusted server code: the caller must authorize the user ID, normally from a
verified session. It does not authenticate a raw ID or require a User/password
record; tenant isolation comes from the configured store. No HTTP, MCP or client
endpoint is registered. `ViewedBackupCodes` exposes borrowed string iteration but
has no Debug/Serialize; owned strings zeroize on drop. Do not log or indiscriminately
serialize this intentionally sensitive result.

The current builtin/custom codec is used without autodetection. Legacy arrays stay
unchanged, duplicates and empty strings survive, and an empty list is valid.
Missing factors produce `BACKUP_CODES_NOT_ENABLED`; invalid JSON/null/false/zero/empty-string JSON produces
`INVALID_BACKUP_CODE`; incompatible keys, formats, callback errors and non-array
JSON fail closed. Upstream can return arbitrary truthy JSON despite its string-array
type; this API retains the bounded 100 strings × 128 UTF-8 byte profile.

Any existing backup-code reservation, active or expired, rejects the read before
decoding. Explicit cleanup remains separate. Async callbacks run outside a database
transaction; a second read verifies the factor generation and absence of a lease,
with fresh-clock and cooperative timeout checks. Builtins and custom legacy-array
reads also recheck the generation after checking reservation absence immediately
before commit; the no-callback legacy-array path has no callback deadline. Ordinary
changes after the final read remain possible. Deletion, regeneration or a new
reservation during decoding rejects the result instead of returning upstream's
stale snapshot. No state is written on success, failure, cancellation or timeout.
These checks do not revoke plaintext already returned, detect every completed
no-change reservation, or authorize the caller. The trusted `_with_clock` variant
supports deterministic policy tests; blocking/non-yielding callbacks remain outside
the cooperative timeout guarantee.

### Application-owned OTP background delivery

`with_otp_background_dispatcher(Arc<dyn OtpBackgroundDispatcher>)` on
`TwoFactorService`, or `AuthHttp::with_two_factor_otp_background_dispatcher`, opts
OTP delivery into synchronous handoff to an application-owned executor. Existing
OTP/storage/MFA builders preserve the dispatcher. Without it, delivery is still
awaited. The send operation first commits its encrypted proof, then calls the
existing delivery hook's fallible `prepare`. Preparation failure remains an empty
500 and never invokes the dispatcher. A successfully prepared task is handed off
once; the response returns `{ "status": true }` without awaiting delivery.

`OtpBackgroundDispatcher::dispatch` receives an owned `OtpBackgroundTask` implementing
`Future<Output = ()> + Send + 'static`. It contains no public secret-inspection,
Debug, or Serialize surface. An application may spawn it on its own runtime or
place it in a bounded queue; it owns capacity, rejection, draining during shutdown,
and cancellation. This library creates no detached task or implicit runtime.
Dropping an unpolled/pending task cancels its Rust future. Unlike an eager JavaScript
Promise, a Rust future does not execute merely because it was constructed. No
claim is made that dropping it cancels external work the hook already started.
Blocking dispatcher/callback code and panics are outside the fallible contract.

Async delivery errors and dispatcher `Err` values are reduced to static sanitized
diagnostics; secret error contents are discarded. They leave the committed OTP
valid and do not change the successful response or mint a session, cookie, or JWT.
A dispatcher that accepts a task and then returns an error owns that accepted task;
there is no compensating cancellation or proof rollback. No storage transaction
is held across delivery polling. The same path serves builtin and custom OTP
field storage. The shared scheduler also serves opt-in password-reset delivery below. Email
verification, invitations, rate cleanup and other plugin background work remain
separate capabilities.

### Opt-in password-reset delivery and shared scheduling

`AuthHttp::with_password_reset_delivery(Arc<dyn PasswordResetDelivery>)` enables the
existing `/request-password-reset` route. Without the hook it retains
`RESET_PASSWORD_DISABLED`. A standalone tenant-bound `PasswordResetService` accepts
the fixed auth base URL and trusted `OriginPolicy`; requests validate email and
redirect before any storage or delivery work. Callback redirects are bounded to
8,192 UTF-8 bytes. Existing request Origin/rate/body policies remain in force.

Missing users receive the same status/message without delivery or a persisted
token. A discarded random token and dummy verification lookup follow upstream's
timing-mitigation shape, without a constant-time claim. Existing users, including
those without a credential account, get a random 24-character ASCII base62 token
with unbiased sampling and a configurable expiry (one hour by default). Email lookup lowercases the input.
The Verification record uses the existing `reset-password:<token>` identifier
and user-ID value. Configured PostgreSQL storage encrypts it; custom AuthStore
implementations own their storage security. POST `/reset-password` consumes the
record through the existing expiry/replay rules; its GET callback only checks the
record and redirects. Existing sessions retain the prior reset
policy. No token is returned by the request endpoint or logged.

Delivery receives an owned `PasswordResetDeliveryContext`: public user projection,
reset URL, and token. No Debug/Serde or raw request/cookie/session fields are
provided. Selected public values must be scalar (strings at most4,096 UTF-8 bytes,
booleans for flags, or null), preventing nested private fields from hitchhiking in
the projection. Token and URL storage zeroizes on drop; callback code must never
log them. The link uses the configured auth base and encodeURIComponent-compatible
redirect encoding. Configurable token TTL, arbitrary custom user projections and
raw JavaScript request context remain separate options.

The token commits before `prepare` is called, with no transaction held across the
returned future. Preparation `Err` is a sanitized empty500; asynchronous delivery
and dispatcher errors preserve the successful response and committed token.
Cancellation while waiting for the writer or before submitting commit leaves no
token; cancellation during a database commit can have an uncertain outcome. After
commit, cancellation can leave a valid token even if no delivery completes. This
is not durable mail queuing.

`AuthHttp::with_background_dispatcher` configures the reusable
`background::AuthBackgroundDispatcher` for reset delivery and inherited OTP
scheduling. Explicit OTP-specific dispatchers take precedence in either builder
order. The existing `OtpBackgroundTask`, `OtpBackgroundDispatcher` and
`OtpDeliveryFuture` remain type-compatible aliases, not competing runtime systems.
No dispatcher means default-await delivery. Applications own lazy task polling,
queue limits, runtime lifetime, draining and cancellation as described above;
there is no implicit executor or retry.

Explicit verification-email delivery can be enabled with
`AuthHttp::with_email_verification_delivery(Arc<dyn EmailVerificationDelivery>)`.
The trusted callback receives an owned, bounded public-user projection and borrowed
accessors to its zeroized token/URL; the context has no Debug or Serde implementation.
The explicit send endpoint always awaits the returned `DeliveryFuture`, even when a
shared background dispatcher is installed. Preparation or delivery errors produce a
sanitized empty 500. Anonymous missing/already-verified requests return the same
success without delivery; anonymous delivery success/failure observes a 500ms floor
(not a constant-time guarantee). Authenticated requests require matching email and
reject already-verified users. Callback redirects must be trusted and at most 8192 bytes.

Issued HS256 tokens use the configured auth secret, lowercase email and a one-hour
expiry. The existing GET verifier marks the current tenant-local email owner verified;
it does not create a session by default. These are email-address proofs, not immutable
user-ID proofs: replay succeeds while unexpired and email reassignment may change the
owner to whom a token applies. No purpose, single-use record or email-change authority
is added by this issuance API. Expiry equality rejects the proof. Delivery remains
disabled unless installed. Automatic signup/signin delivery, configurable expiry,
email-change forwarding, before/after hooks and auto-sign-in remain separate options.

`AuthHttp::with_email_verification_policy(EmailVerificationPolicy { require,
send_on_signup, send_on_signin })` separately enables automatic email/password
verification policy. Defaults preserve authenticated signup/signin and send no mail,
even if the manual delivery hook is installed. `send_on_signup: None` follows
`require`; explicit false overrides it. `send_on_signin` sends only for a correct
password when required verification is still incomplete. Required signup returns
`token:null` without cookies/JWT/session; signin returns 403 `EMAIL_NOT_VERIFIED`
before creating an MFA challenge or consuming device trust. A missing hook is valid:
the application may verify addresses through another trusted path.

Automatic delivery uses the shared background dispatcher (or awaits by default),
handles asynchronous delivery errors, and propagates preparation errors as empty 500.
Signup constructs a prospective public user before delivery, then checks uniqueness
again and atomically inserts the user/account and optional session. No database
transaction spans callback code. Unlike upstream callback transaction visibility,
that prospective user is not yet persisted; callbacks cannot rely on reading it.
Cancellation or final commit failure can therefore leave delivered mail without an
account. A concurrent winner is never overwritten. Required duplicate signup hashes
the supplied password and returns a fresh synthetic profile from submitted fields,
not the existing user's data. These boundaries preserve atomic native persistence,
not arbitrary callback side-effect rollback.

For direct services, install the same policy with
`CredentialService::with_email_verification_policy` and use `sign_up_outcome` to
handle `SignupOutcome::VerificationRequired`. Legacy `sign_up` rejects required
policy before side effects because its return type requires an authenticated grant.
Email verification is a service configuration, not a global user login prohibition.
Independent custom synthetic-user/existing-user hooks, social
policy and verification before/after hooks remain separate capabilities.

`AuthHttp::with_signup_auto_sign_in(false)` and the corresponding direct
`CredentialService` builder suppress only signup session issuance. The default is
true. New and duplicate signup return `token:null` without authentication cookies;
subsequent correct-password signin remains enabled unless verification policy
independently requires an already-verified address. Disabling signup auto-signin
does not turn on verification delivery: its default still follows `require`.
Required verification takes precedence over auto-signin true.

`sign_up_outcome` returns the new `SignupOutcome::Unauthenticated { user }` variant
for this independent no-session option; existing `VerificationRequired` is retained.
Consumers exhaustively matching the public enum must add the new variant. The legacy
`sign_up` API returns `SIGNUP_SESSION_DISABLED` before effects when auto-signin is
false (or `EMAIL_NOT_VERIFIED` if required verification also applies). Required and
auto-signin-disabled duplicate signup share the existing synthetic-profile privacy,
password hashing, callback ordering and final uniqueness/atomicity rules.

`CredentialService::with_revoke_sessions_on_password_reset(true)` and the matching
`AuthHttp` builder opt into revoking the user's sessions after a password reset;
the default remains false. The tenant-bound indexed query deletes all matching
sessions, including expired ones, without deleting another user's sessions.
Password/proof commits precede the separate atomic session-deletion transaction:
if revocation fails, the reset returns an error but the new password and consumed
proof remain committed, and session deletions roll back. This matches the observed
upstream partial-failure boundary, rather than making the whole reset atomic.
The reset response does not clear browser cookies. This option does not install
an `onPasswordReset` callback.

`with_password_reset_observer(Arc<dyn PasswordResetObserver>)` on
`CredentialService` or `AuthHttp` installs the bounded `onPasswordReset` hook.
Its `prepare` receives an opaque `PasswordResetObserverContext`; `user_json()`
exposes the bounded native public-user snapshot captured before the password
update. The owned JSON string is zeroized on drop, with no Debug/Serde, password,
reset token, session or raw Request. Callback-owned copies remain application data.
Arbitrary additional upstream user fields and raw Request access are outside this
callback profile. Selected user fields are validated before cloning; a projection
failure is reported after password commit, while no-hook operation skips projection.
The callback is always awaited outside database transactions after password commit
and before optional session revocation, regardless of the background dispatcher.
Preparation/future errors produce empty HTTP 500; cancellation leaves the changed
password and consumed proof committed without running later revocation. A callback
may reenter storage/authentication; successful completion then revokes the current
sessions if configured, including ones it created. Blocking code and panics are
outside the fallible cooperative callback contract.

`AuthHttp::with_before_email_verification` and
`with_after_email_verification` independently install trusted
`EmailVerificationObserver` callbacks. Both receive an opaque, zeroized owned
public-user JSON snapshot through `EmailVerificationObserverContext::user_json()`;
there is no Debug/Serde, raw Request, credential or arbitrary custom-field access.
Selected borrowed fields are bounded and validated before cloning. No-hook paths
skip this projection. Both preparation and the future are awaited outside database
transactions, regardless of the background dispatcher.
Ordinary unverified proofs call before with `emailVerified:false`, commit the
verification, then call after with `emailVerified:true`. Already-verified replays
skip both. Change-email verification calls after only, after its user/session commit
but before response cookies; legacy change and confirmation branches call neither.
Before errors/cancellation prevent this handler's verification update (callback-owned
effects remain the application's responsibility). After errors/cancellation
leave committed user/session state intact. Callback errors return empty HTTP500
without redirects or response cookies; this also suppresses any pending refresh
cookie from earlier processing. The native writer retains its ID/email recheck
after the before callback, rejecting a changed address/owner instead of applying a
stale update. This stronger callback race boundary is distinct from upstream's
email-based update. Tokens retain the existing address-owner, reassignment and
replay semantics, not single-use or ID-bound authority. These hooks do not enable
`autoSignInAfterVerification`. Callback-owned copies, blocking code and panics are
outside the bounded cooperative context contract.

`AuthHttp::with_auto_sign_in_after_verification(true)` opts into ordinary newly
verified email-proof login after the successful after-verification hook; default
false, already-verified replay, and independent change-email login behavior remain
unchanged. A valid current session for the same user is reused; absent, invalid,
expired or other-user sessions cause a new session for the verified user. Redirects
retain the login cookie. A signed `dont_remember` marker suppresses cookie Max-Age
and is retained, while new persisted sessions keep the configured one-day lifetime.
Expired-session cleanup/refresh commits before new-session creation. Unexpected
creation/commit failure returns empty HTTP500 without response cookies, preserving
verified user state and already-committed expired-session cleanup. Upstream's
custom-adapter null session result (`FAILED_TO_CREATE_SESSION`, including its
retained pending clear cookies) cannot be represented by native `AuthStore::insert`
returning `Result<()>`; native storage errors are unexpected failures, not that
handled null case. After-hook failure/cancellation creates no login session.
A final tenant-bound writer rechecks target ID, email and verified status and loads
or creates the session; it refuses stale callback authority if the user was deleted,
changed address or became unverified. This is deliberately stronger than upstream
snapshot-based session creation. No database transaction spans a hook await.

### Shared session policy

`SessionPolicy::new(expires_in, update_age, fresh_age, disable_refresh)` accepts
trusted whole-second `u32` values, with positive expiry. `Default` preserves the
ForkLaunch preset: 86,400-second expiry/freshness and 3,600-second refresh age.
This differs from upstream Better Auth's unconfigured seven-day expiry. Zero
update age permits refresh on every read; zero freshness disables the age check.
Update age may exceed expiry. Negative/fractional values and upstream's zero-expiry
fallback are outside this typed profile; use an explicit positive expiry instead.
Timestamp arithmetic is checked before persistence.

Apply `AuthHttp::with_session_policy(policy)` once to configure credential,
provider, factor, verification, organization session reads and session cookies.
It preserves installed factor options regardless of builder order. Direct
`SessionService`, `CredentialService`, `GoogleAccounts`, `OrganizationService`
and `TwoFactorService` callers have the same additive builder and must choose a
consistent policy when composing services themselves. Existing constructors and
`SessionCookie::serialize` retain their defaults; `serialize_with_policy` exposes
configured cookie serialization without changing the public cookie struct.

Persistent creation uses configured expiry and cookie Max-Age. Explicit
`rememberMe:false` credential login and pending MFA completion instead retain
upstream's fixed one-day stored lifetime and omit cookie Max-Age. Enrollment,
disable, password rotation and automatic email-verification login use configured
expiry even when a signed transient marker suppresses persistent cookies, because
those upstream calls create a session without the transient creation argument.
Existing session expiry remains authoritative: changing policy affects refresh
thresholds/current freshness checks and future grants, without rewriting old rows.
Expiry equality is valid, refresh-threshold equality refreshes, freshness equality
rejects unless freshness is zero, and disabled refresh still cleans expired rows.
JWT expiration and MFA challenge/trust/OTP lifetimes remain independent. Deferred
GET/POST refresh, secondary storage and cookie-cache options remain separate work.

Direct domain methods continue to use their caller-supplied `now`. HTTP services
use the trusted request clock at authoritative read and session issuance/refresh
boundaries, including after awaited hashing, provider work and delivery hooks.
The clock propagation and stale-authority limits are described below; this does
not promise a full TTL remaining when database commit or response delivery ends.


### Trusted credential and token options

`CredentialPolicy::new(disable_signup, minimum, maximum)` installs the email/password
signup switch and UTF-16 password-length limits through `CredentialService`, `AuthHttp`,
and `TwoFactorService`. Zero limits select Better Auth's 8/128 defaults; effective
limits are bounded to 1..=4096. Inverted bounds remain admissible: new-password checks
report minimum failure before maximum failure, while existing-password proofs apply
only the maximum. HTTP body validation still precedes the disabled-signup handler.
The policy never comes from request fields. Installing the policy before or after MFA
configuration preserves the other options.

`AuthTokenLifetimes::new(reset_seconds, verification_seconds)` is a whole-second `u32`
profile, installed with `with_token_lifetimes` on `AuthHttp`, `CredentialService`, or
`PasswordResetService`. Reset zero selects 3600 seconds; verification zero is honored.
Issuance uses checked UTC arithmetic. Existing proofs retain their issued expiry.
Manual verification delivery and automatic signup/signin delivery share the configured
verification lifetime. Native `/change-email` remains disabled and `/delete-user`
remains unavailable; this group does not implement their upstream opt-in policies.
Existing change-email proof processing does not issue new change-email tokens. Generic
JavaScript negative/fractional option values are outside these typed constructors.

Direct services preserve the explicit caller-supplied timestamp contract. HTTP-driven
services carry a private trusted clock and resample after hashing, delivery hooks and
authoritative reads, and immediately before session construction/refresh. Both clock
and MFA builder orders preserve this behavior. Manual reset/verification issuance also
samples after its authoritative user lookup; existing codec lease clocks remain intact.

Original session/challenge expiry is checked again after awaited replacement writes,
before returning to commit. An expired authority cannot produce a replacement session;
failed factor/password mutations roll back atomically. Refresh that crosses the original
expiry during its write removes the expired session instead of reviving it. This is a
stronger native stale-write boundary than arbitrary upstream adapter races. A database
commit can itself take time: there is no post-commit rollback or exact response-time TTL
guarantee. Authenticated OTP operations that do not issue a new session retain their
existing single-sample clock behavior. Date sampling outside these issuance/refresh
paths, third-party hooks and custom store isolation remain distinct contracts.
Organization routes also use the fresh clock for their secondary transaction-local
session/user rechecks, after acquiring the operation transaction. This covers the
existing organization, active selection, dynamic role, member, team and invitation
recheck paths; it preserves the initial refresh and direct explicit-time API behavior.
A session that expires during those awaited authority reads rejects the operation
without committing graph/selection changes. Exact session-expiry equality remains
valid. These are authorization snapshots, not a guarantee that an authority cannot
expire after the last read or during subsequent work/commit; unrelated organization
record timestamps and invitation lifetime policies are unchanged.

### Trusted scalar organization policy

`OrganizationService::with_organization_policy(OrganizationPolicy::new(allow_create,
organization_limit, membership_limit))` and the matching `AuthHttp` builder install
server-owned policy. Defaults remain `true`, `None`, and `100`; request JSON cannot
change the policy. This typed subset admits nonnegative whole `u32` limits, not
JavaScript negative/fractional/nonfinite values or asynchronous policy callbacks.

`Some(0)` organization limit forbids creation, whereas membership limit `0` falls
back to `100`. Creation checks session authority, allow-create, the organization
limit, then duplicate slug. As in the configured default adapter, the organization
limit counts the first **100 matching membership rows**, including non-owner,
duplicate and orphan memberships. It is not an unbounded owner-only quota; limits
above 100 are never reached by that window. Disabling creation does not remove
existing organizations. Trusted server creation using a supplied user ID without
a session is not introduced by this builder.

Membership limits apply to trusted server-only `add_member` and authenticated
invitation acceptance after their existing user/duplicate/team/organization checks.
The existing 4096-record scan bound still fails explicitly rather than truncating
admission counts. Native write transactions serialize count-and-insert, providing a
stronger concurrent admission boundary than arbitrary upstream adapter races.
Existing members are retained when policy decreases. The upstream full-organization
reader applies this option to **user hydration**, not to the member join limit;
reading a pre-existing larger graph can therefore fail with the existing sanitized
storage error unless the caller supplies a smaller `membersLimit`. No implicit
member truncation or policy migration occurs. Session/clock and MFA builders retain
the selected organization policy independently.

### Trusted invitation policy

`InvitationPolicy::new(expires_in_seconds, invitation_limit, require_verified_email)`
configures `OrganizationService::with_invitation_policy` and
`AuthHttp::with_invitation_policy`. Defaults remain 172800 seconds, 100 pending
invitations and verified recipients. Expiration and count admit whole nonnegative
`u32` values: zero expiration falls back to 48 hours, while zero quota rejects new
invitations. Resending an existing unexpired invitation bypasses quota and renews
its expiration using the current policy. Time addition is checked before writes.
The count examines the first 100 matching pending rows before excluding expired
rows, matching the configured upstream adapter window; limits above 100 are not
an unbounded organization quota. Existing bounded scans remain fail-closed.

The verification flag applies only to invitation-by-ID get, accept and reject.
Listing invitations for the session email still requires verification, even when
this flag is false. Recipient email matching and tenant/organization authorization
remain mandatory. The default `true` preserves the configured reference's custom
ID-generator behavior; this API does not infer upstream defaults for arbitrary
ID generators. Configuration is server-owned; HTTP request fields cannot alter it.
Other organization and session policy builders retain this independent policy.

New-invitation admission and writes remain one tenant transaction, including
concurrent quota checks. Failures leave existing invitations unchanged. Policy
changes do not rewrite old expiration values; resend does. Cancellation on
reinvite and invitation delivery hooks remain separate options: upstream can
persist cancellation before a later admission error and persists invitations
before invoking delivery. This policy slice does not claim those behaviors.

### Cancellation on reinvite

`with_cancel_pending_invitations_on_reinvite(true)` on `OrganizationService` or
`AuthHttp` enables the trusted `cancelPendingInvitationsOnReInvite` option;
its default is false. Explicit `resend: true` takes precedence, renewing the same
pending invitation without cancellation or quota/team admission checks.
Otherwise, role/recipient/organization checks precede cancellation. The exact
selected pending invitation is marked canceled and committed before quota/team
checks and replacement insertion, matching upstream's observable partial-failure
boundary. A failed replacement does **not** restore the canceled invitation.
Failure to commit cancellation leaves the original pending invitation intact.

The replacement uses a second tenant writer transaction with fresh session and
role authorization. The canceled predecessor must still match, including actor
and organization. An ID/value snapshot of the already bounded store-read matching unexpired
pending rows permits unchanged preexisting duplicate pending invitations; only the
first is canceled, then a replacement is created as upstream does. Newly observed
or changed pending rows deny admission, and an intervening invitation is never canceled by an
automatic retry. Missing/changed predecessor yields `INVITATION_STATE_CHANGED`
(HTTP 409); revoked authority or a concurrent replacement denies admission through
its existing error. These are stronger concurrency safeguards than upstream's
independent adapter calls, not a claim of reproducing unsafe stale races. No user
callback runs inside either transaction. This option does not send invitation
email; delivery remains a separate capability. A caller canceled after the first
commit can leave a canceled predecessor without a replacement, as with any
interruption at this explicit partial-commit boundary.

A storage error after a successful cancellation COMMIT can leave the predecessor
canceled despite the error response. An error acknowledging the second COMMIT can
leave both canceled predecessor and pending replacement persisted. These outcomes
are not rolled back or automatically retried; callers must inspect current state.

Snapshot comparison includes the matching rows already returned by the bounded
store scan (maximum4096); it does not expand the existing first100 selection or
quota windows. Thus a known101st pending row shifting into the lookup window is
accepted, while a newly inserted row is rejected. Stable ID/value sets, rather
than row order alone, determine whether preexisting duplicates remain unchanged.

### Trusted invitation email delivery

`with_invitation_delivery(Arc<dyn InvitationEmailDelivery>)` on `AuthHttp` or
`OrganizationService` implements the optional `sendInvitationEmail` callback.
`prepare(InvitationDeliveryContext)` returns the existing owned `DeliveryFuture`.
By default its future is awaited; `with_background_dispatcher` transfers it to
application-owned scheduling, with the same dispatcher used by other auth delivery
paths. Rust futures are lazy, unlike JavaScript promises; the application owns
polling, capacity, shutdown draining and dropping accepted tasks. No implicit task
spawn, hard timeout, panic handling or cancellation of external email is promised.

The opaque context has `id`, `role`, lowercased `email`, `organization`,
`invitation`, and `inviter` accessors; `inviter` contains the member and its nested
configured public user. There is no raw Request, session token or arbitrary custom
field export. Selected scalar fields are validated while borrowed before cloning,
with at most4096 UTF-8 bytes per string and configured boolean/null user fields.
Organization metadata in this bounded profile is string/null, as persisted by the
native configured creation path. Unknown fields are omitted; malformed selected
fields cause a sanitized preparation failure. Context has no Debug/Serde; the
application is responsible for any data it deliberately copies or logs.

Snapshots come from the authoritative write transaction, but projection errors
are deferred until persistence succeeds. No callback runs inside a transaction.
Successful creation or resend commits first, then invokes preparation exactly once.
A preparation Result error returns an empty HTTP500 **after** the invitation/new
expiry persists. Future rejection or scheduler Result error is logged with static
sanitized fields and the successful HTTP200 response is preserved. A future may
reenter the store without retaining the admission writer lock. Cancellation-only
admission failures and storage/COMMIT errors invoke zero callbacks, including an
uncertain commit acknowledgement even if the row actually persisted. A request
interrupted after commit can leave a persisted invitation without completed email;
this is not durable email delivery. The disabled/default path performs no callback
projection and retains its prior response behavior. Policy builders preserve the
installed delivery hook and shared dispatcher.

### Trusted scalar team policy

`OrganizationService::with_team_policy` and `AuthHttp::with_team_policy` accept
`TeamPolicy::new(default_team, maximum_teams, maximum_members, allow_removing_all)`.
Defaults are `true, None, None, false`; the typed limits admit nonnegative `u32`
values. `Some(0)` teams means unlimited, whereas `Some(0)` members denies new
pairs and still permits reusing an existing membership. These are server options,
not request fields. Other builders preserve them.

The policy applies to organization default-team creation, team creation/removal,
team membership addition, server-only organization-member addition, invitation
admission, and invitation acceptance. Default-team creation bypasses both limits,
matching the plugin; disabling it preserves the prior active team while selecting
the newly created organization. Team quotas and invitation hydrated-member checks
retain upstream first-100 windows, so they are not unbounded quotas. Actual seat
admission retains the existing stored overcount and bounded actual member scan.
It atomically admits one new pair per available seat and leaves existing pairs
unchanged. Invitation resend bypasses the new-invitation team checks.

Native writer transactions serialize last-team deletion, quotas, and seat admission.
Rejected seat admission also rolls back counter synchronization; upstream can
persist an undercount correction before returning the same limit error.
Failures roll back their operation, unlike upstream compensating cleanup whose
intermediate effects can be visible. No arbitrary asynchronous policy/default-team
creator callbacks, additional schemas, or cleanup-failure equivalence is implied.

### Organization deletion and role quotas

`OrganizationControls::new(disable_deletion, maximum_roles)` is installed through
`OrganizationService::with_organization_controls` or the equivalent `AuthHttp`
builder. Defaults are deletion enabled and no role quota. The optional `u32` quota
includes every matching dynamic-role record; zero denies new roles. It uses the
indexed total under the tenant's writer transaction, rather than the first-100
permission-hydration window. As elsewhere with indexed counts, off-page ciphertext
is not decrypted and the total assumes store-managed index integrity.

Disabled deletion returns `404 ORGANIZATION_DELETION_DISABLED` after normal HTTP
input/origin admission but before any session lookup, refresh, or storage access.
Role quotas follow predefined-name, membership, and `ac:create` checks, and precede
permission-resource validation and duplicate stored-name checks. The quota grants
no permissions itself. Writer serialization prevents concurrent admissions from
exceeding the last slot; default deletion behavior remains unchanged. These are
trusted scalar options, not HTTP body fields or arbitrary policy callbacks.

### Configured creator role

`CreatorRole::new(value)` and `with_creator_role` on `OrganizationService` or
`AuthHttp` select the trusted organization creator identity. The default and
empty-string fallback are `owner`; other UTF-8 values up to 4096 bytes are kept
exactly, including whitespace and commas. This setting applies to the initial
membership, invitation creator-role admission, member removal/role changes,
leaving, and last-creator protection. It does not register a dynamic role or
change the configured static grants. Creator permission bypass is limited to
member-role updates and still validates stored dynamic grants first.

The existing adapter profile counts creators in the first 100 organization
members. Role token comparisons preserve upstream endpoint-specific trimming;
a configured comma-containing value is stored intact but does not match its
individual split tokens. Membership mutations retain tenant/session authority
checks and writer-transaction serialization, including concurrent last-creator
changes. HTTP body fields cannot override this server-owned setting. Async
organization policy callbacks and general custom role definitions are separate
capabilities.

### Declarative organization access control

`OrganizationAccessControl::new(statements, roles)` accepts owned
`OrganizationStatements` and `OrganizationRoles` maps. Install it with
`with_access_control` on `OrganizationService` or `AuthHttp`. Omitting this builder
retains the configured platform resource and owner/admin/editor/system/viewer
roles. An explicitly supplied role map replaces that preset, including when it is
empty; names and action strings are not trimmed or normalized. The bounded typed
profile permits 128 roles, 128 resources per map, 4096 total role/resource/action
entries, 4096 UTF-8 bytes per string, and 1 MiB aggregate string bytes.

Stored dynamic statements extend the selected static role with unique actions,
using the existing tenant-scoped first-100 role window. Different member roles
are alternatives: a request must be satisfied by one role, not a union of grants
from several roles. Dynamic-role creation/update validates resource names against
the catalog and requested actions against the actor's grants; the catalog's
listed actions are not an extra runtime restriction on static role statements.
Static names protect dynamic-role creation, renaming, and name-based deletion.
Invitation and member-role admission recognizes upstream owner/admin/member plus
the supplied static names, then checks the existing dynamic-role profile. All
organization/team/member/invitation permission checks share the same immutable
configuration. HTTP bodies cannot install policy. The separate ForkLaunch
scoped permission-surfacing service retains its explicit TS platform mapping.

This implements declarative `createAccessControl`/`newRole` statements, not
arbitrary JavaScript `Role.authorize` overrides. The upstream override is
synchronous and a stored dynamic row for the same role replaces it with
`ac.newRole`; a bounded trusted custom-authorizer extension remains separate.
Explicit own role/resource keys, including `__proto__`, `constructor`, and
`toString`, are retained. Permission-record admission strips `__proto__` without
validating its value, as the pinned Zod record parser does. Absent inherited
role/resource lookup and dynamic-merge faults return sanitized empty 500s,
including role-list short-circuit ordering and JavaScript integer-index-first
resource iteration. The finite Role-object prototype introduced by a dynamic
`__proto__` row is modeled explicitly: repeated merges, inherited
`authorize`/`statements` faults, later own-role shadowing, and statement-array
spread into string-valued index resources. No JavaScript is evaluated.
No upstream in-memory role cache or arbitrary JavaScript receiver behavior is claimed.

Remaining organization option families after these scalar/declarative profiles:

| Family | Implemented scope | Remaining scope |
| --- | --- | --- |
| Creation/membership/dynamic-role limits | Trusted scalar values, creator identity, deletion toggle | Async policy callbacks |
| Teams | Default creation toggle, team/seat limits, last-team policy | Plugin route disabling, custom default-team creation, async limits |
| Invitations | TTL, quota, verification, cancellation, trusted delivery/scheduler | Async quota callback and lifecycle hooks |
| Access control | Declarative catalog/static roles and persisted dynamic grants | Custom synchronous authorizers and plugin-disable topology |
| Lifecycle hooks | Invitation delivery only | Before/after organization, member, invitation, team and team-member hooks |
| Storage/schema | Fixed configured models and fields, encrypted tenant store | Arbitrary model/field remapping and additional-field schema definitions |

These are remaining capabilities, not exclusions from the requested broader
Better Auth parity scope.
