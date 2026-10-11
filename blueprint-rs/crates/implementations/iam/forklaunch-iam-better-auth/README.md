# Native Better Auth module

This crate implements the configured Better Auth 1.7.6 protocol incrementally.
It currently provides compatible scrypt password hashes and signed session
cookies, Ed25519 JWT issuance and public JWKs. It does not yet expose the authentication HTTP API or
organization endpoints and is not selectable as a CLI module preset.

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

`jwt::SigningKey` issues the configured EdDSA payload (subject, email, active
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
key replacement as well. Token/JWKS HTTP endpoints remain to be integrated with
persistent sessions.

`sessions::SessionService` implements native authoritative session creation,
one-hour refresh, creation-based freshness, scoped revocation, sign-out and
transient/expired-session cleanup. Memory and live PostgreSQL lifecycle tests
pass. The actual configured HTTP fixture compares 48 native response bodies and
stored states after session operations; credential endpoints seed identical
records, so this does not claim native sign-up/sign-in. Signed cookie extraction
is exercised at the fixture boundary; production HTTP cookie headers/clearing,
CSRF/Origin guards and the CLI preset remain to be integrated. Session queries
currently use the explicitly bounded store scan; indexed lookup remains open.

`accounts::CredentialService` provides atomic native email signup/signin. Scrypt
runs in a shared bounded blocking pool outside tenant write locks; concurrent
signup rechecks email uniqueness inside the write transaction, and signin
rechecks the account after password verification to reject concurrent password
changes. Thirty-nine actual-handler comparisons cover response/stored state,
case normalization, callbacks and Unicode password limits; memory and live
PostgreSQL exercise concurrent signup and persistent credential/session records.
Credential DTOs are parsed inputs: HTTP media/body validation, CSRF/Origin and
callback guards still belong to the pending HTTP adapter. Password/user/account
updates and provider linking remain open.
